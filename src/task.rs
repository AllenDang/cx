//! Shared wire/output contract for task-level queries, separate from legacy pages.
use crate::index::Freshness;
use crate::output::{
    ErrorCode, ErrorInfo, QueryInfo, SCHEMA_VERSION, command_with_offset, print_toon, shell_quote,
};
use crate::query::Pagination;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug)]
pub struct Failure {
    pub code: ErrorCode,
    pub message: String,
}
impl Failure {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub struct Report<T> {
    pub rows: Vec<T>,
    pub analysis: Value,
    pub complete: bool,
    pub warnings: Vec<String>,
    pub error: Option<Failure>,
}
impl<T> Report<T> {
    pub fn failure(error: Failure) -> Self {
        Self {
            rows: Vec::new(),
            analysis: json!({}),
            complete: false,
            warnings: Vec::new(),
            error: Some(error),
        }
    }
}
#[derive(Serialize)]
struct Page {
    total: Option<usize>,
    offset: usize,
    limit: Option<usize>,
    truncated: bool,
}
#[derive(Serialize)]
struct Envelope<'a, T: Serialize> {
    schema_version: u32,
    query: QueryInfo,
    freshness: Freshness,
    page: Page,
    results: &'a [T],
    warnings: &'a [String],
    next_queries: Vec<String>,
    error: Option<ErrorInfo>,
    analysis: &'a Value,
}

/// A byte budget includes metadata and follow-up commands. Never truncate a
/// JSON string, identity, or witness; use a shorter page or an explicit error.
pub fn emit<T: Serialize>(
    freshness: &Freshness,
    kind: &'static str,
    subject: &str,
    mut report: Report<T>,
    pg: &Pagination,
    as_json: bool,
    byte_budget: usize,
) -> i32 {
    let discovered = report.rows.len();
    report.analysis["discovered_count"] = json!(discovered);
    report.analysis["complete"] = json!(report.complete);
    let snapshot = report.analysis["snapshot"].as_str().map(str::to_owned);
    let rows: Vec<_> = report
        .rows
        .into_iter()
        .skip(pg.offset)
        .take(pg.limit.unwrap_or(usize::MAX))
        .collect();
    let mut count = rows.len();
    let mut budget_note = false;
    loop {
        let truncated = pg.offset.saturating_add(count) < discovered;
        let mut next_queries = Vec::new();
        if truncated && count > 0 {
            let mut command = command_with_offset(pg.offset + count);
            if let Some(id) = &snapshot
                && !std::env::args().any(|a| a == "--snapshot" || a.starts_with("--snapshot="))
            {
                command.push_str(&format!(" --snapshot {}", shell_quote(id)));
            }
            next_queries.push(command);
        }
        let envelope = Envelope {
            schema_version: SCHEMA_VERSION,
            query: QueryInfo::new(kind, Some(subject.into())),
            freshness: freshness.clone(),
            page: Page {
                total: report.complete.then_some(discovered),
                offset: pg.offset,
                limit: pg.limit,
                truncated,
            },
            results: &rows[..count],
            warnings: &report.warnings,
            next_queries,
            error: report.error.as_ref().map(|f| ErrorInfo {
                code: f.code,
                message: f.message.clone(),
            }),
            analysis: &report.analysis,
        };
        let encoded = serde_json::to_string(&envelope).expect("task report is serializable");
        let bytes = encoded.len() + 1;
        if bytes <= byte_budget || byte_budget == usize::MAX {
            if as_json {
                println!("{encoded}");
            } else {
                print_toon(&envelope);
                if truncated {
                    eprintln!(
                        "cx: output page truncated; use next_queries, not a larger traversal budget"
                    );
                }
            }
            return i32::from(report.error.is_some());
        }
        if count <= 1 {
            let failure = json!({"schema_version": SCHEMA_VERSION, "query": {"kind":kind,"subject":subject.chars().take(64).collect::<String>(),"subject_truncated":subject.chars().count()>64},
                "freshness":freshness, "page":{"total":null,"offset":pg.offset,"limit":pg.limit,"truncated":false},
                "results":[],"warnings":["No identities or evidence were silently shortened"],"next_queries":[],
                "error":{"code":ErrorCode::BudgetTooSmall,"message":format!("An indivisible report needs {bytes} bytes; increase --byte-budget")},
                "analysis":{"complete":false,"required_bytes":bytes,"requested_bytes":byte_budget,"discovered_count":discovered,"snapshot":snapshot,"original_error_code":report.error.as_ref().map(|e|e.code)}});
            if as_json {
                println!("{failure}");
            } else {
                print_toon(&failure);
            }
            return 1;
        }
        if !budget_note {
            report.warnings.push("Output page shortened to the byte budget; traversal facts and witness identities are preserved".into());
            budget_note = true;
        }
        count -= 1;
    }
}
