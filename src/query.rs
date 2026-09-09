use std::fs;
use std::path::{Path, PathBuf};

use memchr::memmem;
use serde::Serialize;

use crate::index::{FileData, Freshness, Index, Symbol, SymbolKind, SymbolRole};
use crate::language::{self, detect_language};
use crate::output::{
    Envelope, ErrorCode, PageInfo, QueryInfo, command_with_all, command_with_offset,
    print_error_json, print_json, print_toon,
};
use crate::util::glob::glob_match;

// --- Pagination ---

/// Pagination parameters resolved from CLI flags.
pub struct Pagination {
    /// Max results to return (None = unlimited).
    pub limit: Option<usize>,
    /// Number of results to skip.
    pub offset: usize,
}

/// Result of applying pagination to a result set.
struct Paginated<T> {
    /// The visible slice after offset + limit.
    items: Vec<T>,
    /// Total number of results before pagination.
    total: usize,
    /// The offset that was applied.
    offset: usize,
    /// The limit that was applied (None = unlimited).
    limit: Option<usize>,
}

impl<T> Paginated<T> {
    /// True when results were cut off (more items exist after this page).
    const fn was_truncated(&self) -> bool {
        self.offset + self.items.len() < self.total
    }

    const fn page_info(&self) -> PageInfo {
        PageInfo {
            total: self.total,
            offset: self.offset,
            limit: self.limit,
            truncated: self.was_truncated(),
        }
    }
}

fn paginate<T>(items: Vec<T>, pg: &Pagination) -> Paginated<T> {
    let total = items.len();
    let visible = items
        .into_iter()
        .skip(pg.offset)
        .take(pg.limit.unwrap_or(usize::MAX))
        .collect();
    Paginated {
        items: visible,
        total,
        offset: pg.offset,
        limit: pg.limit,
    }
}

/// A query that could not be answered, as opposed to one that found nothing
/// (roadmap §6.2).
struct QueryFailure {
    code: ErrorCode,
    message: String,
}

/// Emit a failure: an error envelope under `--json`, the familiar `cx: ...`
/// line otherwise.  Always exit code 1.
fn fail(
    json: bool,
    kind: &'static str,
    subject: Option<String>,
    freshness: &Freshness,
    failure: QueryFailure,
) -> i32 {
    if json {
        print_error_json(
            QueryInfo::new(kind, subject),
            freshness.clone(),
            failure.code,
            &failure.message,
        );
    } else {
        eprintln!("cx: {}", failure.message);
    }
    1
}

/// Emit a successful page of results.
///
/// JSON always returns the same envelope object, whether the result set is
/// empty, complete, or truncated (roadmap §6.1).  TOON keeps its compact
/// tabular body plus the stderr hints agents already rely on.
fn emit<T: Serialize>(
    json: bool,
    kind: &'static str,
    subject: Option<String>,
    freshness: &Freshness,
    paged: &Paginated<T>,
    hint_noun: &str,
    narrow_hint: &str,
) -> i32 {
    if json {
        let mut next_queries = Vec::new();
        if paged.was_truncated() {
            next_queries.push(command_with_offset(paged.offset + paged.items.len()));
            if paged.limit.is_some() {
                next_queries.push(command_with_all());
            }
        }
        let envelope = Envelope::new(
            QueryInfo::new(kind, subject),
            freshness.clone(),
            paged.page_info(),
            &paged.items,
        )
        .with_next_queries(next_queries);
        print_json(&envelope);
        return 0;
    }

    if paged.items.is_empty() {
        eprintln!("cx: no matches");
        return 0;
    }
    print_toon(&paged.items);
    if paged.was_truncated() {
        emit_pagination_hint(
            paged.total,
            paged.offset,
            paged.items.len(),
            hint_noun,
            narrow_hint,
        );
    }
    0
}

/// Narrowing hints shown on stderr when TOON output is truncated.
const NARROW_SYMBOLS: &str = "--file PATH | --kind KIND | --role ROLE";
const NARROW_FROM: &str = "--from PATH";
const NARROW_FILE: &str = "--file PATH";
const NARROW_SUBDIR: &str = "cx overview <subdir>";

/// Maximum number of distinct qualified names listed in an ambiguity warning
/// before the rest are elided, so the warning itself stays bounded.
const AMBIGUITY_LIST_LIMIT: usize = 5;

/// Emit a compact pagination hint on stderr.
fn emit_pagination_hint(
    total: usize,
    offset: usize,
    shown: usize,
    subject: &str,
    narrow_hint: &str,
) {
    let next_offset = offset + shown;
    eprintln!(
        "cx: {shown}/{total} {subject} | {narrow_hint} to narrow | --offset {next_offset} for more | --all"
    );
}

// --- Serializable output types ---

#[derive(Serialize)]
struct SymbolRowOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    name: String,
    /// Fully qualified lexical name, or empty when cx does not model this
    /// language's scopes (roadmap §5.3).
    qualified: String,
    kind: String,
    role: String,
    signature: String,
}

#[derive(Serialize)]
struct SymbolRowWithRangeOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    name: String,
    qualified: String,
    kind: String,
    role: String,
    range: String,
    signature: String,
}

#[derive(Serialize)]
struct DefinitionResult {
    file: String,
    line: usize,
    qualified: String,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lines: Option<usize>,
    body: String,
}

/// Qualified name for output: the modelled qualified name, or an empty string
/// when the language's scopes are not modelled.  Empty means "unresolved", never
/// "top level" — a top-level symbol in a modelled language reports its own name.
fn qualified_or_empty(symbol: &Symbol) -> String {
    symbol.qualified_name.clone().unwrap_or_default()
}

// --- Query implementations ---

struct SymbolRow<'a> {
    file: &'a Path,
    symbol: &'a Symbol,
}

/// Symbol selection filters shared by `symbols` and `definition`.
///
/// Grouped rather than passed positionally so adding a dimension (role in
/// Phase 2, qualified scope in Phase 5) does not grow every call site.
#[derive(Default)]
pub struct Filters<'a> {
    /// Restrict to one file, or to a directory subtree.
    pub file: Option<&'a Path>,
    /// Glob matched against the symbol name.
    pub name_glob: Option<&'a str>,
    /// Glob matched against the qualified name, for disambiguating same-name
    /// symbols in different scopes (e.g. `--scope 'alpha::*'`).
    pub scope_glob: Option<&'a str>,
    pub kind: Option<SymbolKind>,
    pub role: Option<SymbolRole>,
}

/// Execute the symbols query with optional file, name glob, kind and role filters.
/// When scoped to a single file, omits the file column from output.
pub fn symbols(
    index: &Index,
    filters: &Filters<'_>,
    ranges: bool,
    json: bool,
    pg: &Pagination,
) -> i32 {
    let (file, name_glob, scope_glob, kind_filter, role_filter) = (
        filters.file,
        filters.name_glob,
        filters.scope_glob,
        filters.kind,
        filters.role,
    );
    // `ranges` is only set by `cx overview <file>`; report that as the query kind.
    let query_kind = if ranges { "overview" } else { "symbols" };
    let mut rows: Vec<SymbolRow<'_>> = Vec::new();

    let rel_path = file.map(|f| make_relative(f, &index.root));
    let subject = name_glob
        .map(std::string::ToString::to_string)
        .or_else(|| rel_path.as_deref().map(display_path));

    let files_to_search: Vec<(&PathBuf, &FileData)> = match rel_path {
        Some(ref rel) => match resolve_file_filter(rel, index) {
            Ok(v) => v,
            Err(failure) => return fail(json, query_kind, subject, &index.freshness, failure),
        },
        None => index.entries.iter().collect(),
    };
    let is_single_file = file.is_some() && files_to_search.len() == 1;

    for (path, data) in files_to_search {
        for sym in &data.symbols {
            if let Some(pattern) = name_glob
                && !glob_match(pattern, &sym.name)
            {
                continue;
            }

            if let Some(kind) = kind_filter
                && sym.kind != kind
            {
                continue;
            }

            if let Some(role) = role_filter
                && sym.role != role
            {
                continue;
            }

            // Scope filtering only matches symbols whose scope cx actually
            // resolved; an unresolved symbol is never assumed to be in scope.
            if let Some(pattern) = scope_glob {
                let Some(qualified) = sym.qualified_name.as_deref() else {
                    continue;
                };
                if !glob_match(pattern, qualified) {
                    continue;
                }
            }

            rows.push(SymbolRow {
                file: path,
                symbol: sym,
            });
        }
    }

    // An empty result set is a successful query with no matches, not an error,
    // so it must still produce the standard envelope (roadmap §6.1/§6.2).
    if rows.is_empty() {
        let empty: Paginated<SymbolRowOut> = Paginated {
            items: Vec::new(),
            total: 0,
            offset: pg.offset,
            limit: pg.limit,
        };
        return emit(
            json,
            query_kind,
            subject,
            &index.freshness,
            &empty,
            "symbols",
            NARROW_SYMBOLS,
        );
    }

    rows.sort_by(|a, b| a.file.cmp(b.file).then(a.symbol.name.cmp(&b.symbol.name)));

    let single_file = is_single_file;
    let mut line_cache = std::collections::HashMap::new();
    if ranges {
        let out: Vec<SymbolRowWithRangeOut> = rows
            .into_iter()
            .map(|r| SymbolRowWithRangeOut {
                file: if single_file {
                    None
                } else {
                    Some(display_path(r.file))
                },
                name: r.symbol.name.clone(),
                qualified: qualified_or_empty(r.symbol),
                kind: r.symbol.kind.as_str().to_string(),
                role: r.symbol.role.as_str().to_string(),
                range: line_range(index, &mut line_cache, r.file, r.symbol.byte_range)
                    .unwrap_or_default(),
                signature: r.symbol.signature.clone(),
            })
            .collect();
        let paged = paginate(out, pg);
        emit(
            json,
            query_kind,
            subject,
            &index.freshness,
            &paged,
            "symbols",
            NARROW_SYMBOLS,
        )
    } else {
        let out: Vec<SymbolRowOut> = rows
            .into_iter()
            .map(|r| SymbolRowOut {
                file: if single_file {
                    None
                } else {
                    Some(display_path(r.file))
                },
                name: r.symbol.name.clone(),
                qualified: qualified_or_empty(r.symbol),
                kind: r.symbol.kind.as_str().to_string(),
                role: r.symbol.role.as_str().to_string(),
                signature: r.symbol.signature.clone(),
            })
            .collect();
        let paged = paginate(out, pg);
        emit(
            json,
            query_kind,
            subject,
            &index.freshness,
            &paged,
            "symbols",
            NARROW_SYMBOLS,
        )
    }
}

/// Serializable row for `kind_counts` output.
#[derive(Serialize)]
struct KindCountRow {
    kind: String,
    count: usize,
}

/// List distinct symbol kinds with their counts, optionally scoped to a file.
pub fn kind_counts(index: &Index, file: Option<&Path>, json: bool) -> i32 {
    let rel_path = file.map(|f| make_relative(f, &index.root));
    let subject = rel_path.as_deref().map(display_path);

    let files_to_search: Vec<(&PathBuf, &FileData)> = match rel_path {
        Some(ref rel) => match resolve_file_filter(rel, index) {
            Ok(v) => v,
            Err(failure) => return fail(json, "kinds", subject, &index.freshness, failure),
        },
        None => index.entries.iter().collect(),
    };

    let mut counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    for (_path, data) in files_to_search {
        for sym in &data.symbols {
            *counts.entry(sym.kind.as_str()).or_insert(0) += 1;
        }
    }

    let mut rows: Vec<KindCountRow> = counts
        .into_iter()
        .map(|(kind, count)| KindCountRow {
            kind: kind.to_string(),
            count,
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.count));

    // Kind counts are already an aggregate, so they are never paginated.
    let total = rows.len();
    let paged = Paginated {
        items: rows,
        total,
        offset: 0,
        limit: None,
    };
    emit(
        json,
        "kinds",
        subject,
        &index.freshness,
        &paged,
        "kinds",
        NARROW_FILE,
    )
}

/// Execute the definition query: find symbol by exact name, return its body.
///
/// `filters.file` acts as the `--from` disambiguator; `filters.name_glob` is
/// unused here because definition matches an exact name.
pub fn definition(
    index: &Index,
    name: &str,
    filters: &Filters<'_>,
    max_lines: usize,
    json: bool,
    pg: &Pagination,
) -> i32 {
    let (from, scope_glob, kind_filter, role_filter) =
        (filters.file, filters.scope_glob, filters.kind, filters.role);
    let from_rel = from.map(|f| make_relative(f, &index.root));

    // Language travels with each match so identity can be language-scoped.
    let mut matches: Vec<(&PathBuf, &Symbol, &str)> = Vec::new();
    for (path, data) in &index.entries {
        for sym in &data.symbols {
            if sym.name == name {
                if let Some(kind) = kind_filter
                    && sym.kind != kind
                {
                    continue;
                }
                if let Some(role) = role_filter
                    && sym.role != role
                {
                    continue;
                }
                if let Some(pattern) = scope_glob {
                    let Some(qualified) = sym.qualified_name.as_deref() else {
                        continue;
                    };
                    if !glob_match(pattern, qualified) {
                        continue;
                    }
                }
                matches.push((path, sym, data.meta.language.as_str()));
            }
        }
    }

    if let Some(ref from_path) = from_rel {
        let is_dir = index.root.join(from_path).is_dir();
        let from_matches: Vec<_> = matches
            .iter()
            .filter(|(path, _, _)| {
                if is_dir {
                    path.starts_with(from_path)
                } else {
                    *path == from_path
                }
            })
            .copied()
            .collect();
        if !from_matches.is_empty() {
            matches = from_matches;
        }
    }

    // An empty match set is a successful query with no results, not an error,
    // so it flows through to the normal emit path (roadmap §6.2).

    // Distinct logical symbols among the matches, by qualified identity.  A
    // declaration and its definition share one identity, so this counts real
    // ambiguity rather than counting locations (roadmap §5.2).
    let mut distinct: Vec<String> = Vec::new();
    for (_, sym, lang) in &matches {
        let id = sym.stable_id(lang);
        let key = id.logical_key();
        let label = format!("{}:{}", key.0, key.1);
        if !distinct.contains(&label) {
            distinct.push(label);
        }
    }
    // Sorted so the warning text is stable across runs: index iteration order is
    // not, and an unstable warning is not something a test can pin.
    distinct.sort_unstable();
    let unresolved_scopes = matches
        .iter()
        .filter(|(_, sym, _)| sym.qualified_name.is_none())
        .count();

    // Sort implementations ahead of signature-only sites, then by symbol
    // priority (types first), then by file path.  An agent asking for a
    // definition wants the body, not the prototype (roadmap §5.1).
    matches.sort_by(|a, b| {
        role_priority(a.1.role)
            .cmp(&role_priority(b.1.role))
            .then(symbol_priority(a.1.kind).cmp(&symbol_priority(b.1.kind)))
            .then(a.0.cmp(b.0))
    });

    // Paginate matches BEFORE reading bodies to avoid pointless disk I/O
    let paged_matches = paginate(matches, pg);

    let results: Vec<DefinitionResult> = paged_matches
        .items
        .iter()
        .map(|(path, sym, _)| {
            let (body, start_line) =
                read_body(&index.root, path, sym.byte_range).unwrap_or((String::new(), 0));
            let line_count = body.lines().count();
            let truncated = line_count > max_lines;

            let display_body = if truncated {
                body.lines().take(max_lines).collect::<Vec<_>>().join("\n")
            } else {
                body
            };

            DefinitionResult {
                file: display_path(path),
                line: start_line,
                qualified: qualified_or_empty(sym),
                role: sym.role.as_str().to_string(),
                truncated: if truncated { Some(true) } else { None },
                lines: if truncated { Some(line_count) } else { None },
                body: display_body,
            }
        })
        .collect();

    if json {
        let mut next_queries = Vec::new();
        if paged_matches.was_truncated() {
            next_queries.push(command_with_offset(paged_matches.offset + results.len()));
            if paged_matches.limit.is_some() {
                next_queries.push(command_with_all());
            }
        }
        // Report ambiguity in terms of distinct symbols, not row count: a C++
        // prototype plus its definition is one symbol at two locations and must
        // not be announced as a conflict (roadmap §6.2, §5.2).
        let mut warnings = Vec::new();
        if distinct.len() > 1 {
            let mut shown: Vec<&str> = distinct
                .iter()
                .take(AMBIGUITY_LIST_LIMIT)
                .map(|d| d.split_once(':').map_or(d.as_str(), |(_, n)| n))
                .collect();
            let elided = distinct.len().saturating_sub(shown.len());
            if elided > 0 {
                shown.push("...");
            }
            warnings.push(format!(
                "{} distinct symbols named \"{name}\": {}. Narrow with --scope or --from.",
                distinct.len(),
                shown.join(", ")
            ));
        }
        if unresolved_scopes > 0 {
            warnings.push(format!(
                "{unresolved_scopes} of {} matches have unresolved scope (language not modelled); they are not disambiguated",
                paged_matches.total
            ));
        }
        let envelope = Envelope::new(
            QueryInfo::new("definition", Some(name.to_string())),
            index.freshness.clone(),
            paged_matches.page_info(),
            &results,
        )
        .with_warnings(warnings)
        .with_next_queries(next_queries);
        print_json(&envelope);
        return 0;
    }

    if results.is_empty() {
        eprintln!("cx: no matches");
        return 0;
    }

    for (i, r) in results.iter().enumerate() {
        if i > 0 {
            println!();
        }
        print!("file: {}\nline: {}", r.file, r.line);
        if !r.qualified.is_empty() {
            print!("\nqualified: {}", r.qualified);
        }
        print!("\nrole: {}", r.role);
        if let Some(total) = r.lines {
            print!("\ntruncated: {total} lines total");
        }
        println!("\n---\n{}", r.body);
    }

    if distinct.len() > 1 {
        eprintln!(
            "cx: {} distinct symbols named \"{name}\" — narrow with --scope or --from",
            distinct.len()
        );
    }

    if paged_matches.was_truncated() {
        let hint_noun = format!("definitions for \"{name}\"");
        emit_pagination_hint(
            paged_matches.total,
            paged_matches.offset,
            results.len(),
            &hint_noun,
            NARROW_FROM,
        );
    }

    0
}

#[derive(Serialize)]
struct ReferenceRow {
    file: String,
    line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    caller: Option<String>,
    /// What the AST says this occurrence is (roadmap §5.4, §14).
    evidence: String,
    /// How strongly the occurrence was established.  References are syntax-level
    /// by construction: cx identifies the node, not the resolved target.
    resolution: String,
    context: String,
}

/// Label a reference occurrence with its evidence kind.
///
/// The AST classification is upgraded to definition/declaration when the
/// tightest enclosing symbol *is* the symbol being referenced — that occurrence
/// is the symbol's own name, not a use of it.
fn reference_evidence(
    symbols: &[Symbol],
    name: &str,
    byte_offset: usize,
    ast: crate::language::RefEvidence,
) -> crate::relations::EvidenceKind {
    use crate::language::RefEvidence;
    use crate::relations::EvidenceKind;

    let own_site = symbols
        .iter()
        .filter(|s| s.byte_range.0 <= byte_offset && byte_offset < s.byte_range.1)
        .min_by_key(|s| s.byte_range.1 - s.byte_range.0)
        .filter(|s| s.name == name);
    if let Some(symbol) = own_site {
        return match symbol.role {
            SymbolRole::Declaration => EvidenceKind::Declaration,
            _ => EvidenceKind::Definition,
        };
    }

    match ast {
        RefEvidence::Call => EvidenceKind::Call,
        RefEvidence::TypeReference => EvidenceKind::TypeReference,
        RefEvidence::Import => EvidenceKind::Import,
        RefEvidence::Identifier => EvidenceKind::IdentifierReference,
    }
}

/// Find the enclosing symbol for a byte offset in a file's symbol list.
fn find_enclosing_symbol(symbols: &[Symbol], byte_offset: usize) -> Option<&str> {
    symbols
        .iter()
        .filter(|s| s.byte_range.0 <= byte_offset && byte_offset < s.byte_range.1)
        // Pick the tightest enclosing symbol (smallest range)
        .min_by_key(|s| s.byte_range.1 - s.byte_range.0)
        .map(|s| s.name.as_str())
}

/// File-level reference summary for default references output.
#[derive(Serialize)]
struct ReferenceSummaryRow {
    file: String,
    lines: String,
    refs: usize,
    callers: String,
}

/// Find all usages of a symbol name across project files.
pub fn references(
    index: &Index,
    name: &str,
    file: Option<&Path>,
    context: bool,
    json: bool,
    pg: &Pagination,
) -> i32 {
    let rel_path = file.map(|f| make_relative(f, &index.root));
    let subject = Some(name.to_string());

    let files_to_search: Vec<(&PathBuf, &FileData)> = match rel_path {
        Some(ref rel) => match resolve_file_filter(rel, index) {
            Ok(v) => v,
            Err(failure) => return fail(json, "references", subject, &index.freshness, failure),
        },
        None => index.entries.iter().collect(),
    };

    let mut rows: Vec<ReferenceRow> = Vec::new();
    let name_bytes = name.as_bytes();

    for (path, data) in files_to_search {
        let abs_path = index.root.join(path);
        let source = match fs::read(&abs_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Skip files that can't possibly contain the name
        if memmem::find(&source, name_bytes).is_none() {
            continue;
        }

        let refs = match language::find_references(&data.meta.language, &source, &abs_path, name) {
            Ok(r) => r,
            Err(language::LangError::NotInstalled(lang)) => {
                return fail(
                    json,
                    "references",
                    subject,
                    &index.freshness,
                    QueryFailure {
                        code: ErrorCode::GrammarNotInstalled,
                        message: format!("{lang} grammar not installed — run: cx lang add {lang}"),
                    },
                );
            }
            Err(_) => continue,
        };
        if refs.is_empty() {
            continue;
        }

        // Convert to str once per file for context extraction
        let text = std::str::from_utf8(&source).ok();
        let lines: Vec<&str> = text.map(|t| t.lines().collect()).unwrap_or_default();

        for r in refs {
            let context = lines
                .get(r.line.wrapping_sub(1))
                .map(|l| l.trim().to_string())
                .unwrap_or_default();
            let caller = find_enclosing_symbol(&data.symbols, r.byte_offset)
                .map(std::string::ToString::to_string);
            let evidence = reference_evidence(&data.symbols, name, r.byte_offset, r.evidence);
            rows.push(ReferenceRow {
                file: display_path(path),
                line: r.line,
                caller,
                evidence: evidence.as_str().to_string(),
                // Syntax is the honest ceiling here: the node kind is known, the
                // target is not resolved.  Use `cx callers` for edge resolution.
                resolution: crate::relations::ResolutionLevel::Syntax
                    .as_str()
                    .to_string(),
                context,
            });
        }
    }

    // An empty result set is not an error: it flows through the normal path so
    // `--json` still returns the standard envelope (roadmap §6.2).
    rows.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    rows.dedup_by(|a, b| a.file == b.file && a.line == b.line);

    let hint_noun = format!("references for \"{name}\"");

    if !context {
        let mut by_file: std::collections::BTreeMap<
            String,
            (usize, std::collections::BTreeSet<String>, Vec<usize>),
        > = std::collections::BTreeMap::new();
        for row in rows {
            let entry = by_file
                .entry(row.file)
                .or_insert_with(|| (0, std::collections::BTreeSet::new(), Vec::new()));
            entry.0 += 1;
            if let Some(caller) = row.caller {
                entry.1.insert(caller);
            }
            entry.2.push(row.line);
        }
        let summary_rows: Vec<ReferenceSummaryRow> = by_file
            .into_iter()
            .map(|(file, (refs, callers, lines))| ReferenceSummaryRow {
                file,
                lines: lines
                    .into_iter()
                    .map(|line| line.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                refs,
                callers: callers.into_iter().collect::<Vec<_>>().join(", "),
            })
            .collect();
        let paged = paginate(summary_rows, pg);
        emit(
            json,
            "references",
            subject,
            &index.freshness,
            &paged,
            &hint_noun,
            NARROW_FILE,
        )
    } else {
        let paged = paginate(rows, pg);
        emit(
            json,
            "references",
            subject,
            &index.freshness,
            &paged,
            &hint_noun,
            NARROW_FILE,
        )
    }
}

// --- Direct relations (callers / callees) ---

/// Emit a direct caller/callee report (roadmap §9).
///
/// Warnings carry the ambiguity and resolution caveats the relation query
/// produced, so an edge with an empty target is never mistaken for "no caller".
pub fn relation_report(
    index: &Index,
    kind: &'static str,
    name: &str,
    report: crate::relations::RelationReport,
    json: bool,
    pg: &Pagination,
) -> i32 {
    let warnings = report.warnings;
    let paged = paginate(report.rows, pg);

    if json {
        let mut next_queries = Vec::new();
        if paged.was_truncated() {
            next_queries.push(command_with_offset(paged.offset + paged.items.len()));
            if paged.limit.is_some() {
                next_queries.push(command_with_all());
            }
        }
        let envelope = Envelope::new(
            QueryInfo::new(kind, Some(name.to_string())),
            index.freshness.clone(),
            paged.page_info(),
            &paged.items,
        )
        .with_warnings(warnings)
        .with_next_queries(next_queries);
        print_json(&envelope);
        return 0;
    }

    if paged.items.is_empty() {
        eprintln!("cx: no matches");
        for warning in &warnings {
            eprintln!("cx: {warning}");
        }
        return 0;
    }

    print_toon(&paged.items);
    for warning in &warnings {
        eprintln!("cx: {warning}");
    }
    if paged.was_truncated() {
        let hint_noun = format!("{kind} for \"{name}\"");
        emit_pagination_hint(
            paged.total,
            paged.offset,
            paged.items.len(),
            &hint_noun,
            "--scope GLOB",
        );
    }
    0
}

// --- Repository map ---

/// Emit the bounded repository map (roadmap §8).
///
/// Notes explaining what was excluded and how rows were ranked travel in
/// `warnings` under `--json` and on stderr otherwise, so the ranking is never
/// presented as self-evident.
pub fn map_report(
    index: &Index,
    opts: &crate::map::MapOptions<'_>,
    json: bool,
    pg: &Pagination,
) -> i32 {
    let report = crate::map::build(index, opts);
    let paged = paginate(report.rows, pg);

    if json {
        let mut next_queries = Vec::new();
        if paged.was_truncated() {
            next_queries.push(command_with_offset(paged.offset + paged.items.len()));
            if paged.limit.is_some() {
                next_queries.push(command_with_all());
            }
        }
        let mut warnings = report.notes.clone();
        warnings.push(format!("ranked by {}", report.ranked_by));
        let envelope = Envelope::new(
            QueryInfo::new("map", None),
            index.freshness.clone(),
            paged.page_info(),
            &paged.items,
        )
        .with_warnings(warnings)
        .with_next_queries(next_queries);
        print_json(&envelope);
        return 0;
    }

    if paged.items.is_empty() {
        eprintln!("cx: no indexed files match this map's filters");
        for note in &report.notes {
            eprintln!("cx: {note}");
        }
        return 0;
    }

    print_toon(&paged.items);
    for note in &report.notes {
        eprintln!("cx: {note}");
    }
    eprintln!("cx: ranked by {}", report.ranked_by);
    if paged.was_truncated() {
        emit_pagination_hint(
            paged.total,
            paged.offset,
            paged.items.len(),
            "subsystems",
            "--depth N | --exclude GLOB",
        );
    }
    0
}

// --- Refresh ---

/// What `cx refresh` did to one requested path.
#[derive(Serialize)]
struct RefreshRow {
    file: String,
    status: &'static str,
}

/// Report the outcome of an explicit refresh (roadmap §7).
///
/// This is the mechanical evidence an agent needs after editing: every named
/// path is listed with what happened to it, alongside the generation those
/// changes landed in.  A later query reporting the same generation is then
/// proof that the edit is included.
pub fn refresh_report(index: &Index, requested: &[PathBuf], json: bool) -> i32 {
    if let Some(error) = &index.refresh_error {
        if json {
            print_error_json(
                QueryInfo::new("refresh", None),
                index.freshness.clone(),
                ErrorCode::RefreshFailed,
                error,
            );
        } else {
            eprintln!("cx: refresh failed: {error}");
        }
        return 1;
    }
    let mut rows: Vec<RefreshRow> = Vec::new();

    if requested.is_empty() {
        // Whole-project verification: only changes are interesting.
        for path in &index.updated {
            rows.push(RefreshRow {
                file: display_path(path),
                status: "updated",
            });
        }
        for path in &index.removed {
            rows.push(RefreshRow {
                file: display_path(path),
                status: "removed",
            });
        }
    } else {
        for (position, path) in requested.iter().enumerate() {
            let rel = make_relative(path, &index.root);
            let status = if let Some(failure) = index
                .named_path_checks
                .get(position)
                .and_then(|check| check.failure)
            {
                failure
            } else if index.named_path_checks.get(position).is_none() {
                "unverified"
            } else if index.updated.contains(&rel) {
                "updated"
            } else if index.removed.contains(&rel) {
                "removed"
            } else if index.entries.contains_key(&rel) {
                "unchanged"
            } else {
                "not_indexed"
            };
            rows.push(RefreshRow {
                file: display_path(&rel),
                status,
            });
        }
    }
    rows.sort_by(|a, b| a.file.cmp(&b.file));

    if json {
        let total = rows.len();
        let paged = Paginated {
            items: rows,
            total,
            offset: 0,
            limit: None,
        };
        return emit(
            true,
            "refresh",
            None,
            &index.freshness,
            &paged,
            "paths",
            NARROW_FILE,
        );
    }

    let f = &index.freshness;
    if rows.is_empty() {
        eprintln!(
            "cx: generation {} | mode {} | checked {} | nothing changed",
            f.generation, f.mode, f.files_checked
        );
        return 0;
    }
    print_toon(&rows);
    eprintln!(
        "cx: generation {} | mode {} | checked {} | updated {} | removed {}",
        f.generation, f.mode, f.files_checked, f.files_updated, f.files_removed
    );
    0
}

// --- Directory overview ---

const DIR_OVERVIEW_MAX_SYMBOLS: usize = 10;

#[derive(Serialize)]
struct DirOverviewRow {
    file: String,
    symbols: String,
}

#[derive(Serialize)]
struct DirOverviewFullRow {
    file: String,
    name: String,
    kind: String,
    range: String,
    signature: String,
}

/// Priority for symbol roles: implementations before signature-only sites.
/// Unknown sits last so an unlabelled construct never outranks a proven body.
const fn role_priority(role: SymbolRole) -> u8 {
    match role {
        SymbolRole::Definition => 0,
        SymbolRole::Heading => 1,
        SymbolRole::Declaration => 2,
        SymbolRole::Unknown => 3,
    }
}

/// Priority for symbol kinds in directory overview: lower = shown first.
const fn symbol_priority(kind: SymbolKind) -> u8 {
    match kind {
        SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::Trait
        | SymbolKind::Interface
        | SymbolKind::Class => 0,
        SymbolKind::Fn
        | SymbolKind::Const
        | SymbolKind::Type
        | SymbolKind::Module
        | SymbolKind::Event
        | SymbolKind::Heading => 1,
        SymbolKind::Field => 2,
    }
}

/// Check if a file path looks like a test file based on naming conventions.
///
/// Shared with the repository map, which classifies paths the same way.
pub(crate) fn is_test_path(path: &Path) -> bool {
    is_test_file(path)
}

/// Check if a file path looks like a test file based on naming conventions.
fn is_test_file(path: &Path) -> bool {
    for component in path.components() {
        if let std::path::Component::Normal(s) = component {
            let s = s.to_str().unwrap_or("");
            if s == "tests" || s == "test" || s == "__tests__" {
                return true;
            }
        }
    }
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    // Go: *_test.go
    if name.ends_with("_test.go") {
        return true;
    }
    // JS/TS: *.test.* or *.spec.*
    for ext in &[
        ".test.ts",
        ".test.tsx",
        ".test.js",
        ".test.jsx",
        ".spec.ts",
        ".spec.tsx",
        ".spec.js",
        ".spec.jsx",
    ] {
        if name.ends_with(ext) {
            return true;
        }
    }
    // Python: test_*.py
    if name.starts_with("test_") && name.ends_with(".py") {
        return true;
    }
    // Ruby: *_spec.rb
    if name.ends_with("_spec.rb") {
        return true;
    }
    false
}

/// Extract the immediate child component of `path` relative to `dir`.
/// Returns `None` if the path is not under `dir`.
/// For a direct child file, returns the full relative path.
/// For a nested file, returns just the first subdirectory component (with trailing /).
fn child_component(path: &Path, dir: &Path) -> Option<PathBuf> {
    let relative = if dir.as_os_str().is_empty() {
        path.to_path_buf()
    } else {
        path.strip_prefix(dir).ok()?.to_path_buf()
    };
    let mut components = relative.components();
    let first = components.next()?;
    if components.next().is_some() {
        // Nested — return just the subdir name
        Some(PathBuf::from(first.as_os_str()))
    } else {
        // Direct child file
        Some(relative)
    }
}

/// Show a single-level overview of files and subdirectories.
pub fn dir_overview(
    index: &Index,
    dir: &Path,
    full: bool,
    no_tests: bool,
    json: bool,
    pg: &Pagination,
) -> i32 {
    let rel_dir = make_relative(dir, &index.root);
    let rel_dir = if rel_dir == Path::new(".") {
        PathBuf::new()
    } else {
        rel_dir
    };

    let all_entries: Vec<(&PathBuf, &FileData)> = index
        .entries
        .iter()
        .filter(|(path, _)| rel_dir.as_os_str().is_empty() || path.starts_with(&rel_dir))
        .filter(|(path, _)| !no_tests || !is_test_file(path))
        .collect();

    if all_entries.is_empty() {
        return fail(
            json,
            "overview",
            Some(display_path(&rel_dir)),
            &index.freshness,
            QueryFailure {
                code: ErrorCode::NoIndexedFiles,
                message: format!("no indexed files under {}", display_path(&rel_dir)),
            },
        );
    }

    // Partition into direct files and subdirectory aggregates
    let mut direct_files: Vec<(&PathBuf, &FileData)> = Vec::new();
    let mut subdirs: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();

    for (path, data) in &all_entries {
        let child = match child_component(path, &rel_dir) {
            Some(c) => c,
            None => continue,
        };
        let sym_count = if no_tests {
            data.symbols.iter().filter(|s| !s.is_test).count()
        } else {
            data.symbols.len()
        };
        if child.components().count() == 1 && child.extension().is_some() {
            direct_files.push((path, data));
        } else {
            let dir_name = child.to_string_lossy().to_string();
            let entry = subdirs.entry(dir_name).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += sym_count;
        }
    }

    direct_files.sort_by_key(|(path, _)| *path);

    // Shared: format subdir display path
    let format_subdir = |dir_name: &str| -> String {
        if rel_dir.as_os_str().is_empty() {
            format!("{dir_name}/")
        } else {
            format!("{}/{}/", display_path(&rel_dir), dir_name)
        }
    };

    fn prepare_symbols(data: &FileData, no_tests: bool) -> Vec<&Symbol> {
        let mut syms: Vec<&Symbol> = data
            .symbols
            .iter()
            .filter(|s| !no_tests || !s.is_test)
            .collect();
        syms.sort_by(|a, b| {
            symbol_priority(a.kind)
                .cmp(&symbol_priority(b.kind))
                .then(a.name.cmp(&b.name))
        });
        syms
    }

    let subject = Some(display_path(&rel_dir));

    if full {
        let mut rows: Vec<DirOverviewFullRow> = Vec::new();
        let mut line_cache = std::collections::HashMap::new();
        for (dir_name, (file_count, sym_count)) in &subdirs {
            rows.push(DirOverviewFullRow {
                file: format_subdir(dir_name),
                name: format!("({file_count} files, {sym_count} symbols)"),
                kind: String::new(),
                range: String::new(),
                signature: String::new(),
            });
        }
        for (path, data) in &direct_files {
            let syms = prepare_symbols(data, no_tests);
            if syms.is_empty() {
                continue;
            }
            let total = syms.len();
            for sym in syms.iter().take(DIR_OVERVIEW_MAX_SYMBOLS) {
                rows.push(DirOverviewFullRow {
                    file: display_path(path),
                    name: sym.name.clone(),
                    kind: sym.kind.as_str().to_string(),
                    range: line_range(index, &mut line_cache, path, sym.byte_range)
                        .unwrap_or_default(),
                    signature: sym.signature.clone(),
                });
            }
            if total > DIR_OVERVIEW_MAX_SYMBOLS {
                rows.push(DirOverviewFullRow {
                    file: display_path(path),
                    name: format!("... (+{} more)", total - DIR_OVERVIEW_MAX_SYMBOLS),
                    kind: String::new(),
                    range: String::new(),
                    signature: String::new(),
                });
            }
        }
        let paged = paginate(rows, pg);
        emit(
            json,
            "overview",
            subject,
            &index.freshness,
            &paged,
            "entries",
            NARROW_SUBDIR,
        )
    } else {
        let mut rows: Vec<DirOverviewRow> = Vec::new();
        for (dir_name, (file_count, sym_count)) in &subdirs {
            rows.push(DirOverviewRow {
                file: format_subdir(dir_name),
                symbols: format!("({file_count} files, {sym_count} symbols)"),
            });
        }
        for (path, data) in &direct_files {
            let syms = prepare_symbols(data, no_tests);
            if syms.is_empty() {
                continue;
            }
            let total = syms.len();
            // Deduplicate names (e.g. overloaded type params)
            let mut seen = std::collections::HashSet::new();
            let names: Vec<&str> = syms
                .iter()
                .take(DIR_OVERVIEW_MAX_SYMBOLS)
                .map(|s| s.name.as_str())
                .filter(|n| seen.insert(*n))
                .collect();
            let shown = names.len();
            let suffix = if total > shown {
                format!(", ... (+{} more)", total - shown)
            } else {
                String::new()
            };
            rows.push(DirOverviewRow {
                file: display_path(path),
                symbols: format!("{}{}", names.join(", "), suffix),
            });
        }
        let paged = paginate(rows, pg);
        emit(
            json,
            "overview",
            subject,
            &index.freshness,
            &paged,
            "entries",
            NARROW_SUBDIR,
        )
    }
}

fn read_body(root: &Path, file: &Path, byte_range: (usize, usize)) -> Option<(String, usize)> {
    let abs_path = root.join(file);
    let source = fs::read(&abs_path).ok()?;
    let (start, end) = byte_range;
    if end > source.len() {
        return None;
    }
    let line = source[..start].iter().filter(|&&b| b == b'\n').count() + 1;
    let body = String::from_utf8_lossy(&source[start..end]).to_string();
    Some((body, line))
}

fn line_range(
    index: &Index,
    cache: &mut std::collections::HashMap<PathBuf, Vec<usize>>,
    file: &Path,
    byte_range: (usize, usize),
) -> Option<String> {
    let line_starts = match cache.entry(file.to_path_buf()) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            let source = fs::read(index.root.join(file)).ok()?;
            let mut starts = vec![0];
            starts.extend(
                source
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &byte)| (byte == b'\n').then_some(i + 1)),
            );
            entry.insert(starts)
        }
    };

    let (start, end) = byte_range;
    if start > end {
        return None;
    }

    let end_offset = end.saturating_sub(1).max(start);
    let start_line = line_starts.partition_point(|&line_start| line_start <= start);
    let end_line = line_starts.partition_point(|&line_start| line_start <= end_offset);
    Some(if start_line == end_line {
        start_line.to_string()
    } else {
        format!("{start_line}-{end_line}")
    })
}

/// Display a path using forward slashes (consistent across platforms).
fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn resolve_file_filter<'a>(
    rel: &Path,
    index: &'a Index,
) -> Result<Vec<(&'a PathBuf, &'a FileData)>, QueryFailure> {
    if let Some(kv) = index.entries.get_key_value(rel) {
        return Ok(vec![kv]);
    }
    let abs = index.root.join(rel);
    if abs.is_dir() {
        let matches: Vec<_> = index
            .entries
            .iter()
            .filter(|(path, _)| path.starts_with(rel))
            .collect();
        if matches.is_empty() {
            return Err(QueryFailure {
                code: ErrorCode::NoIndexedFiles,
                message: format!("no indexed files under {}", display_path(rel)),
            });
        }
        return Ok(matches);
    }
    if abs.exists() && detect_language(&abs).is_none() {
        let ext = abs.extension().and_then(|e| e.to_str()).unwrap_or("(none)");
        Err(QueryFailure {
            code: ErrorCode::UnsupportedFileType,
            message: format!("unsupported file type: .{ext}"),
        })
    } else {
        Err(QueryFailure {
            code: ErrorCode::FileNotIndexed,
            message: format!("file not in index: {}", display_path(rel)),
        })
    }
}

/// Resolve a user-supplied path to its project-root-relative form.
///
/// Both sides go through the canonical identity (roadmap §4.1) so an argument
/// spelled through a symlink (`/tmp/p/src/a.rs`) still matches an index built
/// under the resolved root (`/private/tmp/p`), and vice versa.  Paths outside
/// the project are returned unchanged so callers can report them verbatim.
fn make_relative(path: &Path, root: &Path) -> PathBuf {
    let abs = crate::util::path::canonical(path);
    let root = crate::util::path::canonical(root);
    abs.strip_prefix(&root).unwrap_or(&abs).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_path_normalizes_backslashes() {
        assert_eq!(display_path(Path::new("src/main.rs")), "src/main.rs");
        assert_eq!(display_path(Path::new("src\\main.rs")), "src/main.rs");
        assert_eq!(
            display_path(Path::new("src\\sub\\file.rs")),
            "src/sub/file.rs"
        );
    }

    // --- is_test_file tests ---

    #[test]
    fn test_file_go() {
        assert!(is_test_file(Path::new("pkg/handler_test.go")));
        assert!(!is_test_file(Path::new("pkg/handler.go")));
    }

    #[test]
    fn test_file_ts_js() {
        assert!(is_test_file(Path::new("src/app.test.ts")));
        assert!(is_test_file(Path::new("src/app.test.tsx")));
        assert!(is_test_file(Path::new("src/app.spec.js")));
        assert!(is_test_file(Path::new("src/app.spec.jsx")));
        assert!(!is_test_file(Path::new("src/app.ts")));
    }

    #[test]
    fn test_file_python() {
        assert!(is_test_file(Path::new("test_utils.py")));
        assert!(!is_test_file(Path::new("utils_test.py"))); // Python convention is test_ prefix
        assert!(!is_test_file(Path::new("test_utils.rs"))); // wrong extension
    }

    #[test]
    fn test_file_ruby() {
        assert!(is_test_file(Path::new("models/user_spec.rb")));
        assert!(!is_test_file(Path::new("models/user.rb")));
    }

    #[test]
    fn test_file_directory() {
        assert!(is_test_file(Path::new("tests/unit/foo.rs")));
        assert!(is_test_file(Path::new("test/foo.js")));
        assert!(is_test_file(Path::new("src/__tests__/app.tsx")));
        assert!(!is_test_file(Path::new("src/foo.rs")));
    }

    #[test]
    fn test_file_normal_files() {
        assert!(!is_test_file(Path::new("src/main.rs")));
        assert!(!is_test_file(Path::new("lib/utils.ts")));
        assert!(!is_test_file(Path::new("index.js")));
    }

    // --- symbol_priority tests ---

    #[test]
    fn symbol_priority_ordering() {
        // Types should come before functions, which come before methods
        assert!(symbol_priority(SymbolKind::Struct) < symbol_priority(SymbolKind::Fn));
        assert!(symbol_priority(SymbolKind::Enum) < symbol_priority(SymbolKind::Fn));
        assert!(symbol_priority(SymbolKind::Trait) < symbol_priority(SymbolKind::Fn));
        assert!(symbol_priority(SymbolKind::Interface) < symbol_priority(SymbolKind::Fn));
        assert!(symbol_priority(SymbolKind::Class) < symbol_priority(SymbolKind::Fn));
        assert!(symbol_priority(SymbolKind::Fn) < symbol_priority(SymbolKind::Field));
    }
}
