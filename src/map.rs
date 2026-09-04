//! Bounded repository map (roadmap §8, §11 Phase 6).
//!
//! `cx overview` stays the cheapest entry point; `map` is the heavier
//! orientation command. It reports only provable facts — directory grouping,
//! file and symbol counts, test/vendor/generated classification, and
//! include/import edges that actually resolve to an indexed file. An import that
//! cannot be resolved is counted as external or ambiguous, never turned into a
//! guessed edge.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::index::{FileData, Index, SymbolRole};
use crate::util::glob::glob_match;

/// How many dependency names and API samples a row may list.
const ROW_LIST_LIMIT: usize = 5;

/// Symbol names that carry almost no orientation value.
///
/// Without this, ranking and API samples are dominated by `run`, `get`, `name`
/// and friends — the failure mode called out in roadmap §8.
const LOW_INFORMATION_NAMES: &[&str] = &[
    "add", "apply", "args", "begin", "build", "call", "clear", "clone", "close", "config",
    "context", "count", "ctx", "data", "default", "drop", "empty", "end", "eq", "error",
    "execute", "fmt", "from", "get", "handle", "hash", "id", "index", "init", "into", "invoke",
    "item", "key", "kind", "len", "list", "log", "main", "map", "name", "new", "next", "node",
    "ok", "open", "options", "params", "parse", "print", "process", "read", "remove", "reset",
    "result", "run", "set", "size", "start", "state", "status", "stop", "str", "string", "to",
    "to_string", "type", "update", "value", "write",
];

/// What a path is, by convention (roadmap §8 filters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathClass {
    Production,
    Test,
    Docs,
    Vendor,
    Generated,
}

impl PathClass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Test => "test",
            Self::Docs => "docs",
            Self::Vendor => "vendor",
            Self::Generated => "generated",
        }
    }
}

const VENDOR_DIRS: &[&str] = &[
    "vendor", "vendored", "third_party", "thirdparty", "3rdparty", "node_modules", "external",
    "extern", "deps", "site-packages", ".venv", "venv", "bundle",
];

const GENERATED_DIRS: &[&str] = &[
    "generated", "gen", "autogen", "build", "dist", "out", "target", "__pycache__", ".next",
];

const DOC_DIRS: &[&str] = &["docs", "doc", "documentation"];

/// Classify a repository-relative path.
///
/// Vendor and generated win over test and docs: a vendored test file is still
/// vendored, which is what a filter needs to know.
pub fn classify(path: &Path) -> PathClass {
    let components: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect();

    if components.iter().any(|c| VENDOR_DIRS.contains(&c.as_str())) {
        return PathClass::Vendor;
    }
    if components.iter().any(|c| GENERATED_DIRS.contains(&c.as_str())) {
        return PathClass::Generated;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.contains(".gen.") || name.ends_with("_pb2.py") || name.ends_with(".pb.go") {
        return PathClass::Generated;
    }
    if crate::query::is_test_path(path) {
        return PathClass::Test;
    }
    if components
        .iter()
        .take(components.len().saturating_sub(1))
        .any(|c| DOC_DIRS.contains(&c.as_str()))
        || matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("md" | "markdown" | "mdown")
        )
    {
        return PathClass::Docs;
    }
    PathClass::Production
}

/// Result of trying to turn one written import into an indexed file.
#[derive(Debug, PartialEq, Eq)]
enum Resolved {
    /// Exactly one indexed file matches.
    File(PathBuf),
    /// Nothing in this project matches: a system or third-party dependency.
    External,
    /// Several indexed files match, so no edge is claimed.
    Ambiguous,
}

/// Resolve an import target to an indexed file, by path only.
///
/// Deliberately syntactic: C/C++ include paths and TypeScript relative
/// specifiers are written paths, so matching them against indexed paths is a
/// fact. Rust module paths are matched against the conventional file layout and
/// are reported as external when that layout does not apply.
fn resolve_import(
    importer: &Path,
    import: &str,
    language: &str,
    indexed: &BTreeSet<&PathBuf>,
) -> Resolved {
    let candidates: Vec<String> = match language {
        "c" | "cpp" => {
            // Suffix match: `#include "ange/ecs.hpp"` may live under any include root.
            let matches: Vec<&PathBuf> = indexed
                .iter()
                .filter(|p| {
                    let text = p.to_string_lossy().replace('\\', "/");
                    text == import || text.ends_with(&format!("/{import}"))
                })
                .copied()
                .collect();
            return match matches.len() {
                0 => Resolved::External,
                1 => Resolved::File(matches[0].clone()),
                _ => Resolved::Ambiguous,
            };
        }
        "typescript" => {
            if !import.starts_with('.') {
                // Bare specifier: a package, not a file in this project.
                return Resolved::External;
            }
            let base = importer.parent().unwrap_or(Path::new(""));
            let joined = crate::util::path::lexical_join(base, import);
            let stem = joined.to_string_lossy().replace('\\', "/");
            ["ts", "tsx", "js", "jsx"]
                .iter()
                .map(|ext| format!("{stem}.{ext}"))
                .chain(
                    ["ts", "tsx", "js", "jsx"]
                        .iter()
                        .map(|ext| format!("{stem}/index.{ext}")),
                )
                .collect()
        }
        "rust" => {
            let Some(rest) = import.strip_prefix("crate::") else {
                // `std::`, an external crate, or a `self::`/`super::` form the
                // conventional layout cannot pin down.
                return Resolved::External;
            };
            let segments: Vec<&str> = rest.split("::").collect();
            let mut candidates = Vec::new();
            // `crate::a::b::Item` may live in a/b.rs, a/b/mod.rs, or a.rs.
            for take in (1..=segments.len()).rev() {
                let joined = segments[..take].join("/");
                candidates.push(format!("src/{joined}.rs"));
                candidates.push(format!("src/{joined}/mod.rs"));
                candidates.push(format!("{joined}.rs"));
            }
            candidates
        }
        _ => return Resolved::External,
    };

    for candidate in candidates {
        let as_path = PathBuf::from(&candidate);
        if let Some(found) = indexed.iter().find(|p| **p == &as_path) {
            return Resolved::File((*found).clone());
        }
    }
    Resolved::External
}

/// Subsystem key for a path: its first `depth` components, or the file itself
/// when it sits at the root.
fn subsystem_of(path: &Path, depth: usize) -> String {
    let parts: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    if parts.len() <= 1 {
        return "(root)".to_string();
    }
    let take = depth.clamp(1, parts.len() - 1);
    format!("{}/", parts[..take].join("/"))
}

/// One subsystem row.
#[derive(Serialize)]
pub struct MapRow {
    pub subsystem: String,
    pub class: String,
    pub files: usize,
    pub symbols: usize,
    pub tests: usize,
    /// Subsystems this one imports from, resolved to indexed files.
    pub depends_on: String,
    /// How many other subsystems import this one (fan-in).
    pub dependents: usize,
    /// Imports that resolve to no indexed file (system or third-party).
    pub external_imports: usize,
    /// Representative symbol names, low-information names removed.
    pub api: String,
}

/// Options for [`build`].
pub struct MapOptions<'a> {
    pub depth: usize,
    pub include_vendor: bool,
    pub include_generated: bool,
    pub include_tests: bool,
    pub exclude_globs: &'a [String],
}

/// The computed map plus the facts needed to explain it.
pub struct MapReport {
    pub rows: Vec<MapRow>,
    /// Human-readable, machine-checkable notes: what was excluded and why, and
    /// what the ranking means.
    pub notes: Vec<String>,
    pub ranked_by: &'static str,
}

/// Build a bounded repository map from the index.
pub fn build(index: &Index, opts: &MapOptions<'_>) -> MapReport {
    let mut excluded: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut excluded_by_glob = 0usize;

    // 1. Select the files this map covers.
    let mut included: Vec<(&PathBuf, &FileData, PathClass)> = Vec::new();
    for (path, data) in &index.entries {
        let class = classify(path);
        let keep = match class {
            PathClass::Vendor => opts.include_vendor,
            PathClass::Generated => opts.include_generated,
            PathClass::Test => opts.include_tests,
            PathClass::Production | PathClass::Docs => true,
        };
        if !keep {
            *excluded.entry(class.as_str()).or_insert(0) += 1;
            continue;
        }
        let display = path.to_string_lossy().replace('\\', "/");
        if opts
            .exclude_globs
            .iter()
            .any(|pattern| glob_match(pattern, &display))
        {
            excluded_by_glob += 1;
            continue;
        }
        included.push((path, data, class));
    }

    let indexed: BTreeSet<&PathBuf> = included.iter().map(|(p, _, _)| *p).collect();

    // 2. Aggregate per subsystem.
    struct Acc {
        classes: BTreeSet<PathClass>,
        files: usize,
        symbols: usize,
        tests: usize,
        depends_on: BTreeSet<String>,
        external_imports: usize,
        ambiguous_imports: usize,
        api: BTreeSet<String>,
    }
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();

    for (path, data, class) in &included {
        let key = subsystem_of(path, opts.depth);
        let entry = acc.entry(key.clone()).or_insert_with(|| Acc {
            classes: BTreeSet::new(),
            files: 0,
            symbols: 0,
            tests: 0,
            depends_on: BTreeSet::new(),
            external_imports: 0,
            ambiguous_imports: 0,
            api: BTreeSet::new(),
        });
        entry.classes.insert(*class);
        entry.files += 1;
        entry.symbols += data.symbols.len();
        if *class == PathClass::Test {
            entry.tests += 1;
        }

        for symbol in &data.symbols {
            if symbol.role != SymbolRole::Definition || symbol.is_test {
                continue;
            }
            let lowered = symbol.name.to_lowercase();
            if symbol.name.len() <= 2 || LOW_INFORMATION_NAMES.contains(&lowered.as_str()) {
                continue;
            }
            entry.api.insert(symbol.name.clone());
        }

        for import in &data.imports {
            match resolve_import(path, import, &data.meta.language, &indexed) {
                Resolved::File(target) => {
                    let target_key = subsystem_of(&target, opts.depth);
                    if target_key != key {
                        entry.depends_on.insert(target_key);
                    }
                }
                Resolved::External => entry.external_imports += 1,
                Resolved::Ambiguous => entry.ambiguous_imports += 1,
            }
        }
    }

    // 3. Fan-in from the edges just collected.
    let mut dependents: BTreeMap<String, usize> = BTreeMap::new();
    for (key, data) in &acc {
        for target in &data.depends_on {
            if target != key {
                *dependents.entry(target.clone()).or_insert(0) += 1;
            }
        }
    }

    let total_ambiguous: usize = acc.values().map(|a| a.ambiguous_imports).sum();

    let mut rows: Vec<MapRow> = acc
        .into_iter()
        .map(|(subsystem, data)| {
            let class = if data.classes.len() == 1 {
                data.classes
                    .iter()
                    .next()
                    .map_or("production", |c| c.as_str())
                    .to_string()
            } else {
                "mixed".to_string()
            };
            MapRow {
                dependents: dependents.get(&subsystem).copied().unwrap_or(0),
                class,
                files: data.files,
                symbols: data.symbols,
                tests: data.tests,
                depends_on: bounded_list(data.depends_on.iter().map(String::as_str)),
                external_imports: data.external_imports,
                api: bounded_list(data.api.iter().map(String::as_str)),
                subsystem,
            }
        })
        .collect();

    // 4. Rank: most depended-upon first, then largest.  Fan-in is the useful
    // signal for "what is load-bearing here"; symbol count breaks ties.
    rows.sort_by(|a, b| {
        b.dependents
            .cmp(&a.dependents)
            .then(b.symbols.cmp(&a.symbols))
            .then(a.subsystem.cmp(&b.subsystem))
    });

    let mut notes = Vec::new();
    for (class, count) in &excluded {
        let flag = match *class {
            "vendor" => " (use --include-vendor)",
            "generated" => " (use --include-generated)",
            "test" => " (use --tests)",
            _ => "",
        };
        notes.push(format!("{count} files excluded as {class}{flag}"));
    }
    if excluded_by_glob > 0 {
        notes.push(format!("{excluded_by_glob} files excluded by --exclude"));
    }
    if total_ambiguous > 0 {
        notes.push(format!(
            "{total_ambiguous} imports matched several indexed files and were left unresolved"
        ));
    }

    MapReport {
        rows,
        notes,
        ranked_by: "dependents desc, then symbols desc, then name",
    }
}

/// Join up to [`ROW_LIST_LIMIT`] entries, marking elision explicitly.
fn bounded_list<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let all: Vec<&str> = items.collect();
    if all.len() <= ROW_LIST_LIMIT {
        return all.join(", ");
    }
    format!(
        "{}, ... (+{} more)",
        all[..ROW_LIST_LIMIT].join(", "),
        all.len() - ROW_LIST_LIMIT
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_recognizes_conventional_paths() {
        assert_eq!(classify(Path::new("src/main.rs")), PathClass::Production);
        assert_eq!(classify(Path::new("tests/foo.rs")), PathClass::Test);
        assert_eq!(classify(Path::new("src/app.test.ts")), PathClass::Test);
        assert_eq!(classify(Path::new("docs/design.md")), PathClass::Docs);
        assert_eq!(classify(Path::new("README.md")), PathClass::Docs);
        assert_eq!(
            classify(Path::new("vendor/thirdparty/blob.cpp")),
            PathClass::Vendor
        );
        assert_eq!(classify(Path::new("node_modules/x/index.js")), PathClass::Vendor);
        assert_eq!(classify(Path::new("generated/gen_api.cpp")), PathClass::Generated);
        assert_eq!(classify(Path::new("build/out.cpp")), PathClass::Generated);
        assert_eq!(classify(Path::new("src/api.gen.ts")), PathClass::Generated);
    }

    #[test]
    fn vendor_wins_over_test_classification() {
        // A vendored test is still vendored: that is what a vendor filter needs.
        assert_eq!(
            classify(Path::new("vendor/lib/tests/x_test.go")),
            PathClass::Vendor
        );
    }

    #[test]
    fn subsystem_grouping_respects_depth() {
        assert_eq!(subsystem_of(Path::new("src/a.rs"), 1), "src/");
        assert_eq!(subsystem_of(Path::new("src/util/a.rs"), 1), "src/");
        assert_eq!(subsystem_of(Path::new("src/util/a.rs"), 2), "src/util/");
        // Depth beyond the directory nesting clamps rather than naming the file.
        assert_eq!(subsystem_of(Path::new("src/a.rs"), 5), "src/");
        assert_eq!(subsystem_of(Path::new("README.md"), 1), "(root)");
    }

    #[test]
    fn bounded_list_marks_elision() {
        assert_eq!(bounded_list(["a", "b"].into_iter()), "a, b");
        let many = ["a", "b", "c", "d", "e", "f", "g"];
        assert_eq!(
            bounded_list(many.into_iter()),
            "a, b, c, d, e, ... (+2 more)"
        );
    }

    #[test]
    fn cpp_include_resolves_by_path_suffix() {
        let a = PathBuf::from("include/ange/ecs.hpp");
        let b = PathBuf::from("src/ecs.cpp");
        let indexed: BTreeSet<&PathBuf> = [&a, &b].into_iter().collect();

        assert_eq!(
            resolve_import(Path::new("src/ecs.cpp"), "ange/ecs.hpp", "cpp", &indexed),
            Resolved::File(a.clone())
        );
        // Not in the project: a system header.
        assert_eq!(
            resolve_import(Path::new("src/ecs.cpp"), "vector", "cpp", &indexed),
            Resolved::External
        );
    }

    #[test]
    fn ambiguous_cpp_include_produces_no_edge() {
        let a = PathBuf::from("a/include/util.h");
        let b = PathBuf::from("b/include/util.h");
        let indexed: BTreeSet<&PathBuf> = [&a, &b].into_iter().collect();
        assert_eq!(
            resolve_import(Path::new("src/x.cpp"), "include/util.h", "cpp", &indexed),
            Resolved::Ambiguous,
            "two matches must not be resolved to an arbitrary one"
        );
    }

    #[test]
    fn typescript_relative_import_resolves_against_the_importer() {
        let a = PathBuf::from("src/app.ts");
        let b = PathBuf::from("src/lib/helper.ts");
        let indexed: BTreeSet<&PathBuf> = [&a, &b].into_iter().collect();

        assert_eq!(
            resolve_import(Path::new("src/app.ts"), "./lib/helper", "typescript", &indexed),
            Resolved::File(b.clone())
        );
        assert_eq!(
            resolve_import(Path::new("src/lib/helper.ts"), "../app", "typescript", &indexed),
            Resolved::File(a.clone())
        );
        // Bare specifiers are packages.
        assert_eq!(
            resolve_import(Path::new("src/app.ts"), "react", "typescript", &indexed),
            Resolved::External
        );
    }

    #[test]
    fn rust_crate_paths_resolve_to_conventional_files() {
        let a = PathBuf::from("src/index.rs");
        let b = PathBuf::from("src/util/path.rs");
        let indexed: BTreeSet<&PathBuf> = [&a, &b].into_iter().collect();

        assert_eq!(
            resolve_import(Path::new("src/main.rs"), "crate::index::Symbol", "rust", &indexed),
            Resolved::File(a.clone())
        );
        assert_eq!(
            resolve_import(Path::new("src/main.rs"), "crate::util::path", "rust", &indexed),
            Resolved::File(b.clone())
        );
        // std and external crates are not project files.
        assert_eq!(
            resolve_import(Path::new("src/main.rs"), "std::collections::HashMap", "rust", &indexed),
            Resolved::External
        );
    }

    #[test]
    fn unmodelled_language_imports_are_external() {
        let a = PathBuf::from("main.py");
        let indexed: BTreeSet<&PathBuf> = [&a].into_iter().collect();
        assert_eq!(
            resolve_import(Path::new("main.py"), "os", "python", &indexed),
            Resolved::External
        );
    }
}
