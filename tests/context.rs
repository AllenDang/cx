//! Independent lexical retrieval and byte-packing controls, not ranker-derived gold.
mod support;
use serde_json::Value;
use std::fs;
use support::run_cx;
fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let p = tempfile::tempdir().unwrap();
    fs::create_dir(p.path().join(".git")).unwrap();
    for (file, source) in files {
        let path = p.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    p
}
fn query(p: &tempfile::TempDir, text: &str, extra: &[&str]) -> support::Run {
    let mut args = vec![
        "--json", "context", "--query", text, "--fresh", "verified", "--detail", "full",
    ];
    args.extend_from_slice(extra);
    let out = run_cx(p.path(), &args);
    assert_eq!(out.code, 0, "{} {}", out.stdout, out.stderr);
    out
}
#[test]
fn exact_name_beats_body_noise_and_zero_evidence_stays_empty() {
    let p = project(&[(
        "a.rs",
        "fn invalidateCache() {}\nfn unrelated() { let x = \"invalidateCache invalidateCache\"; }\n",
    )]);
    let out = query(&p, "invalidateCache", &[]);
    assert_eq!(out.results()[0]["name"], "invalidateCache");
    let empty = query(&p, "zebra_needle_absent", &[]);
    assert_eq!(empty.json_len(), 0);
    assert_eq!(empty.page()["total"], 0);
}
#[test]
fn subwords_and_comment_only_evidence_find_the_implementation() {
    let p = project(&[(
        "src/cache.rs",
        "fn invalidateCache() {}\nfn sweep() {\n // eviction expires stale entries\n}\n",
    )]);
    assert_eq!(
        query(&p, "invalidate cache", &[]).results()[0]["name"],
        "invalidateCache"
    );
    let out = query(&p, "eviction", &[]);
    assert_eq!(out.json_len(), 1);
    let row = &out.results()[0];
    assert_eq!(row["name"], "sweep");
    assert!(
        row["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["field"] == "comment_text" && m["line"] == 3)
    );
}
#[test]
fn strings_are_not_call_evidence_and_body_bytes_are_original() {
    let source = "fn render() {\n let text = \"é 中文缓存 notice\";\n}\n";
    let p = project(&[("a.rs", source)]);
    let out = query(&p, "中文缓存", &["--include-body"]);
    assert_eq!(out.json_len(), 1);
    let row = &out.results()[0];
    assert!(
        row["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["field"] == "string_text")
    );
    let range = row["body"]["byte_range"].as_array().unwrap();
    assert_eq!(
        row["body"]["text"],
        &source[range[0].as_u64().unwrap() as usize..range[1].as_u64().unwrap() as usize]
    );
}
#[test]
fn vendor_and_fixture_twins_do_not_pollute_default_results() {
    let p = project(&[
        ("src/real.rs", "fn cache_entry() {}\n"),
        ("vendor/copy.rs", "fn cache_entry() {}\n"),
        ("tests/fixtures/copy.rs", "fn cache_entry() {}\n"),
    ]);
    let out = query(&p, "cache_entry", &[]);
    assert_eq!(out.json_len(), 1);
    assert_eq!(out.results()[0]["file"], "src/real.rs");
    let opted = query(&p, "vendor/copy.rs", &["--include-vendor"]);
    assert_eq!(opted.results()[0]["file"], "vendor/copy.rs");
}
#[test]
fn budget_and_pagination_keep_subject_identity_and_disclosures() {
    let mut source = String::new();
    for n in 0..12 {
        source.push_str(&format!(
            "fn cache_{n}() {{\n // cache behavior {}\n}}\n",
            "x".repeat(300)
        ));
    }
    let p = project(&[("a.rs", &source)]);
    let small = query(
        &p,
        "cache",
        &["--include-body", "--byte-budget", "4096", "--limit", "12"],
    );
    assert!(small.stdout.len() <= 4096);
    assert!(small.page()["truncated"].as_bool().unwrap());
    assert!(!small.results().is_empty());
    assert_eq!(small.page()["total"], 12);
    assert!(small.results()[0]["file"].is_string());
    let large = query(
        &p,
        "cache",
        &["--include-body", "--byte-budget", "32768", "--limit", "12"],
    );
    assert_eq!(large.json_len(), 12);
    assert_eq!(small.results()[0]["name"], large.results()[0]["name"]);
    let snapshot = small.json()["analysis"]["snapshot"]
        .as_str()
        .unwrap()
        .to_string();
    let next = query(
        &p,
        "cache",
        &[
            "--byte-budget",
            "4096",
            "--offset",
            &small.json_len().to_string(),
            "--snapshot",
            &snapshot,
        ],
    );
    assert_eq!(next.page()["offset"], small.json_len());
    assert!(next.json()["error"].is_null());
}
#[test]
fn tests_remain_available_unless_explicitly_filtered() {
    let p = project(&[
        ("src/a.rs", "fn retry_logic() {}\n"),
        ("tests/retry.rs", "fn retry_test() {}\n"),
    ]);
    let all = query(&p, "retry", &[]);
    assert_eq!(all.json_len(), 2);
    let prod = query(&p, "retry", &["--no-tests"]);
    assert_eq!(prod.json_len(), 1);
    assert_eq!(prod.results()[0]["file"], "src/a.rs");
    let _: Value = prod.json();
}

#[test]
fn filtered_test_body_does_not_reappear_as_file_level_text() {
    let p = project(&[(
        "src/a.rs",
        "fn production() {}\n#[test]\nfn check() { let x = \"uniqueneedle\"; }\n",
    )]);
    assert_eq!(query(&p, "uniqueneedle", &["--no-tests"]).json_len(), 0);
}

#[test]
fn exact_name_beats_an_earlier_metadata_subword_twin() {
    let p = project(&[(
        "a.rs",
        "fn invalidateCacheHelper() {}\nfn invalidateCache() {}\n",
    )]);
    assert_eq!(
        query(&p, "invalidateCache", &[]).results()[0]["name"],
        "invalidateCache"
    );
}

#[test]
fn small_budget_errors_are_valid_bounded_json_and_dash_queries_are_data() {
    let p = project(&[("a.rs", "fn present() {}\n")]);
    let huge = "界".repeat(2000);
    let out = run_cx(
        p.path(),
        &[
            "--json",
            "context",
            "--query",
            &huge,
            "--byte-budget",
            "1024",
        ],
    );
    assert_eq!(out.code, 1);
    assert_eq!(out.error_code().as_deref(), Some("budget_too_small"));
    assert!(out.stdout.len() <= 1024);
    let dash = query(&p, "--version", &[]);
    assert_eq!(dash.json()["query"]["subject"], "--version");
    assert_eq!(
        run_cx(
            p.path(),
            &["--json", "context", "--query", "present", "--limit", "0"]
        )
        .code,
        2
    );
}

#[test]
fn compact_default_keeps_match_provenance_without_repeating_excerpt_text() {
    let p = project(&[("a.rs", "fn sweep() { // eviction cache\n}\n")]);
    let out = run_cx(
        p.path(),
        &[
            "--json", "context", "--query", "eviction", "--fresh", "verified",
        ],
    );
    assert_eq!(out.code, 0);
    assert_eq!(out.results()[0]["matches"][0]["field"], "comment_text");
    assert!(out.results()[0]["matches"][0].get("text").is_none());
    let full = query(&p, "eviction", &[]);
    assert!(full.results()[0]["matches"][0]["text"].is_string());
}
