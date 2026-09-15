//! Coverage controls frozen after the first fixed-corpus diagnostic, before
//! bounded/lazy parsing implementation. No corpus-derived oracle is generated.
mod support;
use serde_json::{Value, json};
use std::fs;
use support::{cx_in, run_cx};
fn coverage(doc: &Value) -> Value {
    let record = doc["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .find_map(|w| w.strip_prefix("relation_coverage: "));
    assert!(
        record.is_some(),
        "mandatory relation_coverage disclosure missing"
    );
    serde_json::from_str(record.unwrap()).unwrap()
}
#[test]
fn valid_call_outside_parse_error_is_retained_as_partial_syntax() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(
        dir.path().join("partial.rs"),
        "fn leaf() {}\nfn entry() { leaf(); }\nfn broken(\n",
    )
    .unwrap();
    let out = run_cx(
        dir.path(),
        &["--json", "callers", "--name", "leaf", "--all"],
    );
    assert_eq!(out.code, 0);
    assert_eq!(out.json_len(), 1);
    assert_eq!(out.results()[0]["from"], "entry");
    assert_eq!(out.results()[0]["line"], 2);
    assert_eq!(out.results()[0]["to"], "");
    assert_eq!(
        coverage(&out.json())["issues"],
        json!([{"file":"partial.rs","reason":"parse_error"}])
    );
    assert_eq!(coverage(&out.json())["complete_within_model"], false);
}
#[test]
fn coverage_schema_and_concurrent_snapshot_results_are_stable() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(
        dir.path().join("a.rs"),
        "fn leaf() {}\nfn entry() { leaf(); }\n",
    )
    .unwrap();
    let args = ["--json", "callers", "--name", "leaf", "--all"];
    let out = run_cx(dir.path(), &args);
    assert_eq!(out.code, 0);
    let report = coverage(&out.json());
    let mut keys: Vec<_> = report
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "complete_within_model",
            "files_analyzed",
            "files_checked",
            "files_skipped_missing_grammar",
            "generation",
            "issue_counts",
            "issues",
            "issues_omitted",
            "issues_total",
            "limitations",
            "model",
            "scope",
            "snapshot_id"
        ]
    );
    assert_eq!(report["model"], "direct_syntax_v2");
    assert_eq!(report["scope"], "indexed_candidates; callers_matching_name");
    assert_eq!(report["files_analyzed"], 1);
    assert_eq!(report["files_checked"], 1);
    assert_eq!(report["issues_total"], 0);
    assert_eq!(report["complete_within_model"], true);
    let id = report["snapshot_id"].as_str().unwrap();
    assert_eq!(id.len(), 16);
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    std::thread::scope(|scope| {
        let readers: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| run_cx(dir.path(), &args)))
            .collect();
        for reader in readers {
            let warm = reader.join().unwrap();
            assert_eq!(warm.code, 0);
            assert_eq!(warm.results(), out.results());
            assert_eq!(coverage(&warm.json()), report);
            assert_eq!(warm.json()["freshness"]["files_updated"], 0);
        }
    });
}

#[test]
fn coverage_output_is_bounded_without_hiding_omitted_issue_count() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    for i in 0..100 {
        fs::write(dir.path().join(format!("notes{i:03}.md")), "# leaf\n").unwrap();
    }
    let out = run_cx(dir.path(), &["--json", "callers", "--name", "leaf"]);
    assert_eq!(out.code, 0);
    let report = coverage(&out.json());
    assert_eq!(report["issues_total"], 100);
    assert_eq!(report["issues_omitted"], 84);
    assert_eq!(report["issue_counts"], json!({"unsupported_language":100}));
    assert_eq!(report["issues"].as_array().unwrap().len(), 16);
    assert_eq!(report["complete_within_model"], false);
    assert_eq!(out.json_len(), 0);
    assert!(out.stdout.len() < 8192, "{} bytes", out.stdout.len());
}
#[test]
fn irreducible_row_retains_all_candidates_and_discloses_overflow() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    let mut source = (0..600)
        .map(|i| format!("void run(T{i} x) {{}}\n"))
        .collect::<String>();
    source.push_str("void entry() { run(1); }\n");
    fs::write(dir.path().join("large.cpp"), source).unwrap();
    let out = run_cx(dir.path(), &["--json", "callees", "--name", "entry"]);
    assert_eq!(out.code, 0);
    assert_eq!(out.json_len(), 1);
    assert_eq!(out.page()["total"], 1);
    assert_eq!(out.page()["truncated"], false);
    assert!(out.stdout.len() > 16384);
    assert_eq!(
        out.results()[0]["ambiguous_candidates"]
            .as_str()
            .unwrap()
            .split(", ")
            .count(),
        600
    );
    assert!(out.json()["warnings"].as_array().unwrap().iter().any(|w| {
        w.as_str()
            .unwrap()
            .contains("irreducible row or metadata exceeds budget")
    }));
}

#[test]
fn large_candidate_evidence_is_paginated_not_deleted() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    let mut source = (0..40)
        .map(|i| format!("void run(T{i} x) {{}}\n"))
        .collect::<String>();
    source.push_str("void entry() {\n");
    for _ in 0..20 {
        source.push_str(" run(1);\n");
    }
    source.push_str("}\n");
    fs::write(dir.path().join("large.cpp"), source).unwrap();
    let all = run_cx(
        dir.path(),
        &["--json", "callees", "--name", "entry", "--all"],
    );
    // All forty overload identities must survive byte-budget pagination.
    assert_eq!(all.code, 0);
    assert_eq!(all.json_len(), 20);
    assert!(all.stdout.len() > 16384); // --all intentionally disables the page cap
    let mut page = run_cx(dir.path(), &["--json", "callees", "--name", "entry"]);
    let mut collected = Vec::new();
    loop {
        assert_eq!(page.code, 0);
        assert!(
            page.stdout.len() <= 16384,
            "output budget assertion: {} bytes",
            page.stdout.len()
        );
        assert_eq!(page.page()["total"], 20);
        assert_eq!(page.page()["offset"], collected.len());
        assert!(!page.results().is_empty(), "pagination must make progress");
        collected.extend(page.results());
        if !page.page()["truncated"].as_bool().unwrap() {
            break;
        }
        assert!(
            page.json()["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w.as_str().unwrap().starts_with("relation_output_budget:"))
        );
        let next = page.json()["next_queries"][0].as_str().unwrap().to_string();
        page = run_cx(
            dir.path(),
            &next.split_whitespace().skip(1).collect::<Vec<_>>(),
        );
        assert!(collected.len() < 20);
    }
    assert_eq!(collected, all.results());
}

#[test]
fn missing_installed_grammar_is_disclosed_in_isolated_cli_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join("a.rs"), "fn leaf() {}\n").unwrap();
    let out = cx_in(dir.path())
        .env("CX_CACHE_DIR", cache.path())
        // The language pack also searches build-time bundled libraries.
        .env("TREE_SITTER_LANGUAGE_PACK_LIBS_DIR", cache.path())
        // Disable the dependency's automatic network install using its local
        // manifest override; a nonexistent local manifest fails deterministically.
        .env(
            "TREE_SITTER_LANGUAGE_PACK_MANIFEST_URL",
            format!("file://{}", cache.path().join("missing.json").display()),
        )
        .args(["--json", "callers", "--name", "leaf"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["freshness"]["files_skipped_missing_grammar"], 1);
    assert_eq!(coverage(&doc)["files_skipped_missing_grammar"], 1);
    assert_eq!(coverage(&doc)["complete_within_model"], false);
    assert_eq!(doc["results"], json!([]));
}
