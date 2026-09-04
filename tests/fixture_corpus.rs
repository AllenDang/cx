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

// --- §4.2 declaration vs definition (Phase 2) ---------------------------------

/// Phase 2 (roadmap §4.2, §5.1): a C++ forward declaration and its definition
/// are now machine-distinguishable by `role`, and the implementation sorts
/// first — an agent asking for a definition gets the body, not the prototype.
#[test]
fn cpp_declaration_and_definition_are_machine_distinguishable() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "definition", "--name", "validate_param", "--all"],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = out.results();
    assert_eq!(
        rows.len(),
        2,
        "decl in header + def in source\n{}",
        out.stdout
    );

    let seen: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap(),
                r["role"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            ("src/ecs.cpp", "definition"),
            ("include/ange/ecs.hpp", "declaration"),
        ],
        "implementation must sort ahead of the prototype\n{}",
        out.stdout
    );
}

#[test]
fn role_filter_selects_declarations_only() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "symbols", "--role", "declaration", "--all"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = out.results();

    // 4 signature-only sites in the C++ header (free function, in-class run,
    // entity_count, constructor), the class-scope `run` declaration in
    // scope_b.cpp, and 1 TypeScript interface member.
    let seen: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r["file"].as_str().unwrap(), r["name"].as_str().unwrap()))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("include/ange/ecs.hpp", "EcsWorld"),
            ("include/ange/ecs.hpp", "entity_count"),
            ("include/ange/ecs.hpp", "run"),
            ("include/ange/ecs.hpp", "validate_param"),
            ("src/app.ts", "run"),
            ("src/scope_b.cpp", "run"),
        ],
        "{}",
        out.stdout
    );
    assert!(
        rows.iter().all(|r| r["role"].as_str().unwrap() == "declaration"),
        "{}",
        out.stdout
    );
}

#[test]
fn role_filter_selects_the_implementation_of_a_shared_name() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &[
            "--json",
            "definition",
            "--name",
            "validate_param",
            "--role",
            "definition",
            "--all",
        ],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = out.results();
    assert_eq!(rows.len(), 1, "{}", out.stdout);
    assert_eq!(rows[0]["file"].as_str().unwrap(), "src/ecs.cpp");
    assert!(rows[0]["body"].as_str().unwrap().contains("runtime_error"), "{}", out.stdout);
}

#[test]
fn every_symbol_carries_a_role() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "symbols", "--all"]);
    let rows = out.results();

    let mut counts = std::collections::BTreeMap::new();
    for row in rows {
        let role = row["role"].as_str().expect("every row has a role");
        *counts.entry(role.to_string()).or_insert(0usize) += 1;
    }
    assert_eq!(counts.get("definition"), Some(&31), "{counts:?}");
    assert_eq!(counts.get("declaration"), Some(&6), "{counts:?}");
    assert_eq!(counts.get("heading"), Some(&4), "{counts:?}");
    assert_eq!(counts.get("unknown"), None, "{counts:?}");
}

#[test]
fn markdown_headings_use_the_heading_role() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "symbols", "--file", "docs/design.md", "--all"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = out.results();
    for row in &rows {
        assert_eq!(row["role"].as_str().unwrap(), "heading", "{}", out.stdout);
    }
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
    let rows = out.results();
    assert_eq!(rows.len(), 12, "distinct `run` symbols\n{}", out.stdout);

    // Every row is name-only: no qualified_name / scope / owner yet.
    for row in &rows {
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
    let rows = out.results();
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
    let rows = out.results();

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
    let rows = out.results();
    assert_eq!(rows.len(), 9, "files containing references\n{}", out.stdout);

    let lib = rows
        .iter()
        .find(|r| r["file"].as_str().unwrap() == "src/lib.rs")
        .expect("src/lib.rs row");
    assert_eq!(lib["refs"].as_u64().unwrap(), 3);
    assert_eq!(lib["lines"].as_str().unwrap(), "5, 12, 18");
    assert_eq!(lib["callers"].as_str().unwrap(), "run, run_both");
}

/// Phase 3 (roadmap §6.2): a successful query with zero results returns the
/// standard envelope with an empty `results` array and `error: null`, so an
/// agent can tell "nothing matched" from "the command failed".
#[test]
fn empty_result_emits_an_envelope_with_zero_results() {
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

    let doc = out.json();
    assert!(doc.is_object(), "{}", out.stdout);
    assert_eq!(doc["schema_version"].as_u64().unwrap(), 1);
    assert_eq!(doc["query"]["kind"].as_str().unwrap(), "references");
    assert_eq!(doc["query"]["subject"].as_str().unwrap(), "run");
    assert!(doc["results"].as_array().unwrap().is_empty(), "{}", out.stdout);
    assert_eq!(doc["page"]["total"].as_u64().unwrap(), 0);
    assert!(!doc["page"]["truncated"].as_bool().unwrap());
    assert!(doc["error"].is_null(), "zero results is not an error: {}", out.stdout);
    // In JSON mode the payload is authoritative: no duplicate stderr chatter.
    assert!(
        !out.stderr.contains("no matches"),
        "json mode should not narrate on stderr: {}",
        out.stderr
    );

    // TOON mode keeps the human/agent note it always had.
    let toon = run_cx(
        p.path(),
        &["references", "--name", "run", "--file", "src/comments.cpp"],
    );
    assert_eq!(toon.code, 0);
    assert!(toon.stdout.is_empty(), "{}", toon.stdout);
    assert!(toon.stderr.contains("no matches"), "stderr: {}", toon.stderr);
}

// --- §6.1/§6.3 stable envelope and output budget ----------------------------

/// Phase 3 (roadmap §3.4, §6.1): the JSON root is always the same object, with
/// the same key set, whether the result set is complete, truncated, or offset.
#[test]
fn json_root_type_is_stable_across_pagination() {
    let p = fixture_project(CORPUS);

    let expected_keys = vec![
        "error",
        "next_queries",
        "page",
        "query",
        "results",
        "schema_version",
        "warnings",
    ];
    let keys_of = |doc: &serde_json::Value| -> Vec<String> {
        let mut keys: Vec<String> = doc.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };

    let unpaged = run_cx(p.path(), &["--json", "symbols", "--name", "run", "--all"]);
    let doc = unpaged.json();
    assert!(doc.is_object(), "{}", unpaged.stdout);
    assert_eq!(keys_of(&doc), expected_keys, "{}", unpaged.stdout);
    assert_eq!(doc["page"]["total"].as_u64().unwrap(), 12);
    assert!(!doc["page"]["truncated"].as_bool().unwrap());
    assert!(doc["page"]["limit"].is_null(), "--all means no limit");
    assert!(doc["next_queries"].as_array().unwrap().is_empty());

    let truncated = run_cx(
        p.path(),
        &["--json", "symbols", "--name", "run", "--limit", "4"],
    );
    let doc = truncated.json();
    assert_eq!(keys_of(&doc), expected_keys, "{}", truncated.stdout);
    assert_eq!(doc["page"]["total"].as_u64().unwrap(), 12);
    assert_eq!(doc["page"]["offset"].as_u64().unwrap(), 0);
    assert_eq!(doc["page"]["limit"].as_u64().unwrap(), 4);
    assert_eq!(doc["results"].as_array().unwrap().len(), 4);
    // Truncation is now a payload fact, not a stderr-only hint.
    assert!(doc["page"]["truncated"].as_bool().unwrap());
    assert!(
        !truncated.stderr.contains("4/12"),
        "json mode carries pagination in the payload, not on stderr: {}",
        truncated.stderr
    );

    // The stderr hint remains for TOON output.
    let toon = run_cx(p.path(), &["symbols", "--name", "run", "--limit", "4"]);
    assert!(
        toon.stderr.contains("4/12"),
        "toon keeps the stderr hint: {}",
        toon.stderr
    );

    let offset = run_cx(
        p.path(),
        &[
            "--json", "symbols", "--name", "run", "--offset", "10", "--all",
        ],
    );
    let doc = offset.json();
    assert_eq!(keys_of(&doc), expected_keys, "{}", offset.stdout);
    assert_eq!(doc["page"]["total"].as_u64().unwrap(), 12);
    assert_eq!(doc["page"]["offset"].as_u64().unwrap(), 10);
    assert_eq!(doc["results"].as_array().unwrap().len(), 2);
    assert!(!doc["page"]["truncated"].as_bool().unwrap());
}

/// Phase 3 (roadmap §6.3): a truncated page carries exact, runnable follow-up
/// commands rather than a prose hint.
#[test]
fn truncated_page_suggests_runnable_next_queries() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "symbols", "--name", "run", "--limit", "4"],
    );
    let doc = out.json();
    let next: Vec<&str> = doc["next_queries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        next,
        vec![
            "cx --json symbols --name run --limit 4 --offset 4",
            "cx --json symbols --name run --all",
        ],
        "{}",
        out.stdout
    );

    // Run the suggested next page verbatim (minus the binary name) and check it
    // returns exactly the rows this page omitted.
    let follow_up = run_cx(
        p.path(),
        &[
            "--json", "symbols", "--name", "run", "--limit", "4", "--offset", "4",
        ],
    );
    assert_eq!(follow_up.code, 0, "stderr: {}", follow_up.stderr);
    assert_eq!(follow_up.page()["offset"].as_u64().unwrap(), 4);
    assert_eq!(follow_up.json_len(), 4);
}

/// Phase 3 (roadmap §6.2): several candidates for one name is an ambiguity that
/// must be reported, not silently resolved by picking the first.
#[test]
fn ambiguous_definition_reports_a_warning() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "definition", "--name", "run", "--all"]);
    let doc = out.json();
    let warnings: Vec<&str> = doc["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(warnings.len(), 1, "{}", out.stdout);
    assert!(
        warnings[0].starts_with("12 candidates share the name \"run\""),
        "{}",
        warnings[0]
    );

    // A single unambiguous match carries no warning.
    let single = run_cx(
        p.path(),
        &["--json", "definition", "--name", "run_all", "--all"],
    );
    assert_eq!(single.json_len(), 1, "{}", single.stdout);
    assert!(
        single.json()["warnings"].as_array().unwrap().is_empty(),
        "{}",
        single.stdout
    );
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
    let rows = out.results();
    let files: Vec<&str> = rows
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
fn unindexed_file_filter_reports_a_machine_readable_code() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "symbols", "--file", "src/nope.cpp"]);
    assert_eq!(out.code, 1);

    // Phase 3: the failure is in the payload, not only on stderr.
    let doc = out.json();
    assert_eq!(out.error_code().as_deref(), Some("file_not_indexed"), "{}", out.stdout);
    assert_eq!(
        doc["error"]["message"].as_str().unwrap(),
        "file not in index: src/nope.cpp"
    );
    assert!(doc["results"].as_array().unwrap().is_empty(), "{}", out.stdout);

    // Without --json the same failure is reported on stderr as before.
    let toon = run_cx(p.path(), &["symbols", "--file", "src/nope.cpp"]);
    assert_eq!(toon.code, 1);
    assert!(
        toon.stderr.contains("file not in index"),
        "stderr: {}",
        toon.stderr
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
    assert_eq!(
        out.error_code().as_deref(),
        Some("unsupported_file_type"),
        "{}",
        out.stdout
    );

    let toon = run_cx(p.path(), &["symbols", "--file", "notes.unknownext"]);
    assert!(
        toon.stderr.contains("unsupported file type: .unknownext"),
        "stderr: {}",
        toon.stderr
    );
}

/// Phase 3 (roadmap §6.2): a directory scope with no indexed files is an error
/// with its own code, distinct from a query that simply matched nothing.
#[test]
fn empty_directory_scope_is_an_error_not_an_empty_result() {
    let p = fixture_project(CORPUS);
    std::fs::create_dir_all(p.path().join("empty_dir")).unwrap();
    let out = run_cx(p.path(), &["--json", "overview", "empty_dir"]);
    assert_eq!(out.code, 1);
    assert_eq!(
        out.error_code().as_deref(),
        Some("no_indexed_files"),
        "{}",
        out.stdout
    );
}
