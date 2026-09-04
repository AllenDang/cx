//! Phase 0 baseline corpus tests.
//!
//! These pin the *current* observable behavior of cx against a deliberate
//! fixture tree (`tests/fixtures/agent_corpus`) so later roadmap phases have a
//! red/green signal instead of prose.  Counts are exact on purpose: a test that
//! only asserts `> 0` cannot detect a regression in symbol identity.
//!
//! Tests whose expectation the roadmap intends to *change* are marked with the
//! phase that will flip them.

mod support;

use support::{fixture_project, run_cx};

const CORPUS: &str = "agent_corpus";

// --- Corpus shape -----------------------------------------------------------

#[test]
fn corpus_indexes_every_supported_file() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "symbols", "--all"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(
        out.json_len(),
        41,
        "total symbols in corpus\n{}",
        out.stdout
    );
}

#[test]
fn root_overview_lists_one_level() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "overview", "."]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    // 6 subdirectories (docs, generated, include, src, tests, vendor) + README.md
    assert_eq!(out.json_len(), 7, "{}", out.stdout);
}

#[test]
fn overview_no_tests_still_lists_production_files() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "overview", "src", "--no-tests"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    // src/ holds 6 indexable files, none of which match test-path conventions.
    assert_eq!(out.json_len(), 6, "{}", out.stdout);
}

#[test]
fn test_paths_are_classified_by_convention() {
    let p = fixture_project(CORPUS);
    let all = run_cx(p.path(), &["--json", "overview", ".", "--full", "--all"]);
    let with_no_tests = run_cx(
        p.path(),
        &["--json", "overview", ".", "--full", "--all", "--no-tests"],
    );
    assert_eq!(all.code, 0, "stderr: {}", all.stderr);
    assert_eq!(with_no_tests.code, 0, "stderr: {}", with_no_tests.stderr);
    // tests/ecs_test.cpp contributes one symbol that --no-tests removes.
    assert_eq!(
        all.json_len() - with_no_tests.json_len(),
        1,
        "--no-tests should drop exactly the tests/ aggregate row\nall:\n{}\nno-tests:\n{}",
        all.stdout,
        with_no_tests.stdout
    );
}

// --- §4.2 declaration vs definition ----------------------------------------

/// KNOWN GAP (roadmap §4.2, flipped by Phase 2): a C++ forward declaration and
/// its definition are both indexed as plain definitions.  Nothing in the
/// machine-readable output distinguishes them except a trailing `;` in the
/// signature, which is not a contract.
#[test]
fn cpp_declaration_and_definition_are_currently_indistinguishable() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "definition", "--name", "validate_param", "--all"],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = out.json();
    let rows = rows.as_array().unwrap();
    assert_eq!(
        rows.len(),
        2,
        "decl in header + def in source\n{}",
        out.stdout
    );

    let files: Vec<&str> = rows.iter().map(|r| r["file"].as_str().unwrap()).collect();
    assert_eq!(files, vec!["include/ange/ecs.hpp", "src/ecs.cpp"]);

    // No role field exists yet — Phase 2 must add one.
    assert!(
        rows.iter().all(|r| r.get("role").is_none()),
        "unexpected role field already present: {}",
        out.stdout
    );
}

// --- §4.3 same name in different scopes ------------------------------------

/// KNOWN GAP (roadmap §4.3, flipped by Phase 5): twelve distinct `run` symbols
/// across namespaces, modules, classes and vendored code are returned as one
/// undifferentiated list keyed only by name + file + range.
#[test]
fn same_name_symbols_in_different_scopes_are_not_qualified() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "symbols", "--name", "run", "--all"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = out.json();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 12, "distinct `run` symbols\n{}", out.stdout);

    // Every row is name-only: no qualified_name / scope / owner yet.
    for row in rows {
        assert_eq!(row["name"].as_str().unwrap(), "run");
        assert!(row.get("qualified_name").is_none(), "{}", out.stdout);
        assert!(row.get("scope_path").is_none(), "{}", out.stdout);
    }

    // Two Rust modules, two C++ namespaces, one C++ class method, one C++
    // declaration, three TypeScript members, vendor + generated copies.
    let files: Vec<&str> = rows.iter().map(|r| r["file"].as_str().unwrap()).collect();
    assert_eq!(files.iter().filter(|f| **f == "src/lib.rs").count(), 2);
    assert_eq!(files.iter().filter(|f| **f == "src/app.ts").count(), 3);
    assert_eq!(files.iter().filter(|f| **f == "src/scope_b.cpp").count(), 2);
    assert_eq!(
        files
            .iter()
            .filter(|f| **f == "vendor/thirdparty/blob.cpp")
            .count(),
        1
    );
    assert_eq!(
        files
            .iter()
            .filter(|f| **f == "generated/gen_api.cpp")
            .count(),
        1
    );
}

#[test]
fn definition_matches_every_scope_of_a_shared_name() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "definition", "--name", "run", "--all"],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(out.json_len(), 12, "{}", out.stdout);
}

#[test]
fn definition_from_narrows_to_one_scope() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &[
            "--json",
            "definition",
            "--name",
            "run",
            "--from",
            "src/scope_a.cpp",
            "--all",
        ],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = out.json();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{}", out.stdout);
    assert_eq!(rows[0]["file"].as_str().unwrap(), "src/scope_a.cpp");
    assert_eq!(rows[0]["line"].as_u64().unwrap(), 6);
}

// --- §3.3 syntax-level references ------------------------------------------

#[test]
fn references_are_syntax_filtered_not_text_matched() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &[
            "--json",
            "references",
            "--name",
            "run",
            "--context",
            "--all",
        ],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = out.json();
    let rows = rows.as_array().unwrap();

    // 18 identifier occurrences exist; two share src/lib.rs line 18 and are
    // collapsed by the per-line dedup, leaving 17 rows.
    assert_eq!(rows.len(), 17, "{}", out.stdout);

    // src/comments.cpp mentions `run` only in comments and a string literal.
    assert!(
        rows.iter()
            .all(|r| r["file"].as_str().unwrap() != "src/comments.cpp"),
        "comment/string text must not be reported as a reference\n{}",
        out.stdout
    );

    // Enclosing-symbol attribution is the only caller evidence today.
    let callers: Vec<&str> = rows
        .iter()
        .filter(|r| r["file"].as_str().unwrap() == "src/scope_b.cpp")
        .map(|r| r["caller"].as_str().unwrap())
        .collect();
    assert_eq!(callers, vec!["run", "run", "run_all"]);

    // No evidence/resolution labelling yet — Phase 7 must add it.
    assert!(
        rows.iter().all(|r| r.get("evidence").is_none()),
        "{}",
        out.stdout
    );
    assert!(
        rows.iter().all(|r| r.get("resolution").is_none()),
        "{}",
        out.stdout
    );
}

#[test]
fn references_summary_groups_by_file() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "references", "--name", "run", "--all"],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = out.json();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 9, "files containing references\n{}", out.stdout);

    let lib = rows
        .iter()
        .find(|r| r["file"].as_str().unwrap() == "src/lib.rs")
        .expect("src/lib.rs row");
    assert_eq!(lib["refs"].as_u64().unwrap(), 3);
    assert_eq!(lib["lines"].as_str().unwrap(), "5, 12, 18");
    assert_eq!(lib["callers"].as_str().unwrap(), "run, run_both");
}

/// KNOWN GAP (roadmap §6.2, flipped by Phase 3): a successful query with zero
/// results prints nothing on stdout, so `--json` output is not parseable and an
/// agent cannot distinguish "no results" from "command produced no output".
#[test]
fn empty_result_currently_emits_no_json_body() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &[
            "--json",
            "references",
            "--name",
            "run",
            "--file",
            "src/comments.cpp",
        ],
    );
    assert_eq!(out.code, 0, "empty result is not an error");
    assert_eq!(out.stdout, "", "no JSON body today: {:?}", out.stdout);
    assert!(out.stderr.contains("no matches"), "stderr: {}", out.stderr);
}

// --- §6.3 output budget ----------------------------------------------------

/// KNOWN GAP (roadmap §3.4/§6.1, flipped by Phase 3): the JSON root type
/// changes with result count — bare array when everything fits, envelope object
/// once truncated or offset.
#[test]
fn json_root_type_changes_with_pagination() {
    let p = fixture_project(CORPUS);

    let unpaged = run_cx(p.path(), &["--json", "symbols", "--name", "run", "--all"]);
    assert!(unpaged.json().is_array(), "{}", unpaged.stdout);

    let truncated = run_cx(
        p.path(),
        &["--json", "symbols", "--name", "run", "--limit", "4"],
    );
    let env = truncated.json();
    assert!(env.is_object(), "{}", truncated.stdout);
    assert_eq!(env["total"].as_u64().unwrap(), 12);
    assert_eq!(env["offset"].as_u64().unwrap(), 0);
    assert_eq!(env["limit"].as_u64().unwrap(), 4);
    assert_eq!(env["results"].as_array().unwrap().len(), 4);
    // Truncation is only visible on stderr, not in the payload.
    assert!(env.get("truncated").is_none(), "{}", truncated.stdout);
    assert!(
        truncated.stderr.contains("4/12"),
        "expected pagination hint on stderr: {}",
        truncated.stderr
    );

    let offset = run_cx(
        p.path(),
        &[
            "--json", "symbols", "--name", "run", "--offset", "10", "--all",
        ],
    );
    let env = offset.json();
    assert!(
        env.is_object(),
        "offset alone also switches root type: {}",
        offset.stdout
    );
    assert_eq!(env["total"].as_u64().unwrap(), 12);
    assert_eq!(env["results"].as_array().unwrap().len(), 2);
}

#[test]
fn pagination_pages_cover_the_full_result_set_without_overlap() {
    let p = fixture_project(CORPUS);
    let mut seen: Vec<String> = Vec::new();
    for offset in ["0", "5", "10"] {
        let out = run_cx(
            p.path(),
            &[
                "--json", "symbols", "--name", "run", "--offset", offset, "--limit", "5",
            ],
        );
        assert_eq!(out.code, 0, "stderr: {}", out.stderr);
        let env = out.json();
        let rows = env["results"].as_array().unwrap();
        for row in rows {
            seen.push(format!(
                "{}:{}",
                row["file"].as_str().unwrap(),
                row["signature"].as_str().unwrap()
            ));
        }
    }
    assert_eq!(
        seen.len(),
        12,
        "three pages must cover all 12 rows: {seen:?}"
    );
}

// --- §4.4 freshness --------------------------------------------------------

#[test]
fn edited_file_is_reindexed_on_next_query() {
    let p = fixture_project(CORPUS);
    let before = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "src/scope_a.cpp", "--all"],
    );
    assert_eq!(before.json_len(), 2, "{}", before.stdout);

    let target = p.path().join("src/scope_a.cpp");
    let mut src = std::fs::read_to_string(&target).unwrap();
    src.push_str("\nnamespace alpha { void extra_tick() {} }\n");
    std::fs::write(&target, src).unwrap();
    support::touch_future(&target);

    let after = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "src/scope_a.cpp", "--all"],
    );
    assert_eq!(
        after.json_len(),
        4,
        "new symbol must appear\n{}",
        after.stdout
    );
}

#[test]
fn deleted_file_drops_out_of_the_index() {
    let p = fixture_project(CORPUS);
    assert_eq!(
        run_cx(p.path(), &["--json", "symbols", "--name", "run", "--all"]).json_len(),
        12
    );

    std::fs::remove_file(p.path().join("vendor/thirdparty/blob.cpp")).unwrap();

    let out = run_cx(p.path(), &["--json", "symbols", "--name", "run", "--all"]);
    assert_eq!(
        out.json_len(),
        11,
        "vendor copy must disappear\n{}",
        out.stdout
    );
}

#[test]
fn renamed_file_moves_its_symbols() {
    let p = fixture_project(CORPUS);
    let _ = run_cx(p.path(), &["--json", "symbols", "--all"]);

    let from = p.path().join("src/scope_a.cpp");
    let to = p.path().join("src/scope_a_renamed.cpp");
    std::fs::rename(&from, &to).unwrap();
    support::touch_future(&to);

    let out = run_cx(p.path(), &["--json", "symbols", "--name", "run", "--all"]);
    let rows = out.json();
    let files: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["file"].as_str().unwrap())
        .collect();
    assert!(!files.contains(&"src/scope_a.cpp"), "{files:?}");
    assert!(files.contains(&"src/scope_a_renamed.cpp"), "{files:?}");
    assert_eq!(files.len(), 12, "{files:?}");
}

/// KNOWN GAP (roadmap §4.4, flipped by Phase 4): nothing in the output reveals
/// index generation, freshness mode, or how many files were checked/updated.
#[test]
fn freshness_is_not_observable_in_output() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "symbols", "--name", "run", "--all"]);
    let text = out.stdout;
    for field in ["freshness", "generation", "files_checked", "files_updated"] {
        assert!(
            !text.contains(field),
            "unexpected freshness field {field}: {text}"
        );
    }
}

// --- Error surfaces --------------------------------------------------------

#[test]
fn unindexed_file_filter_exits_1() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "symbols", "--file", "src/nope.cpp"]);
    assert_eq!(out.code, 1);
    assert!(out.stdout.is_empty(), "{}", out.stdout);
    assert!(
        out.stderr.contains("file not in index"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn unsupported_file_type_reports_extension() {
    let p = fixture_project(CORPUS);
    std::fs::write(p.path().join("notes.unknownext"), "run\n").unwrap();
    let out = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "notes.unknownext"],
    );
    assert_eq!(out.code, 1);
    assert!(
        out.stderr.contains("unsupported file type: .unknownext"),
        "stderr: {}",
        out.stderr
    );
}
