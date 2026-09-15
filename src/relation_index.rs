//! Command-local relation facts. No persistent graph, display-name keys, or
//! resolved-edge cache. Symbols/imports and call sites share a content identity.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::index::{Index, Symbol, SymbolKind, SymbolRole, content_hash};
use crate::language::{CallSite, LangError};
use crate::map::{ImportIndex, Resolved};
use crate::relations::ResolutionLevel;

/// Exact site within this index content version, not a cross-version entity ID.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct DefinitionSiteId {
    pub file: PathBuf,
    pub language: String,
    pub range: (usize, usize),
}

pub struct Candidate<'a> {
    pub id: DefinitionSiteId,
    pub symbol: &'a Symbol,
    lexical_parent: Option<(usize, usize)>,
    signature: Option<String>,
    /// Declaration sites linked only by an exact signature and file/include fact.
    pub declarations: Vec<DefinitionSiteId>,
}
impl Candidate<'_> {
    pub fn label(&self) -> String {
        label_of(self.symbol)
    }
    fn site_label(&self) -> String {
        format!(
            "{} [{}:{}..{}; {}]",
            self.label(),
            display_path(&self.id.file),
            self.id.range.0,
            self.id.range.1,
            self.id.language
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageReason {
    ContentChanged,
    ReadFailed,
    MissingGrammar,
    ParseError,
    UnsupportedCallForm,
    UnsupportedLanguage,
}
#[derive(Clone, Serialize)]
pub struct CoverageIssue {
    pub file: String,
    pub reason: CoverageReason,
}
#[derive(Clone, Serialize)]
pub struct Coverage {
    pub model: &'static str,
    pub scope: &'static str,
    pub generation: u64,
    pub snapshot_id: String,
    pub files_checked: usize,
    pub files_analyzed: usize,
    pub files_skipped_missing_grammar: usize,
    pub complete_within_model: bool,
    pub issues: Vec<CoverageIssue>,
    pub issues_total: usize,
    pub issues_omitted: usize,
    pub issue_counts: BTreeMap<CoverageReason, usize>,
    pub limitations: &'static str,
}

impl Coverage {
    pub fn record(&mut self, file: &Path, reason: CoverageReason) {
        self.issues_total += 1;
        *self.issue_counts.entry(reason).or_default() += 1;
        self.issues.push(CoverageIssue {
            file: display_path(file),
            reason,
        });
        self.issues
            .sort_by(|a, b| a.reason.cmp(&b.reason).then(a.file.cmp(&b.file)));
        self.issues.truncate(16);
        self.issues_omitted = self.issues_total.saturating_sub(self.issues.len());
        self.complete_within_model = false;
    }
}

/// Shared immutable inputs and name table, built once per relation command.
/// A failed source check disables target narrowing: dropping a candidate must
/// not make another candidate spuriously unique.
pub struct RelationIndex<'a> {
    pub candidates: BTreeMap<String, Vec<Candidate<'a>>>,
    pub calls: BTreeMap<PathBuf, Vec<CallSite>>,
    pub coverage: Coverage,
    imports: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    trustworthy_candidates: bool,
}

impl<'a> RelationIndex<'a> {
    pub fn from_cached(index: &'a Index) -> Self {
        Self::build_selected(
            index,
            |_| Err(std::io::Error::from(std::io::ErrorKind::Other)),
            |_, _| false,
            false,
        )
    }
    pub fn for_callers(index: &'a Index, name: &str) -> Self {
        let mut analysis = Self::build_selected(
            index,
            |path| std::fs::read(path),
            |_, source| memchr::memmem::find(source, name.as_bytes()).is_some(),
            true,
        );
        analysis.coverage.scope = "indexed_candidates; callers_matching_name";
        analysis
    }

    pub fn for_callees(index: &'a Index, name: &str, scope: Option<&str>) -> Self {
        let mut analysis = Self::build_selected(
            index,
            |path| std::fs::read(path),
            |data, _| {
                data.symbols.iter().any(|s| {
                    s.name == name
                        && s.kind == SymbolKind::Fn
                        && s.role == SymbolRole::Definition
                        && scope.is_none_or(|pattern| {
                            s.qualified_name
                                .as_deref()
                                .is_some_and(|q| crate::util::glob::glob_match(pattern, q))
                        })
                })
            },
            true,
        );
        analysis.coverage.scope = "indexed_candidates; callees_selected_bodies";
        analysis
    }

    #[cfg(test)]
    pub fn build(index: &'a Index) -> Self {
        Self::build_with_reader(index, |path| std::fs::read(path))
    }

    // Injectable reader is also a deterministic race barrier in unit tests.
    #[cfg(test)]
    fn build_with_reader(
        index: &'a Index,
        read: impl FnMut(&Path) -> std::io::Result<Vec<u8>>,
    ) -> Self {
        Self::build_selected(index, read, |_, _| true, true)
    }

    fn build_selected(
        index: &'a Index,
        mut read: impl FnMut(&Path) -> std::io::Result<Vec<u8>>,
        should_parse: impl Fn(&crate::index::FileData, &[u8]) -> bool,
        use_source: bool,
    ) -> Self {
        let lookup = ImportIndex::build(index.entries.keys());
        let mut imports = BTreeMap::new();
        let mut calls = BTreeMap::new();
        let mut candidates: BTreeMap<String, Vec<Candidate<'a>>> = BTreeMap::new();
        let mut files: Vec<_> = index.entries.iter().collect();
        files.sort_by_key(|(path, _)| *path);
        let mut coverage = Coverage {
            model: "direct_syntax_v2",
            scope: "indexed_files",
            generation: index.freshness.generation,
            snapshot_id: String::new(),
            files_checked: files.len(),
            files_analyzed: 0,
            files_skipped_missing_grammar: index.freshness.files_skipped_missing_grammar,
            complete_within_model: true,
            issues: Vec::new(),
            issues_total: 0,
            issues_omitted: 0,
            issue_counts: BTreeMap::new(),
            limitations: "Not compiler resolution: no types, receiver binding, macro expansion, dynamic dispatch, or general import binding. Candidate symbols are indexed syntax facts; parse coverage is only the requested call-site scope. Unindexed files are outside the snapshot; metadata freshness retains its blind spots.",
        };
        let mut trustworthy_candidates = index.freshness.files_skipped_missing_grammar == 0;
        let mut manifest = b"direct_syntax_v2/callee-heads-v3\0".to_vec();
        for (path, data) in files {
            let imported: BTreeSet<PathBuf> = data
                .imports
                .iter()
                .filter_map(
                    |import| match lookup.resolve(path, import, &data.meta.language) {
                        Resolved::File(file) => Some(file),
                        _ => None,
                    },
                )
                .collect();
            imports.insert(path.clone(), imported);
            for symbol in &data.symbols {
                if symbol.kind == SymbolKind::Fn
                    && matches!(
                        symbol.role,
                        SymbolRole::Definition | SymbolRole::Declaration
                    )
                {
                    candidates
                        .entry(symbol.name.clone())
                        .or_default()
                        .push(Candidate {
                            id: DefinitionSiteId {
                                file: path.clone(),
                                language: data.meta.language.clone(),
                                range: symbol.byte_range,
                            },
                            symbol,
                            lexical_parent: data
                                .symbols
                                .iter()
                                .filter(|parent| {
                                    parent.kind == SymbolKind::Fn
                                        && parent.byte_range != symbol.byte_range
                                        && parent.byte_range.0 <= symbol.byte_range.0
                                        && parent.byte_range.1 >= symbol.byte_range.1
                                })
                                .min_by_key(|parent| parent.byte_range.1 - parent.byte_range.0)
                                .map(|parent| parent.byte_range),
                            signature: Some(signature_key(symbol, &symbol.signature)),
                            declarations: Vec::new(),
                        });
                }
            }
            manifest.extend_from_slice(display_path(path).as_bytes());
            manifest.push(0);
            manifest.extend_from_slice(data.meta.language.as_bytes());
            manifest.push(0);
            manifest.extend_from_slice(&data.meta.content_hash.to_le_bytes());
            let reason = if !use_source {
                None
            } else if !crate::language::supports_calls(&data.meta.language) {
                Some(CoverageReason::UnsupportedLanguage)
            } else {
                match read(&index.root.join(path)) {
                    Err(_) => Some(CoverageReason::ReadFailed),
                    Ok(source) if content_hash(&source) != data.meta.content_hash => {
                        Some(CoverageReason::ContentChanged)
                    }
                    Ok(source) => {
                        // The complete prototype is persisted in v15; reading it
                        // again remains a legacy one-hop consistency check.
                        for symbol in data
                            .symbols
                            .iter()
                            .filter(|s| s.role == SymbolRole::Declaration)
                        {
                            if let Some(candidate) =
                                candidates.get_mut(&symbol.name).and_then(|set| {
                                    set.iter_mut().find(|c| {
                                        c.id.file == *path && c.id.range == symbol.byte_range
                                    })
                                })
                            {
                                candidate.signature = source
                                    .get(symbol.byte_range.0..symbol.byte_range.1)
                                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                                    .map(|text| signature_key(symbol, text));
                            }
                        }
                        if !should_parse(data, &source) {
                            continue;
                        }
                        match crate::language::find_call_facts(&data.meta.language, &source, path) {
                            Ok(facts) => {
                                calls.insert(path.clone(), facts.sites);
                                coverage.files_analyzed += 1;
                                if facts.has_parse_errors {
                                    Some(CoverageReason::ParseError)
                                } else {
                                    (facts.unsupported_calls > 0)
                                        .then_some(CoverageReason::UnsupportedCallForm)
                                }
                            }
                            Err(LangError::NotInstalled(_)) => Some(CoverageReason::MissingGrammar),
                            Err(LangError::ParseFailed) => Some(CoverageReason::ParseError),
                        }
                    }
                }
            };
            if let Some(reason) = reason {
                if !matches!(
                    reason,
                    CoverageReason::UnsupportedLanguage | CoverageReason::UnsupportedCallForm
                ) {
                    trustworthy_candidates = false;
                }
                coverage.issues_total += 1;
                *coverage.issue_counts.entry(reason).or_default() += 1;
                coverage.issues.push(CoverageIssue {
                    file: display_path(path),
                    reason,
                });
            }
        }
        // Actionable failures must survive the bounded sample. A large README/
        // Python inventory must not hide the one C++ file that failed analysis.
        coverage
            .issues
            .sort_by(|a, b| a.reason.cmp(&b.reason).then(a.file.cmp(&b.file)));
        coverage.issues_omitted = coverage.issues.len().saturating_sub(16);
        coverage.issues.truncate(16);
        // Association is deliberately narrower than C++ semantic equivalence.
        // Unmatched declarations remain candidates, even if some definition exists.
        for set in candidates.values_mut() {
            let mut links = Vec::new();
            for (d, decl) in set
                .iter()
                .enumerate()
                .filter(|(_, c)| c.symbol.role == SymbolRole::Declaration)
            {
                let definitions: Vec<_> = set
                    .iter()
                    .enumerate()
                    .filter(|(_, def)| {
                        def.symbol.role == SymbolRole::Definition
                            && def.id.language == decl.id.language
                            && matches!(def.id.language.as_str(), "c" | "cpp")
                            && def.symbol.qualified_name == decl.symbol.qualified_name
                            && decl.signature.is_some()
                            && def.signature == decl.signature
                            && (def.id.file == decl.id.file
                                || imports
                                    .get(&def.id.file)
                                    .is_some_and(|files| files.contains(&decl.id.file)))
                    })
                    .map(|(i, _)| i)
                    .collect();
                if let [definition] = definitions.as_slice() {
                    links.push((d, *definition));
                }
            }
            for (decl, def) in &links {
                let id = set[*decl].id.clone();
                set[*def].declarations.push(id);
            }
            for (decl, _) in links.into_iter().rev() {
                set.remove(decl);
            }
            set.sort_by(|a, b| a.id.cmp(&b.id));
        }
        coverage.snapshot_id = format!("{:016x}", content_hash(&manifest));
        coverage.complete_within_model = coverage.issues_total == 0 && trustworthy_candidates;
        Self {
            candidates,
            calls,
            coverage,
            imports,
            trustworthy_candidates,
        }
    }

    pub fn named(&self, name: &str) -> &[Candidate<'a>] {
        self.candidates.get(name).map_or(&[], Vec::as_slice)
    }

    /// Resolve to a typed candidate, never a display name. A written qualifier
    /// that fails to match does not fall through to unrelated lexical candidates.
    pub fn resolve<'b>(
        &'b self,
        file: &Path,
        language: &str,
        caller: Option<&Symbol>,
        site: &CallSite,
    ) -> Resolution<'b, 'a> {
        let candidates: Vec<_> = self
            .named(&site.name)
            .iter()
            .filter(|c| c.id.language == language)
            .collect();
        let unresolved = || Resolution {
            to: None,
            level: ResolutionLevel::Syntax,
            candidates: candidates.clone(),
        };
        if !self.trustworthy_candidates || site.indirect || language == "html" {
            return unresolved();
        }
        let resolve = |set: Vec<&'b Candidate<'a>>, level| {
            if set.len() == 1 {
                Resolution {
                    to: Some(set[0]),
                    level,
                    candidates: Vec::new(),
                }
            } else {
                Resolution {
                    to: None,
                    level: ResolutionLevel::Syntax,
                    candidates: set,
                }
            }
        };
        if let Some(qualifier) = &site.qualifier {
            let separator = if language == "typescript" { "." } else { "::" };
            let prefix = qualifier.trim_start_matches("::");
            let written = if prefix.is_empty() {
                site.name.clone()
            } else {
                format!("{prefix}{separator}{}", site.name)
            };
            let matching: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|c| {
                    c.symbol.qualified_name.as_deref() == Some(written.as_str())
                        || (!qualifier.starts_with("::")
                            && caller.is_some_and(|host| {
                                (1..=host.scope_path.len()).any(|n| {
                                    c.symbol.qualified_name.as_deref()
                                        == Some(
                                            format!(
                                                "{}{}{}",
                                                host.scope_path[..n].join(separator),
                                                separator,
                                                written
                                            )
                                            .as_str(),
                                        )
                                })
                            }))
                })
                .collect();
            return if matching.is_empty() {
                unresolved()
            } else {
                resolve(matching, ResolutionLevel::LexicalScope)
            };
        }
        // Lexical visibility requires the same file and the same enclosing
        // execution container, not just equal namespace text in another file.
        let mut scope = caller.map_or_else(Vec::new, |c| c.scope_path.clone());
        if let Some(caller) = caller {
            scope.push(caller.name.clone());
        }
        let imported = |c: &Candidate<'_>| {
            matches!(language, "c" | "cpp")
                && self.imports.get(file).is_some_and(|files| {
                    files.contains(&c.id.file)
                        || c.declarations.iter().any(|id| files.contains(&id.file))
                })
        };
        let visible: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|c| {
                (c.id.file == file || imported(c))
                    && scope.starts_with(&c.symbol.scope_path)
                    && c.lexical_parent.is_none_or(|(start, end)| {
                        start <= site.byte_offset && site.byte_offset < end
                    })
            })
            .collect();
        if !visible.is_empty() {
            let depth = visible
                .iter()
                .map(|c| c.symbol.scope_path.len())
                .max()
                .unwrap();
            let level = if visible.iter().any(|c| c.id.file == file) {
                ResolutionLevel::LexicalScope
            } else {
                ResolutionLevel::ImportResolved
            };
            return resolve(
                visible
                    .into_iter()
                    .filter(|c| c.symbol.scope_path.len() == depth)
                    .collect(),
                level,
            );
        }
        if candidates.iter().any(|c| c.lexical_parent.is_some()) {
            return unresolved();
        }
        resolve(candidates, ResolutionLevel::Syntax)
    }

    pub fn warning(&self) -> String {
        format!(
            "relation_coverage: {}",
            serde_json::to_string(&self.coverage).expect("coverage is serializable")
        )
    }
}

pub struct Resolution<'b, 'a> {
    pub to: Option<&'b Candidate<'a>>,
    pub level: ResolutionLevel,
    pub candidates: Vec<&'b Candidate<'a>>,
}

pub fn candidate_labels(candidates: &[&Candidate<'_>]) -> Vec<String> {
    let mut labels: Vec<_> = candidates
        .iter()
        .map(|candidate| {
            if candidates
                .iter()
                .filter(|c| c.label() == candidate.label())
                .count()
                > 1
            {
                candidate.site_label()
            } else {
                candidate.label()
            }
        })
        .collect();
    labels.sort();
    labels
}

fn signature_key(symbol: &Symbol, signature: &str) -> String {
    let mut signature = signature.trim().trim_end_matches(';').to_string();
    for scope in symbol.scope_path.iter().rev() {
        signature = signature.replace(&format!("{scope}::{}", symbol.name), &symbol.name);
    }
    signature.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn label_of(symbol: &Symbol) -> String {
    symbol
        .qualified_name
        .clone()
        .unwrap_or_else(|| symbol.name.clone())
}
pub fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests;
