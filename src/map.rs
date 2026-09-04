//! Bounded repository map (roadmap §8, §11 Phase 6).
//!
//! `cx overview` stays the cheapest entry point; `map` is the heavier
//! orientation command. It reports only provable facts — directory grouping,
//! file and symbol counts, test/vendor/generated classification, and
//! include/import edges that actually resolve to an indexed file. An import that
//! cannot be resolved is counted as external or ambiguous, never turned into a
//! guessed edge.

use std::collections::{BTreeMap, BTreeSet, HashMap};
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
    "add",
    "apply",
    "args",
    "begin",
    "build",
    "call",
    "clear",
    "clone",
    "close",
    "config",
    "context",
    "count",
    "ctx",
    "data",
    "default",
    "drop",
    "empty",
    "end",
    "eq",
    "error",
    "execute",
    "fmt",
    "from",
    "get",
    "handle",
    "hash",
    "id",
    "index",
    "init",
    "into",
    "invoke",
    "item",
    "key",
    "kind",
    "len",
    "list",
    "log",
    "main",
    "map",
    "name",
    "new",
    "next",
    "node",
    "ok",
    "open",
    "options",
    "params",
    "parse",
    "print",
    "process",
    "read",
    "remove",
    "reset",
    "result",
    "run",
    "set",
    "size",
    "start",
    "state",
    "status",
    "stop",
    "str",
    "string",
    "to",
    "to_string",
    "type",
    "update",
    "value",
    "write",
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
    "vendor",
    "vendored",
    "third_party",
    "thirdparty",
    "3rdparty",
    "node_modules",
    "external",
    "extern",
    "deps",
    "site-packages",
    ".venv",
    "venv",
    "bundle",
];

const GENERATED_DIRS: &[&str] = &[
    "generated",
    "gen",
    "autogen",
    "build",
    "dist",
    "out",
    "target",
    "__pycache__",
    ".next",
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
    if components
        .iter()
        .any(|c| GENERATED_DIRS.contains(&c.as_str()))
    {
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
pub(crate) enum Resolved {
    /// Exactly one indexed file matches.
    File(PathBuf),
    /// Nothing in this project matches: a system or third-party dependency.
    External,
    /// Several indexed files match, so no edge is claimed.
    Ambiguous,
}

/// Pre-built path lookup that turns import resolution into a hash lookup.
///
/// Built once per command from the exact path set the caller wants to resolve
/// against.  The previous implementation scanned every indexed path for every
/// import and allocated two `String`s per (import, path) pair, which on the
/// fixed ANGE corpus meant 6,229 imports x 3,905 files and a 3.4 s `map`.
///
/// The three outcomes are unchanged: an include that matches nothing is
/// `External`, one that matches exactly one file is `File`, and one that matches
/// several is `Ambiguous` with no target chosen.
pub(crate) struct ImportIndex {
    /// Every component-boundary suffix of every path, mapped to the files ending
    /// with it.  A suffix shared by several files is exactly what makes an
    /// include ambiguous, so the shared entry *is* the ambiguity evidence.
    by_suffix: HashMap<String, Vec<PathBuf>>,
    /// Normalized full path -> file, for languages that resolve by constructing
    /// a complete candidate path and taking the first that exists.
    by_path: HashMap<String, PathBuf>,
}

impl ImportIndex {
    /// Index the given paths.  Duplicate paths are collapsed, so a repeated
    /// entry cannot fabricate an ambiguity.
    pub(crate) fn build<'a, I>(paths: I) -> Self
    where
        I: IntoIterator<Item = &'a PathBuf>,
    {
        let mut by_suffix: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let mut by_path: HashMap<String, PathBuf> = HashMap::new();

        for path in paths {
            let normalized = path.to_string_lossy().replace('\\', "/");
            by_path.insert(normalized.clone(), path.clone());

            // The full path, then each suffix beginning after a '/'.  Together
            // these are exactly the matches the old `text == import ||
            // text.ends_with("/" + import)` test accepted.
            let mut push = |key: String| {
                let bucket = by_suffix.entry(key).or_default();
                if !bucket.contains(path) {
                    bucket.push(path.clone());
                }
            };
            push(normalized.clone());
            for (offset, _) in normalized.match_indices('/') {
                push(normalized[offset + 1..].to_string());
            }
        }

        Self { by_suffix, by_path }
    }

    /// Resolve an import target to an indexed file, by path only.
    ///
    /// Deliberately syntactic: C/C++ include paths and TypeScript relative
    /// specifiers are written paths, so matching them against indexed paths is a
    /// fact. Rust module paths are matched against the conventional file layout
    /// and are reported as external when that layout does not apply.
    pub(crate) fn resolve(&self, importer: &Path, import: &str, language: &str) -> Resolved {
        let candidates: Vec<String> = match language {
            "c" | "cpp" => {
                // One lookup replaces the whole-corpus scan.  `#include
                // "ange/ecs.hpp"` may live under any include root, which is why
                // the key is a component-boundary suffix rather than a full path.
                return match self.by_suffix.get(import) {
                    None => Resolved::External,
                    Some(files) if files.len() == 1 => Resolved::File(files[0].clone()),
                    Some(_) => Resolved::Ambiguous,
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

        // Candidate order is significant and unchanged: the first candidate that
        // exists wins.
        for candidate in candidates {
            if let Some(found) = self.by_path.get(&candidate) {
                return Resolved::File(found.clone());
            }
        }
        Resolved::External
    }
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

    // Built once for the whole map: the filtered file set is exactly what an
    // import may resolve to, so an include pointing at an excluded file stays
    // External, as before.
    let imports = ImportIndex::build(included.iter().map(|(p, _, _)| *p));

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
            match imports.resolve(path, import, &data.meta.language) {
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
        assert_eq!(
            classify(Path::new("node_modules/x/index.js")),
            PathClass::Vendor
        );
        assert_eq!(
            classify(Path::new("generated/gen_api.cpp")),
            PathClass::Generated
        );
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
        let idx = ImportIndex::build([&a, &b]);

        assert_eq!(
            idx.resolve(Path::new("src/ecs.cpp"), "ange/ecs.hpp", "cpp"),
            Resolved::File(a.clone())
        );
        // Not in the project: a system header.
        assert_eq!(
            idx.resolve(Path::new("src/ecs.cpp"), "vector", "cpp"),
            Resolved::External
        );
    }

    #[test]
    fn cpp_include_matches_only_on_component_boundaries() {
        // The suffix lookup must not accept a partial final component: `cs.hpp`
        // is a substring of `ecs.hpp` but not a path suffix of it.
        let a = PathBuf::from("include/ange/ecs.hpp");
        let idx = ImportIndex::build([&a]);

        assert_eq!(
            idx.resolve(Path::new("src/x.cpp"), "cs.hpp", "cpp"),
            Resolved::External
        );
        assert_eq!(
            idx.resolve(Path::new("src/x.cpp"), "ge/ecs.hpp", "cpp"),
            Resolved::External
        );
        // Every genuine component-boundary suffix does resolve.
        for import in ["ecs.hpp", "ange/ecs.hpp", "include/ange/ecs.hpp"] {
            assert_eq!(
                idx.resolve(Path::new("src/x.cpp"), import, "cpp"),
                Resolved::File(a.clone()),
                "{import}"
            );
        }
    }

    #[test]
    fn ambiguous_cpp_include_produces_no_edge() {
        let a = PathBuf::from("a/include/util.h");
        let b = PathBuf::from("b/include/util.h");
        let idx = ImportIndex::build([&a, &b]);
        assert_eq!(
            idx.resolve(Path::new("src/x.cpp"), "include/util.h", "cpp"),
            Resolved::Ambiguous,
            "two matches must not be resolved to an arbitrary one"
        );
        // A longer, unambiguous spelling still resolves.
        assert_eq!(
            idx.resolve(Path::new("src/x.cpp"), "a/include/util.h", "cpp"),
            Resolved::File(a.clone())
        );
    }

    #[test]
    fn duplicate_paths_cannot_fabricate_ambiguity() {
        let a = PathBuf::from("include/util.h");
        let idx = ImportIndex::build([&a, &a]);
        assert_eq!(
            idx.resolve(Path::new("src/x.cpp"), "util.h", "cpp"),
            Resolved::File(a.clone()),
            "one file listed twice is still one file"
        );
    }

    #[test]
    fn typescript_relative_import_resolves_against_the_importer() {
        let a = PathBuf::from("src/app.ts");
        let b = PathBuf::from("src/lib/helper.ts");
        let idx = ImportIndex::build([&a, &b]);

        assert_eq!(
            idx.resolve(Path::new("src/app.ts"), "./lib/helper", "typescript"),
            Resolved::File(b.clone())
        );
        assert_eq!(
            idx.resolve(Path::new("src/lib/helper.ts"), "../app", "typescript"),
            Resolved::File(a.clone())
        );
        // Bare specifiers are packages.
        assert_eq!(
            idx.resolve(Path::new("src/app.ts"), "react", "typescript"),
            Resolved::External
        );
    }

    #[test]
    fn rust_crate_paths_resolve_to_conventional_files() {
        let a = PathBuf::from("src/index.rs");
        let b = PathBuf::from("src/util/path.rs");
        let idx = ImportIndex::build([&a, &b]);

        assert_eq!(
            idx.resolve(Path::new("src/main.rs"), "crate::index::Symbol", "rust"),
            Resolved::File(a.clone())
        );
        assert_eq!(
            idx.resolve(Path::new("src/main.rs"), "crate::util::path", "rust"),
            Resolved::File(b.clone())
        );
        // std and external crates are not project files.
        assert_eq!(
            idx.resolve(
                Path::new("src/main.rs"),
                "std::collections::HashMap",
                "rust"
            ),
            Resolved::External
        );
    }

    #[test]
    fn unmodelled_language_imports_are_external() {
        let a = PathBuf::from("main.py");
        let idx = ImportIndex::build([&a]);
        assert_eq!(
            idx.resolve(Path::new("main.py"), "os", "python"),
            Resolved::External
        );
    }

    /// Regression guard for the O(imports x files) scan this lookup replaced.
    ///
    /// The old implementation compared every import against every indexed path
    /// and allocated two `String`s per pair.  At the scale below that is
    /// 6,000 x 4,000 = 24M comparisons with ~48M allocations, which takes tens of
    /// seconds in a debug build; the lookup does 4,000 inserts plus 6,000 hash
    /// probes and finishes in milliseconds.  The bound is deliberately loose so
    /// it cannot flake on a slow machine, while still being orders of magnitude
    /// below a reintroduced full scan.
    #[test]
    fn include_resolution_does_not_scale_with_corpus_size() {
        use std::time::Instant;

        let paths: Vec<PathBuf> = (0..4_000)
            .map(|i| PathBuf::from(format!("src/mod{}/unit{i}.h", i % 40)))
            .collect();

        let build_start = Instant::now();
        let idx = ImportIndex::build(paths.iter());
        let build_elapsed = build_start.elapsed();

        let resolve_start = Instant::now();
        let mut resolved = 0usize;
        let mut external = 0usize;
        for i in 0..6_000 {
            // Half the imports hit a real file, half miss entirely.
            let import = if i % 2 == 0 {
                format!("unit{}.h", i % 4_000)
            } else {
                format!("absent/header{i}.h")
            };
            match idx.resolve(Path::new("src/caller.cpp"), &import, "cpp") {
                Resolved::File(_) => resolved += 1,
                Resolved::External => external += 1,
                Resolved::Ambiguous => panic!("unique file names must not be ambiguous"),
            }
        }
        let resolve_elapsed = resolve_start.elapsed();

        // Correctness of the workload itself, so a no-op cannot pass the timing.
        assert_eq!(resolved, 3_000, "half the imports must resolve");
        assert_eq!(external, 3_000, "half the imports must miss");

        let total = build_elapsed + resolve_elapsed;
        assert!(
            total.as_secs_f64() < 5.0,
            "include resolution regressed to corpus-wide scanning: build={build_elapsed:?} resolve={resolve_elapsed:?}"
        );
    }
}
