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

use serde::Serialize;

use crate::index::{Index, Symbol, SymbolRole};
use crate::language::CallSite;
use crate::relation_index::{RelationIndex, candidate_labels, display_path, label_of};

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
    #[allow(
        dead_code,
        reason = "contract vocabulary; cx performs no type resolution"
    )]
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

/// Result of a one-hop projection. Coverage travels through the existing
/// warnings array so the JSON v1 envelope and legacy edge fields stay intact.
pub struct RelationReport {
    pub rows: Vec<EdgeRow>,
    pub warnings: Vec<String>,
}

/// Calls belong to the nearest execution container, not every symbol whose
/// byte range contains them. Anonymous closures have no named function owner.
pub(crate) fn owner<'a>(symbols: &'a [Symbol], site: &CallSite) -> Option<&'a Symbol> {
    if site.anonymous_owner {
        return None;
    }
    let symbol = enclosing_symbol(symbols, site.byte_offset)?;
    let (start, end) = site.owner_range?;
    (symbol.kind == crate::index::SymbolKind::Fn
        && symbol.byte_range.0 <= start
        && symbol.byte_range.1 >= end)
        .then_some(symbol)
}

fn project_edge(
    analysis: &RelationIndex<'_>,
    index: &Index,
    path: &std::path::Path,
    site: &CallSite,
    scope: Option<&str>,
) -> Option<EdgeRow> {
    let data = &index.entries[path];
    let caller = owner(&data.symbols, site);
    let resolution = analysis.resolve(path, &data.meta.language, caller, site);
    if let Some(pattern) = scope {
        let matches = |candidate: &crate::relation_index::Candidate<'_>| {
            candidate
                .symbol
                .qualified_name
                .as_deref()
                .is_some_and(|q| crate::util::glob::glob_match(pattern, q))
        };
        if !resolution.to.is_some_and(matches) && !resolution.candidates.iter().any(|c| matches(c))
        {
            return None;
        }
    }
    Some(EdgeRow {
        from: caller.map(label_of).unwrap_or_else(|| {
            site.owner_range.map_or_else(
                || "(file scope)".to_string(),
                |(start, _)| format!("(anonymous scope@{start})"),
            )
        }),
        to: resolution.to.map_or_else(String::new, |c| c.label()),
        evidence: EvidenceKind::Call.as_str().to_string(),
        resolution: resolution.level.as_str().to_string(),
        file: display_path(path),
        line: site.line,
        ambiguous_candidates: candidate_labels(&resolution.candidates).join(", "),
    })
}

/// Direct callers, using the shared command-local snapshot and candidate table.
pub fn callers(index: &Index, name: &str, scope_glob: Option<&str>) -> RelationReport {
    let analysis = RelationIndex::for_callers(index, name);
    let mut warnings = vec![analysis.warning()];
    let targets: Vec<_> = analysis.named(name).iter().collect();
    if targets.len() > 1 {
        warnings.push(format!(
            "{} distinct symbols named \"{name}\": {}. Edges with an empty target could not be narrowed to one.",
            targets.len(), candidate_labels(&targets).join(", ")));
    }
    let mut rows = Vec::new();
    for (path, sites) in &analysis.calls {
        for site in sites.iter().filter(|site| site.name == name) {
            if let Some(row) = project_edge(&analysis, index, path, site, scope_glob) {
                rows.push(row);
            }
        }
    }
    let uncertain = rows.iter().filter(|r| r.to.is_empty()).count();
    if scope_glob.is_some() && uncertain > 0 {
        warnings.push(format!("relation_scope: {uncertain} unresolved call sites retained by matching candidates; targets remain unknown and candidate sets are not narrowed"));
    }
    let syntax = rows.iter().filter(|r| r.resolution == "syntax").count();
    if syntax > 0 {
        warnings.push(format!(
            "{syntax} of {} call sites are syntax evidence only; cx does not resolve types",
            rows.len()
        ));
    }
    RelationReport { rows, warnings }
}

/// Direct callees. Distinct definition sites (including identical display names)
/// must not be silently unioned into a single subject.
pub fn callees(index: &Index, name: &str, scope_glob: Option<&str>) -> RelationReport {
    let analysis = RelationIndex::for_callees(index, name, scope_glob);
    let mut warnings = vec![analysis.warning()];
    let matching: Vec<_> = analysis
        .named(name)
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
    // Reading a definition body is not proof that an unmatched declaration is
    // the same entity. Keep all sites for target resolution, but select bodies
    // by definition role, just as the definition command does.
    let definitions: Vec<_> = matching
        .iter()
        .copied()
        .filter(|c| c.symbol.role == SymbolRole::Definition)
        .collect();
    let declarations = matching
        .iter()
        .filter(|c| c.symbol.role == SymbolRole::Declaration)
        .count();
    if !definitions.is_empty() && declarations > 0 {
        warnings.push(format!("relation_subject: {declarations} declaration sites not merged; selecting definition bodies only, without claiming declaration equivalence"));
    }
    let hosts = if definitions.is_empty() {
        matching
    } else {
        definitions
    };
    if hosts.len() > 1 {
        warnings.push(format!(
            "{} distinct symbols named \"{name}\": {}. Narrow with --scope to choose one. Equal qualified names at different sites cannot be selected by this legacy interface.",
            hosts.len(), candidate_labels(&hosts).join(", ")));
        return RelationReport {
            rows: Vec::new(),
            warnings,
        };
    }
    let mut rows = Vec::new();
    if let Some(host) = hosts.first() {
        if host.symbol.role == SymbolRole::Declaration {
            warnings.push("relation_subject: declaration_only; no function body analyzed".into());
        } else if let Some(sites) = analysis.calls.get(&host.id.file) {
            let data = &index.entries[&host.id.file];
            for site in sites {
                if owner(&data.symbols, site).is_some_and(|s| s.byte_range == host.id.range)
                    && let Some(row) = project_edge(&analysis, index, &host.id.file, site, None)
                {
                    rows.push(row);
                }
            }
        }
    } else {
        warnings.push("relation_subject: not_found".into());
    }
    let unresolved = rows.iter().filter(|r| r.to.is_empty()).count();
    if unresolved > 0 {
        warnings.push(format!(
            "{unresolved} of {} calls could not be narrowed to one definition",
            rows.len()
        ));
    }
    RelationReport { rows, warnings }
}

/// Symbol enclosing a byte offset: the tightest range wins.
fn enclosing_symbol(symbols: &[Symbol], offset: usize) -> Option<&Symbol> {
    symbols
        .iter()
        .filter(|s| s.byte_range.0 <= offset && offset < s.byte_range.1)
        .min_by_key(|s| s.byte_range.1 - s.byte_range.0)
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
