//! Source-derived regression controls for the frozen ANGE task failures.
//! Fixtures are authored reductions, not copies of private project source.
mod support;
use serde_json::{Value, json};
use std::fs;
use support::run_cx;

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let p = tempfile::tempdir().unwrap();
    fs::create_dir(p.path().join(".git")).unwrap();
    for (file, text) in files {
        fs::write(p.path().join(file), text).unwrap();
    }
    p
}
fn coverage(out: &support::Run) -> Value {
    let doc = out.json();
    serde_json::from_str(
        doc["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .find_map(|w| w.strip_prefix("relation_coverage: "))
            .unwrap(),
    )
    .unwrap()
}
fn query(p: &tempfile::TempDir, cmd: &str, name: &str, scope: &str) -> support::Run {
    let out = run_cx(
        p.path(),
        &["--json", cmd, "--name", name, "--scope", scope, "--all"],
    );
    assert_eq!(out.code, 0, "{} {}", out.stdout, out.stderr);
    out
}

#[test]
fn casts_do_not_pollute_repeated_call_counts() {
    let p = project(&[(
        "a.cpp",
        "void leaf(int);\nvoid probe(int n) { leaf(static_cast<unsigned char>(n)); leaf(static_cast<int>(n)); }\n",
    )]);
    let out = query(&p, "callees", "probe", "probe");
    assert_eq!(out.json_len(), 2, "{}", out.stdout);
    assert_eq!(
        out.results()
            .iter()
            .map(|r| r["to"].clone())
            .collect::<Vec<_>>(),
        vec![json!("leaf"), json!("leaf")]
    );
    let bad = run_cx(p.path(), &["--json", "callers", "--name", "char", "--all"]);
    assert_eq!(bad.code, 0);
    assert_eq!(bad.json_len(), 0);
}

#[test]
fn template_receiver_calls_are_still_unresolved() {
    let p = project(&[(
        "a.cpp",
        "void run() {}\nvoid probe(Unknown object) { object.run<int>(); }\n",
    )]);
    let out = query(&p, "callees", "probe", "probe");
    assert_eq!(out.json_len(), 1);
    assert_eq!(out.results()[0]["to"], "");
    assert_eq!(out.results()[0]["ambiguous_candidates"], "run");
}

#[test]
fn unique_definition_body_does_not_require_alias_equivalence_proof() {
    let p = project(&[
        (
            "a.hpp",
            "namespace lib { struct Value {}; }\nstruct Registry { static lib::Value find(const lib::Value &name); };\n",
        ),
        (
            "a.cpp",
            "#include \"a.hpp\"\nusing namespace lib;\nvoid leaf() {}\nValue Registry::find(const Value &name) { leaf(); return name; }\n",
        ),
    ]);
    let out = query(&p, "callees", "find", "Registry::find");
    assert_eq!(out.json_len(), 1, "{}", out.stdout);
    assert_eq!(out.results()[0]["to"], "leaf");
    assert_eq!(out.results()[0]["file"], "a.cpp");
    assert!(
        out.json()["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("declaration sites not merged"))
    );
    // Reading the definition does NOT invent a declaration/definition binding.
    fs::write(
        p.path().join("caller.cpp"),
        "#include \"a.hpp\"\nvoid entry() { Registry::find(value); }\n",
    )
    .unwrap();
    let callers = query(&p, "callers", "find", "Registry::find");
    assert_eq!(callers.json_len(), 1);
    assert_eq!(callers.results()[0]["to"], "");
}

#[test]
fn callee_body_coverage_does_not_parse_a_declaration_only_header() {
    let p = project(&[
        ("a.hpp", "void host();\nvoid broken(\n"),
        (
            "a.cpp",
            "#include \"a.hpp\"\nvoid leaf() {}\nvoid host() { leaf(); }\n",
        ),
    ]);
    let out = query(&p, "callees", "host", "host");
    assert_eq!(out.json_len(), 1);
    assert_eq!(coverage(&out)["files_analyzed"], 1);
    assert_eq!(coverage(&out)["issue_counts"], json!({}));
    // Candidate source hashes remain checked, but this is NOT a global parse claim.
    assert_eq!(
        coverage(&out)["scope"],
        "indexed_candidates; callees_selected_bodies"
    );
}

#[test]
fn scoped_callers_keep_matching_unresolved_frontier_without_binding_it() {
    let p = project(&[
        (
            "good.cpp",
            "namespace api { void leaf() {} }\nvoid entry() { api::leaf(); }\n",
        ),
        ("bad.cpp", "void broken() { leaf(\n"),
        ("unrelated.cpp", "namespace other { void leaf() {} }\n"),
    ]);
    let out = query(&p, "callers", "leaf", "api::leaf");
    assert_eq!(out.json_len(), 1, "{}", out.stdout);
    let row = &out.results()[0];
    assert_eq!(
        (row["file"].clone(), row["line"].clone()),
        (json!("good.cpp"), json!(2))
    );
    assert_eq!(row["to"], "");
    assert_eq!(row["resolution"], "syntax");
    assert_eq!(row["ambiguous_candidates"], "api::leaf, other::leaf");
    assert_eq!(coverage(&out)["complete_within_model"], false);
    assert!(
        out.json()["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().starts_with("relation_scope:"))
    );
    // A scope with no matching candidate is still excluded; no guessed edge.
    assert_eq!(query(&p, "callers", "leaf", "missing::leaf").json_len(), 0);
}

#[test]
fn critical_file_failures_survive_the_bounded_issue_sample() {
    let p = project(&[("z.cpp", "void broken() { leaf(\n")]);
    for n in 0..20 {
        fs::write(p.path().join(format!("a{n:02}.md")), "# leaf\n").unwrap();
    }
    let out = query(&p, "callers", "leaf", "leaf");
    let cov = coverage(&out);
    assert_eq!(cov["issues_total"], 21);
    assert_eq!(cov["issues_omitted"], 5);
    assert_eq!(cov["issues"].as_array().unwrap().len(), 16);
    assert_eq!(
        cov["issues"][0],
        json!({"file":"z.cpp", "reason":"parse_error"})
    );
    assert_eq!(
        cov["issue_counts"],
        json!({"parse_error":1, "unsupported_language":20})
    );
}

#[test]
fn absolute_qualification_does_not_bind_the_enclosing_namespace() {
    let p = project(&[(
        "a.cpp",
        "void leaf() {}\nnamespace n { void leaf() {} void entry() { ::leaf(); } }\n",
    )]);
    let out = query(&p, "callees", "entry", "n::entry");
    assert_eq!(out.json_len(), 1);
    assert_eq!(out.results()[0]["to"], "leaf");
    assert_eq!(out.results()[0]["resolution"], "lexical_scope");
}

#[test]
fn computed_calls_are_disclosed_instead_of_inventing_key_calls() {
    let p = project(&[(
        "a.ts",
        "function probe(table: any, key: string) { table[key](); }\n",
    )]);
    let out = query(&p, "callees", "probe", "probe");
    assert_eq!(out.json_len(), 0);
    assert_eq!(coverage(&out)["complete_within_model"], false);
    assert_eq!(
        coverage(&out)["issue_counts"],
        json!({"unsupported_call_form":1})
    );
}
