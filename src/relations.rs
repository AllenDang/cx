//! Direct caller/callee relations with explicit evidence and resolution levels
//! (roadmap §5.4, §9, §11 Phase 7).
//!
//! Only one hop. No multi-hop impact graph, because §9 puts that last and only
//! after identity and evidence are stable.
//!
//! The central rule: an edge states how it was established. Tree-sitter can show
//! that an identifier sits in a call position — that is `syntax`. Narrowing which
//! definition it refers to requires lexical-scope or import facts, and when
//! neither narrows it to one candidate the edge keeps `to` empty and lists the
//! candidates instead of picking one.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::index::{Index, Symbol, SymbolRole};
use crate::map::{Resolved, resolve_import};

/// What kind of fact an edge rests on (roadmap §5.4).
///
/// `Text` is part of the vocabulary but never produced: every cx reference is
/// filtered through the AST, so a plain text match is not something cx reports.
/// It stays here so a consumer reading the contract sees the full ladder and so a
/// future grep fallback has a truthful label available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Definition,
    Declaration,
    Call,
    TypeReference,
    IdentifierReference,
    Import,
    #[allow(dead_code, reason = "contract vocabulary; cx never matches text-only")]
    Text,
}

impl EvidenceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Declaration => "declaration",
            Self::Call => "call",
            Self::TypeReference => "type_reference",
            Self::IdentifierReference => "identifier_reference",
            Self::Import => "import",
            Self::Text => "text",
        }
    }
}

/// How strongly an edge's target was established (roadmap §5.4).
///
/// Ordered weakest to strongest. Output must name the highest level actually
/// reached and never more: a name-only match is `syntax`, not `lexical_scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionLevel {
    /// Matched as text only.  Never produced: cx filters through the AST.
    #[allow(dead_code, reason = "contract vocabulary; cx never matches text-only")]
    Text,
    /// The AST places this identifier in a callee position.
    Syntax,
    /// Lexical scope narrowed the candidates to one: either a qualifier written
    /// at the call site matched exactly one candidate, or exactly one candidate
    /// is visible from the call site by lexical nesting.
    LexicalScope,
    /// The calling file imports the file defining exactly one candidate.
    ImportResolved,
    /// Types were resolved.  cx never claims this — the variant exists so the
    /// ladder is complete and so nothing has to invent a label for a level cx
    /// does not reach.
    #[allow(dead_code, reason = "contract vocabulary; cx performs no type resolution")]
    TypeResolved,
}

impl ResolutionLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Syntax => "syntax",
            Self::LexicalScope => "lexical_scope",
            Self::ImportResolved => "import_resolved",
            Self::TypeResolved => "type_resolved",
        }
    }
}

/// One direct relation, in output form.
#[derive(Serialize)]
pub struct EdgeRow {
    /// Qualified name of the calling symbol, or its bare name when the scope is
    /// unresolved.
    pub from: String,
    /// Qualified name of the target, empty when it could not be narrowed to one.
    pub to: String,
    pub evidence: String,
    pub resolution: String,
    pub file: String,
    pub line: usize,
    /// Candidates left when `to` is empty, so nothing is silently discarded.
    pub ambiguous_candidates: String,
}

/// A definition that a call might refer to.
struct Candidate<'a> {
    path: &'a PathBuf,
    symbol: &'a Symbol,
    language: &'a str,
}

impl Candidate<'_> {
    fn label(&self) -> String {
        self.symbol
            .qualified_name
            .clone()
            .unwrap_or_else(|| self.symbol.name.clone())
    }
}

/// Collect the definitions a name could refer to.
///
/// Declarations are included only when no definition exists, so a C++ prototype
/// never competes with its own implementation.
fn candidates_for<'a>(index: &'a Index, name: &str) -> Vec<Candidate<'a>> {
    let mut definitions = Vec::new();
    let mut declarations = Vec::new();
    for (path, data) in &index.entries {
        for symbol in &data.symbols {
            if symbol.name != name {
                continue;
            }
            let candidate = Candidate {
                path,
                symbol,
                language: data.meta.language.as_str(),
            };
            match symbol.role {
                SymbolRole::Definition => definitions.push(candidate),
                SymbolRole::Declaration => declarations.push(candidate),
                _ => {}
            }
        }
    }
    if definitions.is_empty() { declarations } else { definitions }
}

/// Distinct logical targets among candidates, by qualified label.
fn distinct_labels(candidates: &[Candidate<'_>]) -> Vec<String> {
    let mut labels: Vec<String> = candidates.iter().map(Candidate::label).collect();
    labels.sort();
    labels.dedup();
    labels
}

/// Outcome of trying to point one call site at one definition.
struct Resolution {
    to: Option<String>,
    level: ResolutionLevel,
    ambiguous: Vec<String>,
}

/// Narrow a call site to a single target, recording how far it got.
///
/// Candidates are always restricted to the caller's language first: a C++ call
/// cannot refer to a TypeScript method, and allowing that is precisely the
/// cross-scope false edge §11 forbids.
fn resolve_call(
    index: &Index,
    call_file: &Path,
    call_language: &str,
    caller_scope: &[String],
    qualifier: Option<&str>,
    candidates: &[Candidate<'_>],
) -> Resolution {
    let same_language: Vec<&Candidate<'_>> = candidates
        .iter()
        .filter(|c| c.language == call_language)
        .collect();

    if same_language.is_empty() {
        return Resolution {
            to: None,
            level: ResolutionLevel::Syntax,
            ambiguous: Vec::new(),
        };
    }

    let labels_of = |set: &[&Candidate<'_>]| -> Vec<String> {
        let mut labels: Vec<String> = set.iter().map(|c| c.label()).collect();
        labels.sort();
        labels.dedup();
        labels
    };

    // A qualifier written at the call site is the strongest lexical evidence
    // available: `alpha::run()` names its scope explicitly.
    if let Some(qualifier) = qualifier {
        let matching: Vec<&Candidate<'_>> = same_language
            .iter()
            .filter(|c| {
                c.symbol.qualified_name.as_deref().is_some_and(|q| {
                    q.contains(&format!("{qualifier}::")) || q.contains(&format!("{qualifier}."))
                })
            })
            .copied()
            .collect();
        let labels = labels_of(&matching);
        if labels.len() == 1 {
            return Resolution {
                to: Some(labels[0].clone()),
                level: ResolutionLevel::LexicalScope,
                ambiguous: Vec::new(),
            };
        }
    }

    // Import evidence: the calling file resolves an import to the file defining
    // exactly one candidate.
    let indexed: BTreeSet<&PathBuf> = index.entries.keys().collect();
    if let Some(data) = index.entries.get(call_file) {
        let mut imported_files: BTreeSet<PathBuf> = BTreeSet::new();
        for import in &data.imports {
            if let Resolved::File(target) =
                resolve_import(call_file, import, &data.meta.language, &indexed)
            {
                imported_files.insert(target);
            }
        }
        let via_import: Vec<&Candidate<'_>> = same_language
            .iter()
            .filter(|c| imported_files.contains(c.path))
            .copied()
            .collect();
        let labels = labels_of(&via_import);
        if labels.len() == 1 {
            return Resolution {
                to: Some(labels[0].clone()),
                level: ResolutionLevel::ImportResolved,
                ambiguous: Vec::new(),
            };
        }
    }

    // Lexical nesting: a candidate is visible when its scope encloses the call.
    let visible: Vec<&Candidate<'_>> = same_language
        .iter()
        .filter(|c| {
            let scope = &c.symbol.scope_path;
            scope.len() <= caller_scope.len() && caller_scope.starts_with(scope.as_slice())
        })
        .copied()
        .collect();
    let visible_labels = labels_of(&visible);
    if visible_labels.len() == 1 {
        return Resolution {
            to: Some(visible_labels[0].clone()),
            level: ResolutionLevel::LexicalScope,
            ambiguous: Vec::new(),
        };
    }

    // Nothing narrowed it. A single project-wide candidate is still a fact worth
    // reporting, but only as syntax evidence: uniqueness is not scope reasoning.
    let all_labels = labels_of(&same_language);
    if all_labels.len() == 1 {
        return Resolution {
            to: Some(all_labels[0].clone()),
            level: ResolutionLevel::Syntax,
            ambiguous: Vec::new(),
        };
    }

    Resolution {
        to: None,
        level: ResolutionLevel::Syntax,
        ambiguous: all_labels,
    }
}

/// Symbol enclosing a byte offset: the tightest range wins.
fn enclosing_symbol(symbols: &[Symbol], offset: usize) -> Option<&Symbol> {
    symbols
        .iter()
        .filter(|s| s.byte_range.0 <= offset && offset < s.byte_range.1)
        .min_by_key(|s| s.byte_range.1 - s.byte_range.0)
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn label_of(symbol: &Symbol) -> String {
    symbol
        .qualified_name
        .clone()
        .unwrap_or_else(|| symbol.name.clone())
}

/// Result of a relation query.
pub struct RelationReport {
    pub rows: Vec<EdgeRow>,
    pub warnings: Vec<String>,
}

/// Direct callers of `name`: one hop, syntax-level call evidence upward.
pub fn callers(index: &Index, name: &str, scope_glob: Option<&str>) -> RelationReport {
    let candidates = candidates_for(index, name);
    let target_labels = distinct_labels(&candidates);
    let mut rows = Vec::new();
    let mut warnings = Vec::new();

    let mut files: Vec<(&PathBuf, &crate::index::FileData)> = index.entries.iter().collect();
    files.sort_by_key(|(path, _)| *path);

    for (path, data) in files {
        let abs = index.root.join(path);
        let Ok(source) = fs::read(&abs) else { continue };
        if memchr::memmem::find(&source, name.as_bytes()).is_none() {
            continue;
        }
        let Ok(sites) = crate::language::find_calls(&data.meta.language, &source, &abs) else {
            continue;
        };

        for site in sites.iter().filter(|s| s.name == name) {
            let caller = enclosing_symbol(&data.symbols, site.byte_offset);
            let caller_scope: Vec<String> = caller
                .map(|c| {
                    // A call inside `fn f` in `mod a` is lexically inside `a`,
                    // and inside `f` itself for nested items.
                    let mut scope = c.scope_path.clone();
                    scope.push(c.name.clone());
                    scope
                })
                .unwrap_or_default();

            let resolution = resolve_call(
                index,
                path,
                &data.meta.language,
                &caller_scope,
                site.qualifier.as_deref(),
                &candidates,
            );

            if let Some(pattern) = scope_glob {
                let matches_target = resolution
                    .to
                    .as_deref()
                    .is_some_and(|t| crate::util::glob::glob_match(pattern, t));
                if !matches_target {
                    continue;
                }
            }

            rows.push(EdgeRow {
                from: caller.map(label_of).unwrap_or_else(|| "(file scope)".to_string()),
                to: resolution.to.clone().unwrap_or_default(),
                evidence: EvidenceKind::Call.as_str().to_string(),
                resolution: resolution.level.as_str().to_string(),
                file: display_path(path),
                line: site.line,
                ambiguous_candidates: resolution.ambiguous.join(", "),
            });
        }
    }

    if target_labels.len() > 1 {
        warnings.push(format!(
            "{} distinct symbols named \"{name}\": {}. Edges with an empty target could not be narrowed to one.",
            target_labels.len(),
            target_labels.join(", ")
        ));
    }
    let unresolved = rows.iter().filter(|r| r.to.is_empty()).count();
    if unresolved > 0 {
        warnings.push(format!(
            "{unresolved} of {} call sites are syntax evidence only; cx does not resolve types",
            rows.len()
        ));
    }

    RelationReport { rows, warnings }
}

/// Direct callees of `name`: calls written inside its body, one hop.
pub fn callees(index: &Index, name: &str, scope_glob: Option<&str>) -> RelationReport {
    let mut warnings = Vec::new();

    // Pick the definition whose body to read.
    let mut hosts: Vec<&Candidate<'_>> = Vec::new();
    let candidates = candidates_for(index, name);
    let filtered: Vec<&Candidate<'_>> = candidates
        .iter()
        .filter(|c| {
            scope_glob.is_none_or(|pattern| {
                c.symbol
                    .qualified_name
                    .as_deref()
                    .is_some_and(|q| crate::util::glob::glob_match(pattern, q))
            })
        })
        .collect();
    hosts.extend(filtered);

    let labels: Vec<String> = {
        let mut labels: Vec<String> = hosts.iter().map(|c| c.label()).collect();
        labels.sort();
        labels.dedup();
        labels
    };

    if labels.len() > 1 {
        // Reading one arbitrary body would silently answer a different question.
        warnings.push(format!(
            "{} distinct symbols named \"{name}\": {}. Narrow with --scope to choose one.",
            labels.len(),
            labels.join(", ")
        ));
        return RelationReport { rows: Vec::new(), warnings };
    }

    let mut rows = Vec::new();
    let mut all_candidates_cache: HashMap<String, Vec<Candidate<'_>>> = HashMap::new();

    for host in &hosts {
        let abs = index.root.join(host.path);
        let Ok(source) = fs::read(&abs) else { continue };
        let Some(data) = index.entries.get(host.path) else { continue };
        let Ok(sites) = crate::language::find_calls(&data.meta.language, &source, &abs) else {
            continue;
        };

        let (start, end) = host.symbol.byte_range;
        let mut host_scope = host.symbol.scope_path.clone();
        host_scope.push(host.symbol.name.clone());

        for site in sites
            .iter()
            .filter(|s| s.byte_offset >= start && s.byte_offset < end)
        {
            let callee_candidates = all_candidates_cache
                .entry(site.name.clone())
                .or_insert_with(|| candidates_for(index, &site.name));

            let resolution = resolve_call(
                index,
                host.path,
                &data.meta.language,
                &host_scope,
                site.qualifier.as_deref(),
                callee_candidates,
            );

            rows.push(EdgeRow {
                from: label_of(host.symbol),
                to: resolution.to.clone().unwrap_or_default(),
                evidence: EvidenceKind::Call.as_str().to_string(),
                resolution: resolution.level.as_str().to_string(),
                file: display_path(host.path),
                line: site.line,
                ambiguous_candidates: resolution.ambiguous.join(", "),
            });
        }
    }

    rows.sort_by(|a, b| a.line.cmp(&b.line).then(a.to.cmp(&b.to)));
    rows.dedup_by(|a, b| a.line == b.line && a.to == b.to && a.ambiguous_candidates == b.ambiguous_candidates);

    let unresolved = rows.iter().filter(|r| r.to.is_empty()).count();
    if unresolved > 0 {
        warnings.push(format!(
            "{unresolved} of {} calls could not be narrowed to one definition",
            rows.len()
        ));
    }

    RelationReport { rows, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_levels_are_ordered_weakest_first() {
        assert!(ResolutionLevel::Text < ResolutionLevel::Syntax);
        assert!(ResolutionLevel::Syntax < ResolutionLevel::LexicalScope);
        assert!(ResolutionLevel::LexicalScope < ResolutionLevel::ImportResolved);
        assert!(ResolutionLevel::ImportResolved < ResolutionLevel::TypeResolved);
    }

    #[test]
    fn evidence_and_resolution_serialize_as_snake_case() {
        assert_eq!(EvidenceKind::TypeReference.as_str(), "type_reference");
        assert_eq!(EvidenceKind::Call.as_str(), "call");
        assert_eq!(ResolutionLevel::ImportResolved.as_str(), "import_resolved");
        assert_eq!(
            serde_json::to_string(&ResolutionLevel::LexicalScope).unwrap(),
            "\"lexical_scope\""
        );
    }

    #[test]
    fn enclosing_symbol_prefers_the_tightest_range() {
        let outer = Symbol {
            name: "outer".into(),
            kind: crate::index::SymbolKind::Fn,
            role: SymbolRole::Definition,
            scope_path: Vec::new(),
            qualified_name: Some("outer".into()),
            signature: "fn outer()".into(),
            byte_range: (0, 100),
            is_test: false,
        };
        let inner = Symbol {
            name: "inner".into(),
            byte_range: (10, 20),
            qualified_name: Some("outer::inner".into()),
            ..outer.clone()
        };
        let symbols = vec![outer, inner];
        assert_eq!(enclosing_symbol(&symbols, 15).unwrap().name, "inner");
        assert_eq!(enclosing_symbol(&symbols, 50).unwrap().name, "outer");
        assert!(enclosing_symbol(&symbols, 500).is_none());
    }
}
