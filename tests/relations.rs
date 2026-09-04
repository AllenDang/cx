//! Phase 7: direct callers/callees with evidence and resolution levels
//! (roadmap §5.4, §9, §11 Phase 7).
//!
//! Acceptance: every direct edge carries a location, a resolution level and its
//! ambiguity, and the `run` fixture — ten distinct symbols sharing one short
//! name — produces no cross-scope false edge.

mod support;

use support::{fixture_project, run_cx};

const CORPUS: &str = "agent_corpus";

/// (from, to, resolution, ambiguous) tuples, sorted for stable comparison.
fn edges(run: &support::Run) -> Vec<(String, String, String, String)> {
    let mut rows: Vec<(String, String, String, String)> = run
        .results()
        .iter()
        .map(|r| {
            (
                r["from"].as_str().unwrap().to_string(),
                r["to"].as_str().unwrap().to_string(),
                r["resolution"].as_str().unwrap().to_string(),
                r["ambiguous_candidates"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    rows.sort();
    rows
}

fn warnings_of(run: &support::Run) -> Vec<String> {
    run.json()["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

// --- §11 acceptance: no cross-scope false edges ----------------------------

/// The headline condition. `run_all` calls `runner.run()`; ten symbols are named
/// `run`. cx must leave the target empty and list the candidates rather than
/// binding the call to whichever `run` sorts first.
#[test]
fn ambiguous_call_produces_no_edge_and_lists_candidates() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "callers", "--name", "run", "--all"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let unresolved: Vec<(String, String, String, String)> = edges(&out)
        .into_iter()
        .filter(|(from, _, _, _)| from == "run_all")
        .collect();
    assert_eq!(unresolved.len(), 1, "{unresolved:?}");
    let (_, to, resolution, ambiguous) = &unresolved[0];

    assert_eq!(to, "", "no target may be invented: {unresolved:?}");
    assert_eq!(resolution, "syntax", "{unresolved:?}");
    // Every C++ candidate is listed; nothing is silently dropped.
    for expected in [
        "alpha::run",
        "ange::EcsWorld::run",
        "beta::Runner::run",
        "gen::run",
        "thirdparty::run",
    ] {
        assert!(ambiguous.contains(expected), "{ambiguous}");
    }
    // Cross-language candidates are excluded outright: a C++ call cannot mean a
    // TypeScript method.
    assert!(!ambiguous.contains("AlphaRunner.run"), "{ambiguous}");
    assert!(!ambiguous.contains("Tickable.run"), "{ambiguous}");
}

/// Written qualifiers are honest lexical evidence, and they must pick the right
/// scope — `alpha::run()` and `beta::run()` on one line stay separate.
#[test]
fn explicit_qualifiers_resolve_to_the_named_scope() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "callers", "--name", "run", "--all"]);

    let from_run_both: Vec<(String, String, String, String)> = edges(&out)
        .into_iter()
        .filter(|(from, _, _, _)| from == "run_both")
        .collect();
    assert_eq!(
        from_run_both,
        vec![
            (
                "run_both".to_string(),
                "alpha::run".to_string(),
                "lexical_scope".to_string(),
                String::new()
            ),
            (
                "run_both".to_string(),
                "beta::run".to_string(),
                "lexical_scope".to_string(),
                String::new()
            ),
        ],
        "two distinct calls on one line must not collapse into one edge"
    );
}

/// Lexical nesting resolves an unqualified call when exactly one candidate
/// encloses the call site.
#[test]
fn lexical_nesting_resolves_a_same_namespace_call() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "callers", "--name", "run", "--all"]);

    let vendor: Vec<(String, String, String, String)> = edges(&out)
        .into_iter()
        .filter(|(from, _, _, _)| from == "thirdparty::helper")
        .collect();
    assert_eq!(vendor.len(), 1, "{vendor:?}");
    assert_eq!(vendor[0].1, "thirdparty::run", "{vendor:?}");
    assert_eq!(vendor[0].2, "lexical_scope", "{vendor:?}");
}

// --- §5.4 every edge carries location, resolution and ambiguity -------------

#[test]
fn every_edge_carries_location_evidence_and_resolution() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "callers", "--name", "run", "--all"]);
    let rows = out.results();
    assert!(!rows.is_empty());

    for row in &rows {
        assert!(!row["file"].as_str().unwrap().is_empty(), "{row}");
        assert!(row["line"].as_u64().unwrap() > 0, "{row}");
        assert_eq!(row["evidence"].as_str().unwrap(), "call", "{row}");
        let resolution = row["resolution"].as_str().unwrap();
        assert!(
            ["syntax", "lexical_scope", "import_resolved"].contains(&resolution),
            "unexpected resolution {resolution}: {row}"
        );
        // cx resolves no types, so it must never claim to.
        assert_ne!(resolution, "type_resolved", "{row}");
        assert_ne!(resolution, "text", "{row}");

        // An empty target must be accompanied by the candidate list.
        if row["to"].as_str().unwrap().is_empty() {
            assert!(
                !row["ambiguous_candidates"].as_str().unwrap().is_empty(),
                "unresolved edge must list candidates: {row}"
            );
        }
    }
}

#[test]
fn caller_query_warns_about_distinct_symbols_and_unresolved_edges() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "callers", "--name", "run", "--all"]);
    let warnings = warnings_of(&out);

    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("8 distinct symbols named \"run\"")),
        "{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("syntax evidence only; cx does not resolve types")),
        "{warnings:?}"
    );
}

/// A name with exactly one definition resolves cleanly for all its callers.
#[test]
fn unambiguous_target_resolves_for_every_caller() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "callers", "--name", "validate_param", "--all"],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let rows = edges(&out);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        rows.iter().all(|(_, to, _, amb)| to == "ange::validate_param" && amb.is_empty()),
        "{rows:?}"
    );
    let callers: Vec<&str> = rows.iter().map(|(from, _, _, _)| from.as_str()).collect();
    assert_eq!(callers, vec!["alpha::run", "ange::EcsWorld::run"], "{rows:?}");

    // One target, so no ambiguity warning.
    assert!(
        !warnings_of(&out)
            .iter()
            .any(|w| w.contains("distinct symbols")),
        "{:?}",
        warnings_of(&out)
    );
}

#[test]
fn scope_filter_narrows_caller_edges_to_one_target() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "callers", "--name", "run", "--scope", "alpha::*", "--all"],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = edges(&out);
    assert!(!rows.is_empty(), "{rows:?}");
    assert!(
        rows.iter().all(|(_, to, _, _)| to == "alpha::run"),
        "{rows:?}"
    );
}

// --- callees --------------------------------------------------------------

#[test]
fn callees_lists_calls_written_inside_a_body() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "callees", "--name", "run_both", "--all"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let rows = edges(&out);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].1, "alpha::run", "{rows:?}");
    assert_eq!(rows[1].1, "beta::run", "{rows:?}");
    assert!(rows.iter().all(|(from, _, _, _)| from == "run_both"), "{rows:?}");
}

/// Reading one arbitrary body would answer a different question than the one
/// asked, so an ambiguous name is refused with the candidates named.
#[test]
fn callees_refuses_an_ambiguous_symbol_instead_of_guessing() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "callees", "--name", "run", "--all"]);
    assert_eq!(out.code, 0, "an ambiguous request is not a failure");
    assert_eq!(out.json_len(), 0, "{}", out.stdout);
    assert!(
        warnings_of(&out)
            .iter()
            .any(|w| w.contains("Narrow with --scope to choose one")),
        "{:?}",
        warnings_of(&out)
    );
}

#[test]
fn callees_scope_filter_selects_one_body() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "callees", "--name", "run", "--scope", "alpha::*", "--all"],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = edges(&out);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].0, "alpha::run", "{rows:?}");
    assert_eq!(rows[0].1, "ange::validate_param", "{rows:?}");
}

#[test]
fn callees_of_a_leaf_symbol_is_an_empty_success() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "callees", "--name", "entity_count", "--all"],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    // `entity_count` calls only std members, which are not indexed symbols.
    let rows = edges(&out);
    assert!(
        rows.iter().all(|(_, to, _, _)| to.is_empty()),
        "no project-local callees: {rows:?}"
    );
    assert!(out.json()["error"].is_null(), "{}", out.stdout);
}

// --- envelope integration -------------------------------------------------

#[test]
fn relation_queries_use_the_standard_envelope() {
    let p = fixture_project(CORPUS);
    for command in ["callers", "callees"] {
        let out = run_cx(p.path(), &["--json", command, "--name", "run_both", "--all"]);
        let doc = out.json();
        let mut keys: Vec<String> = doc.as_object().unwrap().keys().cloned().collect();
        keys.sort();
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
            ],
            "{command}: {}",
            out.stdout
        );
        assert_eq!(doc["query"]["kind"].as_str().unwrap(), command);
        assert_eq!(doc["query"]["subject"].as_str().unwrap(), "run_both");
        assert!(doc["freshness"]["generation"].as_u64().unwrap() >= 1);
    }
}

#[test]
fn relation_output_is_paginated() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "callers", "--name", "run", "--limit", "2"],
    );
    assert_eq!(out.json_len(), 2, "{}", out.stdout);
    assert!(out.page()["truncated"].as_bool().unwrap());
    let next: Vec<String> = out.json()["next_queries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(next.iter().any(|c| c.contains("--offset 2")), "{next:?}");
}

/// §9 forbids multi-hop until direct edges are solid: there is no `--depth`.
#[test]
fn no_multi_hop_traversal_is_offered() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["callers", "--name", "run", "--depth", "3"],
    );
    assert_ne!(out.code, 0, "a depth flag must not exist yet: {}", out.stdout);
    assert!(
        out.stderr.contains("unexpected argument") || out.stderr.contains("--depth"),
        "stderr: {}",
        out.stderr
    );
}
