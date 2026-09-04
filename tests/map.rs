//! Phase 6: bounded repository map (roadmap §8, §11 Phase 6).
//!
//! Acceptance: the top of the map must not be dominated by third-party or
//! generated code or by common identifiers, output must be bounded, and the
//! ranking must be explained. Only provable facts appear — an import that cannot
//! be resolved to an indexed file is counted, never turned into an edge.

mod support;

use std::fs;

use support::{fixture_project, run_cx};

const CORPUS: &str = "agent_corpus";

fn rows_of(run: &support::Run) -> Vec<(String, String, u64, u64)> {
    run.results()
        .iter()
        .map(|r| {
            (
                r["subsystem"].as_str().unwrap().to_string(),
                r["class"].as_str().unwrap().to_string(),
                r["files"].as_u64().unwrap(),
                r["dependents"].as_u64().unwrap(),
            )
        })
        .collect()
}

fn warnings_of(run: &support::Run) -> Vec<String> {
    run.json()["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

// --- §8 filters -------------------------------------------------------------

#[test]
fn vendor_generated_and_tests_are_excluded_by_default_and_reported() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "map"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let subsystems: Vec<String> = rows_of(&out).into_iter().map(|(s, _, _, _)| s).collect();
    assert!(
        !subsystems.contains(&"vendor/".to_string()),
        "{subsystems:?}"
    );
    assert!(
        !subsystems.contains(&"generated/".to_string()),
        "{subsystems:?}"
    );
    assert!(
        !subsystems.contains(&"tests/".to_string()),
        "{subsystems:?}"
    );

    // Exclusions are facts in the payload, not silent omissions.
    let warnings = warnings_of(&out);
    assert!(
        warnings
            .iter()
            .any(|w| w == "1 files excluded as vendor (use --include-vendor)"),
        "{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w == "1 files excluded as generated (use --include-generated)"),
        "{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w == "1 files excluded as test (use --tests)"),
        "{warnings:?}"
    );
}

#[test]
fn opt_in_flags_bring_excluded_classes_back() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &[
            "--json",
            "map",
            "--include-vendor",
            "--include-generated",
            "--tests",
        ],
    );
    let rows = rows_of(&out);
    let by_name: Vec<(String, String)> = rows
        .iter()
        .map(|(s, c, _, _)| (s.clone(), c.clone()))
        .collect();

    assert!(
        by_name.contains(&("vendor/".to_string(), "vendor".to_string())),
        "{by_name:?}"
    );
    assert!(
        by_name.contains(&("generated/".to_string(), "generated".to_string())),
        "{by_name:?}"
    );
    assert!(
        by_name.contains(&("tests/".to_string(), "test".to_string())),
        "{by_name:?}"
    );
}

#[test]
fn exclude_glob_filters_paths_and_reports_the_count() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "map", "--exclude", "src/scope_*"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let src = rows_of(&out)
        .into_iter()
        .find(|(s, _, _, _)| s == "src/")
        .expect("src/ row");
    // src/ holds 6 indexable files; two match the glob.
    assert_eq!(src.2, 4, "{src:?}");
    assert!(
        warnings_of(&out)
            .iter()
            .any(|w| w == "2 files excluded by --exclude"),
        "{:?}",
        warnings_of(&out)
    );
}

// --- §8 acceptance: the top is meaningful ----------------------------------

/// The load-bearing header ranks first because two subsystems include it — not
/// the vendored blob, and not whichever directory happens to be largest.
#[test]
fn ranking_puts_the_most_depended_upon_subsystem_first() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &[
            "--json",
            "map",
            "--include-vendor",
            "--include-generated",
            "--tests",
        ],
    );
    let rows = rows_of(&out);
    assert_eq!(rows[0].0, "include/", "{rows:?}");
    assert_eq!(rows[0].3, 2, "src/ and tests/ both include it: {rows:?}");

    // Vendored and generated code is present but not at the top.
    let vendor_position = rows.iter().position(|(s, _, _, _)| s == "vendor/").unwrap();
    assert!(vendor_position > 0, "{rows:?}");
    assert_eq!(
        rows[vendor_position].3, 0,
        "nothing depends on vendor: {rows:?}"
    );
}

#[test]
fn ranking_basis_is_explained_in_the_output() {
    let p = fixture_project(CORPUS);
    let json = run_cx(p.path(), &["--json", "map"]);
    assert!(
        warnings_of(&json)
            .iter()
            .any(|w| w == "ranked by dependents desc, then symbols desc, then name"),
        "{:?}",
        warnings_of(&json)
    );

    let toon = run_cx(p.path(), &["map"]);
    assert!(
        toon.stderr.contains("ranked by dependents desc"),
        "stderr: {}",
        toon.stderr
    );
}

/// Common identifiers must not fill the API sample; `run` exists in nearly every
/// fixture file and carries no orientation value.
#[test]
fn low_information_symbol_names_are_suppressed_from_api_samples() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "map", "--include-vendor", "--include-generated"],
    );
    for row in out.results() {
        let api = row["api"].as_str().unwrap();
        let names: Vec<&str> = api.split(", ").collect();
        assert!(
            !names.contains(&"run"),
            "`run` must be suppressed as low-information: {api}"
        );
        assert!(!names.contains(&"get") && !names.contains(&"new"), "{api}");
    }

    // Distinctive names still appear.
    let src = out
        .results()
        .into_iter()
        .find(|r| r["subsystem"].as_str().unwrap() == "src/")
        .expect("src/ row");
    let api = src["api"].as_str().unwrap();
    assert!(
        api.contains("AlphaRunner") || api.contains("Tickable"),
        "{api}"
    );
}

// --- §8: only provable edges ----------------------------------------------

#[test]
fn resolved_includes_become_edges_and_unresolved_ones_are_counted() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "map"]);

    let src = out
        .results()
        .into_iter()
        .find(|r| r["subsystem"].as_str().unwrap() == "src/")
        .expect("src/ row");

    // `#include "ange/ecs.hpp"` resolves to include/ange/ecs.hpp.
    assert_eq!(src["depends_on"].as_str().unwrap(), "include/", "{src}");
    // `<stdexcept>` and `<string>`/`<vector>` are not project files.
    assert!(
        src["external_imports"].as_u64().unwrap() >= 1,
        "system headers must be counted as external: {src}"
    );
}

#[test]
fn ambiguous_includes_produce_no_edge_and_are_reported() {
    let p = fixture_project(CORPUS);
    // Two indexed files match the include suffix `dup/util.h`, so no edge may be
    // claimed for either.
    for dir in ["a", "b"] {
        fs::create_dir_all(p.path().join(format!("{dir}/dup"))).unwrap();
        fs::write(
            p.path().join(format!("{dir}/dup/util.h")),
            "void util_fn();\n",
        )
        .unwrap();
    }
    fs::write(
        p.path().join("src/uses_dup.cpp"),
        "#include \"dup/util.h\"\nvoid uses_dup() {}\n",
    )
    .unwrap();

    let out = run_cx(p.path(), &["--json", "map"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let warnings = warnings_of(&out);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("matched several indexed files and were left unresolved")),
        "{warnings:?}"
    );

    let src = out
        .results()
        .into_iter()
        .find(|r| r["subsystem"].as_str().unwrap() == "src/")
        .expect("src/ row");
    let depends: Vec<&str> = src["depends_on"].as_str().unwrap().split(", ").collect();
    assert!(!depends.contains(&"a/"), "{src}");
    assert!(!depends.contains(&"b/"), "{src}");
}

// --- §8: bounded output ---------------------------------------------------

#[test]
fn depth_controls_subsystem_granularity() {
    let p = fixture_project(CORPUS);

    let shallow = run_cx(p.path(), &["--json", "map", "--depth", "1"]);
    let deep = run_cx(p.path(), &["--json", "map", "--depth", "2"]);

    let shallow_names: Vec<String> = rows_of(&shallow)
        .into_iter()
        .map(|(s, _, _, _)| s)
        .collect();
    let deep_names: Vec<String> = rows_of(&deep).into_iter().map(|(s, _, _, _)| s).collect();

    assert!(
        shallow_names.contains(&"include/".to_string()),
        "{shallow_names:?}"
    );
    assert!(
        deep_names.contains(&"include/ange/".to_string()),
        "depth 2 splits the include tree: {deep_names:?}"
    );
}

#[test]
fn map_output_is_paginated_with_runnable_next_queries() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &[
            "--json",
            "map",
            "--include-vendor",
            "--include-generated",
            "--tests",
            "--limit",
            "2",
        ],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(out.json_len(), 2, "{}", out.stdout);

    let page = out.page();
    assert!(page["truncated"].as_bool().unwrap());
    assert!(page["total"].as_u64().unwrap() > 2);

    let next: Vec<String> = out.json()["next_queries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(next.iter().any(|c| c.contains("--offset 2")), "{next:?}");

    // The suggested page returns the remaining rows without overlap.
    let page2 = run_cx(
        p.path(),
        &[
            "--json",
            "map",
            "--include-vendor",
            "--include-generated",
            "--tests",
            "--limit",
            "2",
            "--offset",
            "2",
        ],
    );
    assert_eq!(page2.page()["offset"].as_u64().unwrap(), 2);
    let first: Vec<String> = rows_of(&out).into_iter().map(|(s, _, _, _)| s).collect();
    let second: Vec<String> = rows_of(&page2).into_iter().map(|(s, _, _, _)| s).collect();
    assert!(
        first.iter().all(|s| !second.contains(s)),
        "pages must not overlap: {first:?} vs {second:?}"
    );
}

#[test]
fn map_rows_cap_their_list_columns() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "map"]);
    let mut saw_elision = false;
    for row in out.results() {
        let api = row["api"].as_str().unwrap();
        let parts: Vec<&str> = api.split(", ").collect();
        if api.contains("more)") {
            saw_elision = true;
            // 5 names plus one "... (+N more)" marker.
            assert_eq!(parts.len(), 6, "{api}");
            assert!(parts[5].starts_with("... (+"), "{api}");
        } else {
            assert!(parts.len() <= 5, "{api}");
        }

        let depends = row["depends_on"].as_str().unwrap();
        if !depends.is_empty() && !depends.contains("more)") {
            assert!(depends.split(", ").count() <= 5, "{depends}");
        }
    }
    assert!(
        saw_elision,
        "src/ has more than 5 API names, so elision must be exercised"
    );
}

// --- overview stays cheap -------------------------------------------------

/// §8 requires `overview` to keep its existing shape and cost; `map` is the
/// heavier command and must not have changed it.
#[test]
fn overview_output_is_unchanged_by_the_map_command() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "overview", "."]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(out.json()["query"]["kind"].as_str().unwrap(), "overview");
    // Root overview lists one level: 6 subdirectories + README.md.
    assert_eq!(out.json_len(), 7, "{}", out.stdout);
    let first = &out.results()[0];
    assert!(
        first.get("subsystem").is_none(),
        "overview rows are not map rows"
    );
}
