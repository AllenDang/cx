use serde::Serialize;
use toon_format::encode_default;

use crate::index::Freshness;

/// Version of the JSON envelope contract (roadmap §6.1).
///
/// Bumped only for a breaking change to the key set or the meaning of a field.
/// Adding a new optional key is additive and does not bump this.
pub const SCHEMA_VERSION: u32 = 1;

/// Encode any serializable value as TOON and print it.
/// Falls back to debug format on encoding error.
pub fn print_toon<T: Serialize>(value: &T) {
    match encode_default(value) {
        Ok(s) => print!("{s}"),
        Err(e) => eprintln!("cx: toon encoding error: {e}"),
    }
}

/// Encode any serializable value as pretty-printed JSON and print it.
pub fn print_json<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("cx: json encoding error: {e}"),
    }
}

/// What the caller asked for, echoed back so a stored result is self-describing.
#[derive(Serialize)]
pub struct QueryInfo {
    /// Command that produced this result: `overview`, `symbols`, `kinds`,
    /// `definition`, or `references`.
    pub kind: &'static str,
    /// The symbol name or path the query was about, when it had one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

impl QueryInfo {
    pub fn new(kind: &'static str, subject: Option<String>) -> Self {
        Self { kind, subject }
    }
}

/// Pagination facts for this page of results.
#[derive(Serialize)]
pub struct PageInfo {
    /// Total matches before pagination.
    pub total: usize,
    /// Number of results skipped.
    pub offset: usize,
    /// Applied limit, `null` when unlimited.
    pub limit: Option<usize>,
    /// True when more results exist after this page.
    pub truncated: bool,
}

/// Machine-readable failure classes (roadmap §6.2).
///
/// A successful query with zero results is **not** an error and never carries
/// one of these codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The path exists and is indexable, but is not in the index.
    FileNotIndexed,
    /// The path has an extension cx has no grammar mapping for.
    UnsupportedFileType,
    /// A directory scope matched no indexed files.
    NoIndexedFiles,
    /// A grammar needed to answer the query is not installed.
    GrammarNotInstalled,
}

#[derive(Serialize)]
pub struct ErrorInfo {
    pub code: ErrorCode,
    pub message: String,
}

/// The single JSON shape every `--json` command returns (roadmap §6.1).
///
/// The key set is fixed: `results`, `warnings` and `next_queries` are always
/// arrays (possibly empty) and `error` is always present (`null` on success), so
/// a consumer never has to branch on whether a key exists.  The root is always
/// an object, whether or not the result set was paginated.
///
/// `freshness` reports which index generation answered the query and how that
/// was established (Phase 4).
#[derive(Serialize)]
pub struct Envelope<'a, T: Serialize> {
    pub schema_version: u32,
    pub query: QueryInfo,
    pub freshness: Freshness,
    pub page: PageInfo,
    pub results: &'a [T],
    pub warnings: Vec<String>,
    /// Exact, runnable follow-up commands (never placeholders).
    pub next_queries: Vec<String>,
    pub error: Option<ErrorInfo>,
}

impl<'a, T: Serialize> Envelope<'a, T> {
    pub fn new(query: QueryInfo, freshness: Freshness, page: PageInfo, results: &'a [T]) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            query,
            freshness,
            page,
            results,
            warnings: Vec::new(),
            next_queries: Vec::new(),
            error: None,
        }
    }

    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }

    pub fn with_next_queries(mut self, next_queries: Vec<String>) -> Self {
        self.next_queries = next_queries;
        self
    }
}

/// Emit a failure envelope.  `results` is empty and `error` is populated, which
/// is what distinguishes this from a successful query that found nothing.
pub fn print_error_json(query: QueryInfo, freshness: Freshness, code: ErrorCode, message: &str) {
    let empty: [u8; 0] = [];
    let mut envelope = Envelope::new(
        query,
        freshness,
        PageInfo {
            total: 0,
            offset: 0,
            limit: None,
            truncated: false,
        },
        &empty,
    );
    envelope.error = Some(ErrorInfo {
        code,
        message: message.to_string(),
    });
    print_json(&envelope);
}

/// Rebuild the current invocation with a different `--offset`, so the suggested
/// follow-up is exactly runnable rather than a hand-written approximation.
///
/// Existing `--offset`/`--all` flags are dropped; everything else (including
/// `--json` and any filters) is preserved.  Arguments containing whitespace or
/// quotes are shell-quoted.
pub fn command_with_offset(next_offset: usize) -> String {
    let mut parts = rewritten_args(true);
    parts.push("--offset".to_string());
    parts.push(next_offset.to_string());
    parts.join(" ")
}

/// Rebuild the current invocation with `--all` in place of any limit/offset.
pub fn command_with_all() -> String {
    let mut parts = rewritten_args(false);
    parts.push("--all".to_string());
    parts.join(" ")
}

/// Current argv with pagination flags removed. `keep_limit` retains `--limit N`
/// (needed for a next-page command, meaningless alongside `--all`).
fn rewritten_args(keep_limit: bool) -> Vec<String> {
    let raw: Vec<String> = std::env::args().collect();
    let mut out: Vec<String> = Vec::new();
    let mut skip_value = false;

    for (i, arg) in raw.iter().enumerate() {
        if i == 0 {
            // Report the tool by name, not by the absolute test-binary path.
            out.push("cx".to_string());
            continue;
        }
        if skip_value {
            skip_value = false;
            continue;
        }
        match arg.as_str() {
            "--offset" => {
                skip_value = true;
                continue;
            }
            "--limit" if !keep_limit => {
                skip_value = true;
                continue;
            }
            "--all" => continue,
            _ => {}
        }
        if arg.starts_with("--offset=") || (!keep_limit && arg.starts_with("--limit=")) {
            continue;
        }
        out.push(shell_quote(arg));
    }
    out
}

fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    let needs_quotes = arg.chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '\'' | '"'
                    | '*'
                    | '?'
                    | '$'
                    | '`'
                    | '\\'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '|'
                    | '&'
                    | ';'
                    | '<'
                    | '>'
                    | '#'
                    | '~'
            )
    });
    if !needs_quotes {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Sym {
        name: String,
        kind: String,
    }

    #[test]
    fn test_toon_encode_array() {
        let syms = vec![
            Sym {
                name: "foo".into(),
                kind: "fn".into(),
            },
            Sym {
                name: "Bar".into(),
                kind: "struct".into(),
            },
        ];
        let result = encode_default(&syms).unwrap();
        // Should produce tabular format
        assert!(result.contains("name"), "should have field names: {result}");
        assert!(result.contains("foo"), "should have value: {result}");
        assert!(result.contains("Bar"), "should have value: {result}");
    }

    #[test]
    fn test_toon_encode_object() {
        use std::collections::BTreeMap;
        let mut obj = BTreeMap::new();
        obj.insert("file", "src/main.rs");
        obj.insert("status", "unchanged");
        let result = encode_default(&obj).unwrap();
        assert!(result.contains("file"), "{result}");
        assert!(result.contains("src/main.rs"), "{result}");
    }

    #[test]
    fn envelope_keys_are_fixed_on_success() {
        let rows = vec![Sym {
            name: "foo".into(),
            kind: "fn".into(),
        }];
        let env = Envelope::new(
            QueryInfo::new("symbols", Some("foo".into())),
            Freshness::empty(crate::index::FreshnessMode::Metadata),
            PageInfo {
                total: 1,
                offset: 0,
                limit: None,
                truncated: false,
            },
            &rows,
        );
        let value: serde_json::Value = serde_json::to_value(&env).unwrap();
        let obj = value.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "error",
                "freshness",
                "next_queries",
                "page",
                "query",
                "results",
                "schema_version",
                "warnings"
            ]
        );
        assert_eq!(obj["schema_version"], 1);
        assert!(obj["error"].is_null());
        assert!(obj["warnings"].as_array().unwrap().is_empty());
        assert_eq!(obj["freshness"]["mode"], "metadata");
    }

    #[test]
    fn page_info_reports_null_limit_when_unlimited() {
        let empty: [u8; 0] = [];
        let env = Envelope::new(
            QueryInfo::new("symbols", None),
            Freshness::empty(crate::index::FreshnessMode::Verified),
            PageInfo {
                total: 0,
                offset: 0,
                limit: None,
                truncated: false,
            },
            &empty,
        );
        let value = serde_json::to_value(&env).unwrap();
        assert!(value["page"]["limit"].is_null());
        assert_eq!(value["page"]["truncated"], false);
        assert!(value["results"].as_array().unwrap().is_empty());
        assert!(
            value["query"].get("subject").is_none(),
            "absent subject is omitted"
        );
        assert_eq!(value["freshness"]["mode"], "verified");
    }

    #[test]
    fn shell_quote_leaves_plain_arguments_alone() {
        assert_eq!(shell_quote("symbols"), "symbols");
        assert_eq!(shell_quote("src/main.rs"), "src/main.rs");
        assert_eq!(shell_quote("*init*"), "'*init*'");
        assert_eq!(shell_quote("two words"), "'two words'");
        assert_eq!(shell_quote(""), "''");
    }
}
