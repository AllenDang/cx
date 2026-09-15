//! Stage R independent oracle: source literals and expected rows below are frozen
//! before implementation. Expectations are from reading these programs, never
//! from the production resolver. All edits occur in disposable temp projects.
mod support;
use serde_json::{Value, json};
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
fn query(p: &tempfile::TempDir, command: &str, name: &str) -> support::Run {
    let out = run_cx(p.path(), &["--json", command, "--name", name, "--all"]);
    assert_eq!(out.code, 0, "{} {}", out.stdout, out.stderr);
    out
}
fn coverage(out: &support::Run) -> Value {
    let doc = out.json();
    let text = doc["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .find_map(|s| s.strip_prefix("relation_coverage: "))
        .expect("relation queries must disclose their analysis coverage");
    serde_json::from_str(text).unwrap()
}
fn targets(out: &support::Run) -> Vec<String> {
    out.results()
        .iter()
        .map(|v| v["to"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn same_name_definition_sites_are_not_one_callee_subject() {
    let p = project(&[("a.rs", "fn run() {}\n"), ("b.rs", "fn run() {}\n")]);
    let out = query(&p, "callees", "run");
    assert_eq!(out.results(), Vec::<Value>::new());
    assert!(
        out.json()["warnings"].as_array().unwrap().iter().any(|w| {
            w.as_str()
                .unwrap()
                .contains("2 distinct symbols named \"run\"")
        }),
        "{}",
        out.stdout
    );
}

#[test]
fn overloads_with_identical_labels_remain_ambiguous() {
    let p = project(&[(
        "over.cpp",
        "void run(int x) {}\nvoid run(double x) {}\nvoid entry() { run(1); }\n",
    )]);
    let out = query(&p, "callers", "run");
    assert_eq!(out.json_len(), 1);
    assert_eq!(targets(&out), vec![""]);
    assert_eq!(
        out.results()[0]["ambiguous_candidates"],
        "run [over.cpp:0..18; cpp], run [over.cpp:19..40; cpp]"
    );
}

#[test]
fn unrelated_definition_does_not_hide_a_declaration() {
    let p = project(&[(
        "decl.cpp",
        "namespace a { void run(); }\nnamespace b { void run() {} }\nvoid entry() { a::run(); }\n",
    )]);
    let out = query(&p, "callers", "run");
    assert_eq!(targets(&out), vec!["a::run"]);
    assert_eq!(out.results()[0]["resolution"], "lexical_scope");
}

#[test]
fn nested_function_calls_do_not_belong_to_outer_body() {
    let p = project(&[(
        "nested.rs",
        "fn leaf() {}\nfn outer() {\n fn inner() { leaf(); }\n inner();\n}\n",
    )]);
    let out = query(&p, "callees", "outer");
    assert_eq!(out.json_len(), 1);
    assert_eq!(out.results()[0]["line"], 4);
    let callers = query(&p, "callers", "leaf");
    assert_eq!(callers.json_len(), 1);
    assert_eq!(callers.results()[0]["from"], "inner");
    assert_eq!(callers.results()[0]["line"], 3);
}

#[test]
fn closure_calls_are_not_attributed_to_enclosing_function() {
    let p = project(&[(
        "closure.rs",
        "fn leaf() {}\nfn outer() { let c = || leaf(); }\n",
    )]);
    assert_eq!(query(&p, "callees", "outer").json_len(), 0);
    let out = query(&p, "callers", "leaf");
    assert_eq!(out.json_len(), 1);
    // Oracle correction: || begins at byte 34 (32 is the preceding '=').
    assert_eq!(out.results()[0]["from"], "(anonymous scope@34)");
}

#[test]
fn receiver_and_macro_calls_never_become_unique_function_edges() {
    let p = project(&[(
        "calls.rs",
        "fn run() {}\nfn entry(x: Unknown) { x.run(); run!(); }\n",
    )]);
    let out = query(&p, "callers", "run");
    assert_eq!(targets(&out), vec!["", ""]);
    assert_eq!(
        out.results()
            .iter()
            .map(|r| r["resolution"].clone())
            .collect::<Vec<_>>(),
        vec![json!("syntax"), json!("syntax")]
    );
    assert_eq!(
        out.results()
            .iter()
            .map(|r| r["ambiguous_candidates"].clone())
            .collect::<Vec<_>>(),
        vec![json!("run"), json!("run")]
    );
}

#[test]
fn written_qualifier_is_not_a_substring_or_fallback_hint() {
    let p = project(&[(
        "scope.rs",
        "mod notalpha { pub fn run() {} }\nfn entry() { alpha::run(); }\n",
    )]);
    let out = query(&p, "callers", "run");
    assert_eq!(targets(&out), vec![""]);
    assert_eq!(out.results()[0]["ambiguous_candidates"], "notalpha::run");
}

#[test]
fn name_only_cross_file_uniqueness_is_syntax_not_lexical() {
    let p = project(&[
        ("a.rs", "fn leaf() {}\n"),
        ("b.rs", "fn entry() { leaf(); }\n"),
    ]);
    let out = query(&p, "callers", "leaf");
    assert_eq!(targets(&out), vec!["leaf"]);
    assert_eq!(out.results()[0]["resolution"], "syntax");
}

#[test]
fn repeated_calls_on_one_line_are_distinct_sites() {
    let p = project(&[(
        "repeat.rs",
        "fn leaf() {}\nfn entry() { leaf(); leaf(); }\n",
    )]);
    assert_eq!(
        targets(&query(&p, "callees", "entry")),
        vec!["leaf", "leaf"]
    );
}

#[test]
fn metadata_blind_edit_cannot_mix_old_symbols_with_new_calls() {
    let p = project(&[("stale.rs", "fn leaf() {}\nfn entry() { leaf(); }\n")]);
    query(&p, "callers", "leaf");
    let path = p.path().join("stale.rs");
    let times = fs::FileTimes::new().set_modified(fs::metadata(&path).unwrap().modified().unwrap());
    fs::write(&path, "fn leaf() {}\nfn other() { leaf(); }\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(times)
        .unwrap();
    let out = query(&p, "callers", "leaf");
    assert_eq!(out.json()["freshness"]["mode"], "metadata");
    assert_eq!(out.json()["freshness"]["files_updated"], 0);
    assert_eq!(out.json_len(), 0, "stale owner must not be reported");
    assert_eq!(
        coverage(&out)["issues"],
        json!([{"file":"stale.rs", "reason":"content_changed"}])
    );
    assert_eq!(coverage(&out)["complete_within_model"], false);
}

#[test]
fn parse_errors_and_unsupported_languages_are_not_complete_empty_success() {
    let p = project(&[
        ("bad.rs", "fn broken() { leaf(\n"),
        ("notes.md", "# leaf\n"),
    ]);
    let out = query(&p, "callers", "leaf");
    assert_eq!(
        coverage(&out)["issues"],
        json!([
            {"file":"bad.rs", "reason":"parse_error"},
            {"file":"notes.md", "reason":"unsupported_language"}
        ])
    );
    assert_eq!(coverage(&out)["complete_within_model"], false);
    assert_eq!(out.json_len(), 0);
}

#[test]
fn new_cross_file_target_invalidates_prior_unique_resolution() {
    let p = project(&[
        ("a.rs", "fn leaf() {}\n"),
        ("entry.rs", "fn entry() { leaf(); }\n"),
    ]);
    assert_eq!(targets(&query(&p, "callers", "leaf")), vec!["leaf"]);
    fs::write(p.path().join("b.rs"), "fn leaf() {}\n").unwrap();
    let out = query(&p, "callers", "leaf");
    assert_eq!(targets(&out), vec![""]);
    assert_eq!(
        out.results()[0]["ambiguous_candidates"],
        "leaf [a.rs:0..12; rust], leaf [b.rs:0..12; rust]"
    );
}

#[test]
fn shadowed_function_names_are_not_resolved_to_global_functions() {
    for (file, source) in [
        ("shadow.rs", "fn run() {}\nfn entry(run: fn()) { run(); }\n"),
        (
            "shadow.ts",
            "function run() {}\nfunction entry(run: () => void) { run(); }\n",
        ),
        (
            "shadow.cpp",
            "void run() {}\nvoid entry(void (*run)()) { run(); }\n",
        ),
    ] {
        let p = project(&[(file, source)]);
        let out = query(&p, "callers", "run");
        assert_eq!(targets(&out), vec![""], "{file}: {}", out.stdout);
        assert_eq!(out.results()[0]["resolution"], "syntax");
    }
}

#[test]
fn ts_import_alias_does_not_bind_a_same_named_global_function() {
    let p = project(&[
        ("lib.ts", "export function leaf() {}\n"),
        (
            "main.ts",
            "import { leaf as run } from './lib';\nfunction run() {}\nfunction entry() { run(); }\n",
        ),
    ]);
    assert_eq!(targets(&query(&p, "callers", "run")), vec![""]);
}

#[test]
fn nested_functions_in_sibling_hosts_do_not_resolve_by_file_alone() {
    let p = project(&[(
        "nested.rs",
        "fn a() { fn run() {} run(); }\nfn b() { fn run() {} run(); }\n",
    )]);
    let out = query(&p, "callers", "run");
    assert_eq!(targets(&out), vec!["run", "run"]);
    assert_eq!(
        out.results()
            .iter()
            .map(|r| r["from"].clone())
            .collect::<Vec<_>>(),
        vec![json!("a"), json!("b")]
    );
    assert_eq!(query(&p, "callees", "run").json_len(), 0);
}

#[test]
fn cpp_matching_declaration_and_definition_preserve_one_entity() {
    let p = project(&[
        ("api.hpp", "namespace api { void leaf(int n); }\n"),
        (
            "api.cpp",
            "#include \"api.hpp\"\nnamespace api { void leaf(int n) {} }\n",
        ),
        (
            "main.cpp",
            "#include \"api.hpp\"\nvoid entry() { api::leaf(1); }\n",
        ),
    ]);
    let out = query(&p, "callers", "leaf");
    assert_eq!(targets(&out), vec!["api::leaf"]);
    assert_eq!(out.results()[0]["line"], 2);
    assert_eq!(query(&p, "callees", "leaf").json_len(), 0);
    assert!(
        !out.json()["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("distinct symbols"))
    );
}

#[test]
fn local_cpp_functions_do_not_merge_across_files() {
    let p = project(&[
        ("a.cpp", "static void run() {}\nvoid a() { run(); }\n"),
        ("b.cpp", "static void run() {}\nvoid b() { run(); }\n"),
    ]);
    let out = query(&p, "callers", "run");
    assert_eq!(
        out.results()
            .iter()
            .map(|r| (r["file"].clone(), r["from"].clone(), r["to"].clone()))
            .collect::<Vec<_>>(),
        vec![
            (json!("a.cpp"), json!("a"), json!("run")),
            (json!("b.cpp"), json!("b"), json!("run"))
        ]
    );
    assert_eq!(query(&p, "callees", "run").json_len(), 0);
}

#[test]
fn pagination_and_warm_queries_preserve_each_repeated_call_site() {
    let p = project(&[(
        "repeat.rs",
        "fn leaf() {}\nfn entry() { leaf(); leaf(); leaf(); }\n",
    )]);
    let all = query(&p, "callees", "entry");
    assert_eq!(all.json_len(), 3);
    for _ in 0..3 {
        let warm = query(&p, "callees", "entry");
        assert_eq!(warm.results(), all.results());
        assert_eq!(warm.page(), all.page());
        assert_eq!(coverage(&warm), coverage(&all));
        assert_eq!(
            warm.json()["freshness"]["generation"],
            all.json()["freshness"]["generation"]
        );
        // Cold indexing updates one file; pure warm queries update zero.
        assert_eq!(warm.json()["freshness"]["files_updated"], 0);
    }
    let first = run_cx(
        p.path(),
        &["--json", "callees", "--name", "entry", "--limit", "1"],
    );
    let next = first.json()["next_queries"][0].as_str().unwrap().to_owned();
    // Fixture names are shell-safe; execute the emitted argv without a shell.
    let args: Vec<_> = next.split_whitespace().skip(1).collect();
    let second = run_cx(p.path(), &args);
    assert_eq!(second.code, 0);
    assert_eq!(second.page()["offset"], 1);
    assert_eq!(second.page()["total"], 3);
    assert_eq!(second.results(), vec![all.results()[1].clone()]);
}

#[test]
fn multiline_declaration_uses_full_source_signature_not_outline_summary() {
    let p = project(&[
        ("api.hpp", "void leaf(int n,\n double value);\n"),
        (
            "api.cpp",
            "#include \"api.hpp\"\nvoid target() {}\nvoid leaf(int n,\n double value) { target(); }\n",
        ),
    ]);
    let out = query(&p, "callees", "leaf");
    assert_eq!(targets(&out), vec!["target"]);
    assert_eq!(out.results()[0]["line"], 4);
}

#[test]
fn multiline_overload_declarations_do_not_merge_on_the_first_line() {
    let p = project(&[
        (
            "api.hpp",
            "void leaf(int n,\n double value);\nvoid leaf(int n,\n const char* value);\n",
        ),
        (
            "api.cpp",
            "#include \"api.hpp\"\nvoid leaf(int n,\n double value) {}\nvoid entry() { leaf(1, 2.0); }\n",
        ),
    ]);
    let out = query(&p, "callers", "leaf");
    assert_eq!(targets(&out), vec![""]);
    assert_eq!(
        out.results()[0]["ambiguous_candidates"],
        "leaf [api.cpp:19..53; cpp], leaf [api.hpp:33..70; cpp]"
    );
}
