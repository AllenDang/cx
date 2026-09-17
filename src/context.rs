//! Deterministic lexical context retrieval. No popularity filler, embedding,
//! generated source text, or guessed runtime relation/test recommendation.
use crate::index::{Index, Symbol};
use crate::output::{ErrorCode, shell_quote};
use crate::snapshot::Sources;
use crate::task::{Failure, Report};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub struct Options {
    pub query: String,
    pub include_body: bool,
    pub include_vendor: bool,
    pub include_generated: bool,
    pub include_fixtures: bool,
    pub no_tests: bool,
    pub snapshot: Option<String>,
}
#[derive(Serialize)]
struct Match {
    field: &'static str,
    terms: Vec<String>,
    line: usize,
    byte_range: Option<(usize, usize)>,
    text: String,
    text_range: Option<(usize, usize)>,
    text_truncated: bool,
}
#[derive(Serialize)]
struct Body {
    text: String,
    byte_range: (usize, usize),
    full_byte_range: (usize, usize),
    truncated: bool,
}
#[derive(Serialize)]
pub struct Row {
    #[serde(serialize_with = "crate::output::serialize_path")]
    pub file: PathBuf,
    pub name: String,
    qualified: Option<String>,
    kind: String,
    role: String,
    line: usize,
    byte_range: (usize, usize),
    score: usize,
    matched_terms: Vec<String>,
    matches: Vec<Match>,
    matches_total: usize,
    matches_omitted: usize,
    signature: String,
    signature_truncated: bool,
    source_hash: String,
    body: Option<Body>,
    body_omitted_for_filter: bool,
    next_queries: Vec<String>,
}
struct Candidate {
    symbol: Option<Symbol>,
    range: (usize, usize),
    score: usize,
    terms: BTreeSet<String>,
    matches: Vec<Match>,
}

/// UTF-8 offsets are carried through case-folded camel/snake subwords.
fn words(text: &str) -> Vec<(String, usize, usize)> {
    let chars: Vec<_> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut start = None;
    for (i, (at, ch)) in chars.iter().copied().enumerate() {
        if !ch.is_alphanumeric() {
            if let Some(s) = start.take() {
                out.push((text[s..at].to_lowercase(), s, at));
            }
            continue;
        }
        let boundary = start.is_some()
            && ch.is_uppercase()
            && i > 0
            && (chars[i - 1].1.is_lowercase()
                || (chars[i - 1].1.is_uppercase()
                    && chars.get(i + 1).is_some_and(|(_, c)| c.is_lowercase())));
        if boundary {
            let s = start.replace(at).unwrap();
            out.push((text[s..at].to_lowercase(), s, at));
        } else {
            start.get_or_insert(at);
        }
    }
    if let Some(s) = start {
        out.push((text[s..].to_lowercase(), s, text.len()));
    }
    out
}
fn terms(text: &str) -> BTreeSet<String> {
    words(text).into_iter().map(|(w, _, _)| w).collect()
}
fn excluded(file: &Path, opts: &Options) -> Option<&'static str> {
    let parts: Vec<_> = file.iter().filter_map(|s| s.to_str()).collect();
    if !opts.include_vendor
        && parts
            .iter()
            .any(|p| matches!(*p, "vendor" | "thirdparty" | "third_party" | "node_modules"))
    {
        return Some("vendor");
    }
    if !opts.include_generated
        && parts
            .iter()
            .any(|p| matches!(*p, "generated" | "target" | "dist" | "build"))
    {
        return Some("generated");
    }
    if !opts.include_fixtures && parts.contains(&"fixtures") {
        return Some("fixtures");
    }
    if opts.no_tests && crate::query::is_test_file(file) {
        return Some("tests");
    }
    None
}
fn text_excerpt(source: &str, start: usize, end: usize) -> (String, (usize, usize)) {
    let line_start = source[..start].rfind('\n').map_or(0, |p| p + 1);
    let line_end = source[end..].find('\n').map_or(source.len(), |p| end + p);
    let mut a = line_start.max(start.saturating_sub(70));
    let mut b = line_end.min(end.saturating_add(70));
    while !source.is_char_boundary(a) {
        a += 1;
    }
    while !source.is_char_boundary(b) {
        b -= 1;
    }
    (source[a..b].into(), (a, b))
}
fn body(source: &str, range: (usize, usize)) -> Body {
    let mut end = range.1.min(range.0.saturating_add(2048));
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    let text = &source[range.0..end];
    if let Some((n, _)) = text.match_indices('\n').nth(39) {
        end = range.0 + n + 1;
    }
    Body {
        text: source[range.0..end].into(),
        byte_range: (range.0, end),
        full_byte_range: range,
        truncated: end < range.1,
    }
}

pub fn run(index: &Index, opts: &Options) -> Report<Row> {
    if opts.query.trim().is_empty() {
        return Report::failure(Failure::new(
            ErrorCode::InvalidInput,
            "context query cannot be empty",
        ));
    }
    let query = opts.query.trim();
    let mut query_terms = terms(query);
    query_terms.retain(|t| {
        !matches!(
            t.as_str(),
            "a" | "an" | "the" | "of" | "to" | "in" | "and" | "or" | "for" | "with"
        )
    });
    let mut excluded_counts = BTreeMap::<&str, usize>::new();
    let mut exact_files = BTreeSet::new();
    for (file, data) in &index.entries {
        if let Some(reason) = excluded(file, opts) {
            *excluded_counts.entry(reason).or_default() += 1;
            continue;
        }
        let path = file.to_string_lossy().replace('\\', "/");
        if query == path {
            exact_files.insert(file.clone());
        }
        for symbol in &data.symbols {
            if opts.no_tests && symbol.is_test {
                continue;
            }
            let name_terms = terms(&format!(
                "{} {} {}",
                symbol.name,
                symbol.qualified_name.as_deref().unwrap_or(""),
                path
            ));
            if query == symbol.name
                || symbol.qualified_name.as_deref() == Some(query)
                || (!query_terms.is_empty() && query_terms.is_subset(&name_terms))
            {
                exact_files.insert(file.clone());
            }
        }
    }
    let metadata_fast_path = !exact_files.is_empty();
    let sources = match if metadata_fast_path {
        Sources::capture_subset(index, &exact_files)
    } else {
        Sources::capture(index)
    } {
        Ok(s) => s,
        Err(e) => return Report::failure(e),
    };
    if let Err(e) = sources.check(opts.snapshot.as_deref()) {
        return Report::failure(e);
    }
    let candidate_universe_verified =
        index.freshness.mode == "verified" || index.freshness.mode == "snapshot";
    let mut rows = Vec::new();
    let mut partial_classification = 0usize;
    let mut omitted_sources = 0usize;
    for (file, bytes) in &sources.files {
        if excluded(file, opts).is_some() {
            continue;
        }
        let Ok(source) = std::str::from_utf8(bytes) else {
            partial_classification += 1;
            omitted_sources += 1;
            continue;
        };
        let data = &index.entries[file];
        let path = file.to_string_lossy().replace('\\', "/");
        let mut candidates = BTreeMap::<(usize, usize), Candidate>::new();
        let metadata = |text: &str| -> Vec<String> {
            terms(text).intersection(&query_terms).cloned().collect()
        };
        for symbol in &data.symbols {
            if opts.no_tests && symbol.is_test {
                continue;
            }
            let range = symbol.byte_range;
            if range.1 > source.len()
                || !source.is_char_boundary(range.0)
                || !source.is_char_boundary(range.1)
            {
                continue;
            }
            let mut c = Candidate {
                symbol: Some(symbol.clone()),
                range,
                score: 0,
                terms: BTreeSet::new(),
                matches: Vec::new(),
            };
            for (field, text, weight) in [
                ("symbol_name", symbol.name.as_str(), 30),
                (
                    "qualified_name",
                    symbol.qualified_name.as_deref().unwrap_or(""),
                    20,
                ),
                ("path", path.as_str(), 15),
                ("signature", symbol.signature.as_str(), 5),
            ] {
                let hits = metadata(text);
                if hits.is_empty() {
                    continue;
                }
                c.score += weight * hits.len();
                c.terms.extend(hits.clone());
                c.matches.push(Match {
                    field,
                    terms: hits,
                    line: sources.line(file, range.0),
                    byte_range: None,
                    text: text.chars().take(200).collect(),
                    text_truncated: text.chars().count() > 200,
                    text_range: None,
                });
            }
            if query == symbol.name || symbol.qualified_name.as_deref() == Some(query) {
                c.score += 10000;
            }
            if c.score > 0 {
                candidates.insert(range, c);
            }
        }
        let text_hits: Vec<_> = if metadata_fast_path {
            Vec::new()
        } else {
            words(source)
                .into_iter()
                .filter(|(w, _, _)| query_terms.contains(w))
                .collect()
        };
        let (regions, parsed) = if text_hits.is_empty() {
            (vec![], true)
        } else {
            match crate::language::text_regions(&data.meta.language, bytes, file) {
                Ok((r, partial)) => {
                    if partial {
                        partial_classification += 1;
                    }
                    (r, !partial)
                }
                Err(_) => {
                    partial_classification += 1;
                    (vec![], false)
                }
            }
        };
        for (term, start, end) in text_hits {
            if opts.no_tests
                && data
                    .symbols
                    .iter()
                    .any(|s| s.is_test && s.byte_range.0 <= start && end <= s.byte_range.1)
            {
                continue;
            }
            let owner = data
                .symbols
                .iter()
                .filter(|s| {
                    s.byte_range.0 <= start
                        && end <= s.byte_range.1
                        && (!opts.no_tests || !s.is_test)
                })
                .min_by_key(|s| s.byte_range.1 - s.byte_range.0);
            // File-level text is not forcibly attributed to the nearest function.
            let range = owner.map_or((0, source.len()), |s| s.byte_range);
            let c = candidates.entry(range).or_insert_with(|| Candidate {
                symbol: owner.cloned(),
                range,
                score: 0,
                terms: BTreeSet::new(),
                matches: Vec::new(),
            });
            if c.terms.insert(term.clone()) {
                c.score += 2;
            }
            let field = regions
                .iter()
                .find(|(a, b, _)| *a <= start && end <= *b)
                .map_or(
                    if parsed { "code_text" } else { "source_text" },
                    |(_, _, f)| *f,
                );
            let (text, text_range) = text_excerpt(source, start, end);
            c.matches.push(Match {
                field,
                terms: vec![term],
                line: sources.line(file, start),
                byte_range: Some((start, end)),
                text,
                text_range: Some(text_range),
                text_truncated: text_range.0 > source[..start].rfind('\n').map_or(0, |p| p + 1)
                    || text_range.1 < source[end..].find('\n').map_or(source.len(), |p| end + p),
            });
        }
        if query == path {
            let c = candidates
                .entry((0, source.len()))
                .or_insert_with(|| Candidate {
                    symbol: None,
                    range: (0, source.len()),
                    score: 0,
                    terms: BTreeSet::new(),
                    matches: Vec::new(),
                });
            c.score += 20000;
            c.terms.extend(query_terms.clone());
            c.matches.push(Match {
                field: "path",
                terms: query_terms.iter().cloned().collect(),
                line: 1,
                byte_range: None,
                text: path.clone(),
                text_range: None,
                text_truncated: false,
            });
        }
        for (_, mut c) in candidates {
            if c.score == 0 {
                continue;
            }
            c.score += c.terms.len() * 3;
            let (name, qualified, kind, role, signature) = if let Some(s) = &c.symbol {
                (
                    s.name.clone(),
                    s.qualified_name.clone(),
                    s.kind.as_str().to_string(),
                    s.role.as_str().to_string(),
                    s.signature.clone(),
                )
            } else {
                (
                    path.clone(),
                    None,
                    "file".into(),
                    "file".into(),
                    String::new(),
                )
            };
            let mut next = if c.symbol.is_some() {
                format!(
                    "cx definition --name {} --from {} --role {}",
                    shell_quote(&name),
                    shell_quote(&path),
                    shell_quote(&role)
                )
            } else {
                format!("cx overview {}", shell_quote(&path))
            };
            next.push_str(&format!(
                " --root {}",
                shell_quote(&index.root.to_string_lossy())
            ));
            let filtered_body = opts.include_body
                && opts.no_tests
                && data
                    .symbols
                    .iter()
                    .any(|s| s.is_test && c.range.0 < s.byte_range.1 && s.byte_range.0 < c.range.1);
            let total = c.matches.len();
            c.matches.truncate(4);
            rows.push(Row {
                file: file.clone(),
                name,
                qualified,
                kind,
                role,
                line: sources.line(file, c.range.0),
                byte_range: c.range,
                score: c.score,
                matched_terms: c.terms.into_iter().collect(),
                matches: c.matches,
                matches_total: total,
                matches_omitted: total.saturating_sub(4),
                signature: signature.chars().take(300).collect(),
                signature_truncated: signature.chars().count() > 300,
                source_hash: format!("{:016x}", crate::index::content_hash(bytes)),
                body: (opts.include_body && !filtered_body).then(|| body(source, c.range)),
                body_omitted_for_filter: filtered_body,
                next_queries: vec![next],
            });
        }
    }
    rows.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.file.cmp(&b.file))
            .then(a.byte_range.cmp(&b.byte_range))
    });
    // Bodies can overlap (e.g. exact class + method matches); don't pay twice.
    let mut packed = BTreeMap::<PathBuf, Vec<(usize, usize)>>::new();
    let mut overlap_omitted = 0;
    for row in &mut rows {
        if let Some(body) = &row.body {
            let ranges = packed.entry(row.file.clone()).or_default();
            if ranges
                .iter()
                .any(|(a, b)| *a < body.byte_range.1 && body.byte_range.0 < *b)
            {
                row.body = None;
                overlap_omitted += 1;
            } else {
                ranges.push(body.byte_range);
            }
        }
    }
    Report {
        rows,
        analysis: json!({"snapshot":sources.id,"model":"lexical_fields_subwords_v1","metadata_fast_path":metadata_fast_path,"candidate_universe_verified":candidate_universe_verified,"excluded_files":excluded_counts,"partial_text_classification_files":partial_classification,"omitted_non_utf8_sources":omitted_sources,"files_skipped_missing_grammar":index.freshness.files_skipped_missing_grammar,"overlapping_bodies_omitted":overlap_omitted,"query_terms":query_terms,"limitations":"Lexical retrieval only, not semantic/cross-language search or call evidence. Body/comment/string hits remain text. The metadata fast path validates selected evidence files; under metadata freshness it discloses that unseen candidate files were not content-verified. No popularity filler or automatic graph expansion."}),
        complete: omitted_sources == 0
            && index.freshness.files_skipped_missing_grammar == 0
            && (!metadata_fast_path || candidate_universe_verified),
        warnings: if metadata_fast_path && !candidate_universe_verified {
            vec!["Fast exact/subword route validated selected evidence files, but metadata freshness did not content-verify the full candidate universe; use --fresh verified for complete ranking coverage".into()]
        } else if partial_classification > 0 || index.freshness.files_skipped_missing_grammar > 0 {
            vec!["Some text provenance could not be fully classified; generic source_text is not call evidence".into()]
        } else {
            vec![]
        },
        error: None,
    }
}

pub fn compact(report: Report<Row>) -> Report<serde_json::Value> {
    let rows=report.rows.into_iter().map(|row| {
        let matches=row.matches.into_iter().take(2).map(|m|json!({"field":m.field,"terms":m.terms,"line":m.line,
            "byte_range":m.byte_range})).collect::<Vec<_>>();
        json!({"file":crate::output::ProtocolPath(&row.file),"name":row.name,"qualified":row.qualified,"kind":row.kind,"role":row.role,"line":row.line,
            "byte_range":row.byte_range,"score":row.score,"matched_terms":row.matched_terms,"matches":matches,
            "matches_total":row.matches_total,"matches_omitted":row.matches_omitted,"source_hash":row.source_hash,
            "body":row.body,"body_omitted_for_filter":row.body_omitted_for_filter,"next_queries":row.next_queries})
    }).collect();
    Report {
        rows,
        analysis: {
            let mut a = report.analysis;
            a["detail"] = json!("compact");
            a["detail_omitted"] = json!(["match_excerpt_text", "signatures"]);
            a["limitations"] = json!([
                "lexical_only",
                "text_not_call_evidence",
                "no_popularity_filler",
                "no_graph_expansion"
            ]);
            a
        },
        complete: report.complete,
        warnings: report.warnings,
        error: report.error,
    }
}
