//! Independent multi-hop oracle: source literals, call lines and graph sets are
//! authored here, not calculated with cx's resolver or BFS.
mod support;
use serde_json::json;
use std::fs;
use support::run_cx;

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let p = tempfile::tempdir().unwrap();
    fs::create_dir(p.path().join(".git")).unwrap();
    for (file, source) in files {
        fs::write(p.path().join(file), source).unwrap();
    }
    p
}
fn impact(p: &tempfile::TempDir, extra: &[&str]) -> support::Run {
    let mut args = vec![
        "--json", "impact", "--name", "leaf", "--fresh", "verified", "--detail", "full",
    ];
    args.extend_from_slice(extra);
    run_cx(p.path(), &args)
}
fn impact_metadata(p: &tempfile::TempDir, extra: &[&str]) -> support::Run {
    let mut args = vec![
        "--json", "impact", "--name", "leaf", "--fresh", "metadata", "--detail", "full",
    ];
    args.extend_from_slice(extra);
    run_cx(p.path(), &args)
}
fn rows(out: &support::Run) -> Vec<(String, usize)> {
    assert_eq!(out.code, 0, "{} {}", out.stdout, out.stderr);
    out.results()
        .iter()
        .map(|r| {
            (
                r["symbol"]["name"].as_str().unwrap().into(),
                r["depth"].as_u64().unwrap() as usize,
            )
        })
        .collect()
}

#[test]
fn reverse_chain_diamond_and_witnesses_are_exact() {
    let p = project(&[(
        "core.rs",
        "fn leaf() {}\nfn left() { leaf(); }\nfn right() { leaf(); }\nfn entry() { left(); right(); }\nfn unrelated() {}\n",
    )]);
    let out = impact(&p, &["--max-depth", "3", "--all"]);
    assert_eq!(
        rows(&out),
        vec![("left".into(), 1), ("right".into(), 1), ("entry".into(), 2)]
    );
    let doc = out.json();
    assert_eq!(doc["analysis"]["root"]["name"], "leaf");
    assert_eq!(doc["page"]["total"], 3);
    assert_eq!(doc["analysis"]["traversal_complete"], true);
    assert_eq!(doc["analysis"]["discovered_count"], 3);
    assert_eq!(out.results()[2]["supported"]["depth"], 2);
    assert_eq!(
        out.results()[2]["supported"]["path"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (
                e["caller_name"].as_str().unwrap(),
                e["callee_name"].as_str().unwrap(),
                e["line"].as_u64().unwrap()
            ))
            .collect::<Vec<_>>(),
        vec![("entry", "left", 4), ("left", "leaf", 2)]
    );
    assert!(out.results().iter().all(|r| r["possible"].is_null()));
}

#[test]
fn recursion_terminates_and_seed_is_never_counted() {
    let p = project(&[(
        "cycle.rs",
        "fn leaf() { leaf(); middle(); }\nfn middle() { leaf(); }\nfn entry() { middle(); }\n",
    )]);
    let out = impact(&p, &["--max-depth", "8", "--all"]);
    assert_eq!(rows(&out), vec![("middle".into(), 1), ("entry".into(), 2)]);
}

#[test]
fn same_named_sites_do_not_cross_file_boundaries_and_root_can_be_selected() {
    let p = project(&[
        ("a.rs", "fn leaf() {}\nfn a() { leaf(); }\n"),
        ("b.rs", "fn leaf() {}\nfn b() { leaf(); }\n"),
    ]);
    let ambiguous = impact(&p, &[]);
    assert_eq!(ambiguous.code, 1);
    assert_eq!(ambiguous.error_code().as_deref(), Some("subject_ambiguous"));
    assert_eq!(
        ambiguous.json()["analysis"]["root_candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let out = impact(&p, &["--file", "a.rs", "--all"]);
    assert_eq!(rows(&out), vec![("a".into(), 1)]);
    assert_eq!(out.results()[0]["symbol"]["id"]["file"], "a.rs");
}

#[test]
fn overloads_require_a_site_selector_and_ambiguous_edges_do_not_propagate() {
    let p = project(&[(
        "a.cpp",
        "void leaf(int n) {}\nvoid leaf(double n) {}\nvoid entry() { leaf(1); }\nvoid ancestor() { entry(); }\n",
    )]);
    assert_eq!(
        impact(&p, &["--file", "a.cpp"]).error_code().as_deref(),
        Some("subject_ambiguous")
    );
    let out = impact(&p, &["--file", "a.cpp", "--line", "1", "--all"]);
    assert_eq!(rows(&out), vec![]);
    assert_eq!(out.json()["analysis"]["frontier_count"], 1);
    assert_eq!(
        out.json()["analysis"]["frontier"][0]["reason"],
        "unresolved_target"
    );
    assert_eq!(
        out.json()["analysis"]["frontier"][0]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn syntax_only_paths_are_possible_not_supported() {
    let p = project(&[
        ("a.rs", "fn leaf() {}\n"),
        (
            "b.rs",
            "fn caller() { leaf(); }\nfn entry() { caller(); }\n",
        ),
    ]);
    let out = impact(&p, &["--all"]);
    assert_eq!(rows(&out), vec![("caller".into(), 1), ("entry".into(), 2)]);
    assert!(out.results().iter().all(|r| r["supported"].is_null()));
    assert_eq!(
        out.results()[1]["possible"]["path"][1]["resolution"],
        "syntax"
    );
}

#[test]
fn unknown_receiver_is_a_frontier_not_an_impact_path() {
    let p = project(&[(
        "a.rs",
        "fn leaf() {}\nfn caller(x: Unknown) { x.leaf(); }\nfn entry() { caller(x); }\n",
    )]);
    let out = impact(&p, &["--all"]);
    assert_eq!(rows(&out), vec![]);
    assert_eq!(out.json()["analysis"]["frontier_count"], 1);
    assert_eq!(out.json()["analysis"]["frontier"][0]["line"], 2);
}

#[test]
fn traversal_and_output_budgets_are_distinct() {
    let p = project(&[(
        "a.rs",
        "fn leaf() {}\nfn middle() { leaf(); }\nfn entry() { middle(); }\n",
    )]);
    let depth = impact(&p, &["--max-depth", "1", "--all"]);
    assert_eq!(rows(&depth), vec![("middle".into(), 1)]);
    assert!(depth.json()["page"]["total"].is_null());
    assert_eq!(depth.json()["page"]["truncated"], false);
    assert!(
        depth.json()["analysis"]["stop_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("max_depth"))
    );
    let nodes = impact(&p, &["--max-nodes", "1", "--all"]);
    assert_eq!(rows(&nodes), vec![]);
    assert!(
        nodes.json()["analysis"]["stop_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("max_nodes"))
    );
    let edges = impact(&p, &["--max-edges", "1", "--all"]);
    assert_eq!(rows(&edges), vec![("middle".into(), 1)]);
    assert!(
        edges.json()["analysis"]["stop_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("max_edges"))
    );
    let page = impact(&p, &["--limit", "1"]);
    assert_eq!(page.page()["total"], 2);
    assert_eq!(page.page()["truncated"], true);
    let snapshot = page.json()["analysis"]["snapshot"]
        .as_str()
        .unwrap()
        .to_owned();
    let next = impact(
        &p,
        &["--limit", "1", "--offset", "1", "--snapshot", &snapshot],
    );
    assert_eq!(rows(&next), vec![("entry".into(), 2)]);
    assert_eq!(next.json()["analysis"]["snapshot"], snapshot);
    fs::write(p.path().join("new.rs"), "fn extra() { leaf(); }\n").unwrap();
    assert_eq!(
        impact(&p, &["--snapshot", &snapshot])
            .error_code()
            .as_deref(),
        Some("snapshot_mismatch")
    );
}

#[test]
fn absent_unsupported_and_invalid_subjects_are_not_empty_success() {
    let p = project(&[("a.py", "def leaf():\n    pass\n")]);
    assert_eq!(
        impact(&p, &[]).error_code().as_deref(),
        Some("unsupported_analysis")
    );
    let out = run_cx(p.path(), &["--json", "impact", "--name", "absent"]);
    assert_eq!(out.error_code().as_deref(), Some("subject_not_found"));
    assert_eq!(impact(&p, &["--max-depth", "33"]).code, 2);
    assert_eq!(impact(&p, &["--max-nodes", "0"]).code, 2);
}

#[test]
fn short_possible_path_does_not_suppress_longer_supported_path() {
    let p = project(&[
        (
            "a.cpp",
            "namespace api { void leaf() {} void middle() { leaf(); } }\n",
        ),
        (
            "b.cpp",
            "void entry() { leaf(); api::middle(); }\nvoid top() { entry(); }\n",
        ),
    ]);
    let out = impact(&p, &["--max-depth", "4", "--all"]);
    assert_eq!(
        rows(&out),
        vec![("middle".into(), 1), ("entry".into(), 1), ("top".into(), 2)]
    );
    let entry = &out.results()[1];
    assert_eq!(entry["supported"]["depth"], 2);
    assert_eq!(entry["possible"]["depth"], 1);
    let top = &out.results()[2];
    assert_eq!(top["supported"]["depth"], 3);
    assert_eq!(top["possible"]["depth"], 2);
}

#[test]
fn anonymous_caller_is_not_relabelled_as_its_outer_function() {
    let p = project(&[("a.rs", "fn leaf() {}\nfn outer() { let c = || leaf(); }\n")]);
    let out = impact(&p, &["--all"]);
    assert_eq!(out.code, 0);
    assert_eq!(out.json_len(), 1);
    assert_eq!(out.results()[0]["symbol"]["id"]["kind"], "anonymous");
    assert_ne!(out.results()[0]["symbol"]["name"], "outer");
    assert!(
        out.json()["analysis"]["stop_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("unmodelled_owner_bindings"))
    );
}

#[test]
fn snapshot_mismatch_and_scope_escape_are_structured_failures() {
    let p = project(&[("a.rs", "fn leaf() {}\nfn entry() { leaf(); }\n")]);
    let first = impact(&p, &[]);
    assert_eq!(first.code, 0);
    let file = p.path().join("a.rs");
    let time = fs::metadata(&file).unwrap().modified().unwrap();
    fs::write(&file, "fn leaf() {}\nfn other() { leaf(); }\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(time))
        .unwrap();
    assert_eq!(
        impact_metadata(&p, &[]).error_code().as_deref(),
        Some("content_changed")
    );
    assert_eq!(
        impact_metadata(&p, &["--file", "../escape.rs"])
            .error_code()
            .as_deref(),
        Some("invalid_input")
    );
    let clean = project(&[("a.rs", "fn leaf() {}\n")]);
    assert_eq!(
        impact(&clean, &["--file", "../escape.rs"])
            .error_code()
            .as_deref(),
        Some("invalid_input")
    );
}

#[test]
fn mutual_cycle_away_from_seed_keeps_minimum_distances() {
    let p = project(&[(
        "a.rs",
        "fn leaf() {}\nfn a() { leaf(); b(); }\nfn b() { a(); }\nfn entry() { b(); }\n",
    )]);
    assert_eq!(
        rows(&impact(&p, &["--max-depth", "8", "--all"])),
        vec![("a".into(), 1), ("b".into(), 2), ("entry".into(), 3)]
    );
}

#[test]
fn compact_default_keeps_primary_witness_without_duplicate_raw_paths() {
    let p = project(&[(
        "a.rs",
        "fn leaf() {}\nfn middle() { leaf(); }\nfn entry() { middle(); }\n",
    )]);
    let out = run_cx(
        p.path(),
        &[
            "--json", "impact", "--name", "leaf", "--fresh", "verified", "--all",
        ],
    );
    assert_eq!(out.code, 0);
    assert_eq!(out.json_len(), 2);
    assert_eq!(out.results()[1]["evidence"], "supported");
    assert_eq!(out.results()[1]["witness"].as_array().unwrap().len(), 2);
    assert!(out.results()[1].get("supported").is_none());
    assert!(out.results()[1].get("possible").is_none());
}

#[test]
fn cached_multiline_declaration_links_imported_call_to_definition() {
    let p = project(&[
        ("api.hpp", "void leaf(int n,\n double value);\n"),
        (
            "api.cpp",
            "#include \"api.hpp\"\nvoid leaf(int n,\n double value) {}\n",
        ),
        (
            "main.cpp",
            "#include \"api.hpp\"\nvoid entry() { leaf(1,2.0); }\n",
        ),
    ]);
    let out = impact(&p, &["--file", "api.cpp", "--all"]);
    assert_eq!(rows(&out), vec![("entry".into(), 1)]);
    assert_eq!(
        out.results()[0]["supported"]["path"][0]["resolution"],
        "import_resolved"
    );
}
