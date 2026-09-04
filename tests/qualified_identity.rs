//! Phase 5: qualified symbol identity (roadmap §5.2, §5.3, §11 Phase 5).
//!
//! Acceptance condition: a symbol name that exists in two different scopes must
//! not be silently merged in definition or reference queries.  cx models Rust,
//! C/C++ and TypeScript scopes; anything else reports its scope as unresolved
//! rather than pretending the symbol is top-level.

mod support;

use support::{fixture_project, run_cx};

const CORPUS: &str = "agent_corpus";

/// (file, qualified) pairs for a symbols query, sorted for stable comparison.
///
/// `file` is omitted by cx when a query is scoped to a single file, so it is
/// read leniently here rather than unwrapped.
fn qualified_pairs(run: &support::Run) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = run
        .results()
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap_or_default().to_string(),
                r["qualified"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    pairs.sort();
    pairs
}

// --- §5.3 lexical scope for the three modelled language shapes -------------

#[test]
fn cpp_namespaces_classes_and_out_of_line_definitions_are_qualified() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "src/ecs.cpp", "--all"],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let pairs = qualified_pairs(&out);
    let qualified: Vec<&str> = pairs.iter().map(|(_, q)| q.as_str()).collect();

    // `void EcsWorld::run()` inside `namespace ange` combines the enclosing
    // namespace with the qualifier written at the definition site.
    assert!(qualified.contains(&"ange::EcsWorld::run"), "{qualified:?}");
    assert!(
        qualified.contains(&"ange::EcsWorld::entity_count"),
        "{qualified:?}"
    );
    assert!(qualified.contains(&"ange::validate_param"), "{qualified:?}");
    assert!(
        qualified.contains(&"ange"),
        "namespace itself: {qualified:?}"
    );
}

#[test]
fn cpp_in_class_declarations_are_qualified_by_their_class() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &[
            "--json",
            "symbols",
            "--file",
            "include/ange/ecs.hpp",
            "--all",
        ],
    );
    let qualified: Vec<String> = qualified_pairs(&out).into_iter().map(|(_, q)| q).collect();
    assert!(
        qualified.contains(&"ange::EcsWorld::run".to_string()),
        "{qualified:?}"
    );
    assert!(
        qualified.contains(&"ange::EcsWorld::entity_count".to_string()),
        "{qualified:?}"
    );
    // Free function in the namespace, not in the class.
    assert!(
        qualified.contains(&"ange::validate_param".to_string()),
        "{qualified:?}"
    );
}

#[test]
fn rust_modules_qualify_their_items() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "src/lib.rs", "--all"],
    );
    let qualified: Vec<String> = qualified_pairs(&out).into_iter().map(|(_, q)| q).collect();

    assert!(
        qualified.contains(&"alpha::run".to_string()),
        "{qualified:?}"
    );
    assert!(
        qualified.contains(&"beta::run".to_string()),
        "{qualified:?}"
    );
    assert!(
        qualified.contains(&"run_both".to_string()),
        "top level: {qualified:?}"
    );
    assert!(
        qualified.contains(&"tests::run_both_sums_scopes".to_string()),
        "test module qualifies its items: {qualified:?}"
    );
}

#[test]
fn typescript_classes_and_interfaces_qualify_their_members() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "src/app.ts", "--all"],
    );
    let qualified: Vec<String> = qualified_pairs(&out).into_iter().map(|(_, q)| q).collect();

    // TypeScript joins with `.`, not `::`.
    assert!(
        qualified.contains(&"Tickable.run".to_string()),
        "{qualified:?}"
    );
    assert!(
        qualified.contains(&"AlphaRunner.run".to_string()),
        "{qualified:?}"
    );
    assert!(
        qualified.contains(&"run".to_string()),
        "exported top-level function: {qualified:?}"
    );
}

/// Languages cx has not modelled report an empty qualified name, which means
/// "unresolved" and never "top level" (roadmap §5.3).
#[test]
fn unmodelled_languages_report_unresolved_scope() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "docs/design.md", "--all"],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = out.results();
    assert!(!rows.is_empty());
    for row in &rows {
        assert_eq!(
            row["qualified"].as_str().unwrap(),
            "",
            "markdown scopes are not modelled: {}",
            out.stdout
        );
    }
}

// --- §11 Phase 5 acceptance: no silent merging -----------------------------

/// The headline condition: twelve `run` locations resolve to ten distinct
/// qualified symbols, each individually addressable.
#[test]
fn same_name_symbols_in_different_scopes_are_distinguished() {
    let p = fixture_project(CORPUS);
    let out = run_cx(p.path(), &["--json", "symbols", "--name", "run", "--all"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let pairs = qualified_pairs(&out);
    assert_eq!(pairs.len(), 12, "{pairs:?}");
    assert_eq!(
        pairs,
        vec![
            ("generated/gen_api.cpp".to_string(), "gen::run".to_string()),
            (
                "include/ange/ecs.hpp".to_string(),
                "ange::EcsWorld::run".to_string()
            ),
            ("src/app.ts".to_string(), "AlphaRunner.run".to_string()),
            ("src/app.ts".to_string(), "Tickable.run".to_string()),
            ("src/app.ts".to_string(), "run".to_string()),
            ("src/ecs.cpp".to_string(), "ange::EcsWorld::run".to_string()),
            ("src/lib.rs".to_string(), "alpha::run".to_string()),
            ("src/lib.rs".to_string(), "beta::run".to_string()),
            ("src/scope_a.cpp".to_string(), "alpha::run".to_string()),
            (
                "src/scope_b.cpp".to_string(),
                "beta::Runner::run".to_string()
            ),
            (
                "src/scope_b.cpp".to_string(),
                "beta::Runner::run".to_string()
            ),
            (
                "vendor/thirdparty/blob.cpp".to_string(),
                "thirdparty::run".to_string()
            ),
        ],
        "every location resolves to a qualified identity"
    );

    // Distinct qualified names, ignoring the two decl/def pairs.
    let mut distinct: Vec<String> = pairs.iter().map(|(_, q)| q.clone()).collect();
    distinct.sort();
    distinct.dedup();
    // alpha::run appears in both lib.rs (Rust) and scope_a.cpp (C++): the same
    // spelling in two languages, which the stable id keeps apart by language.
    assert_eq!(distinct.len(), 9, "{distinct:?}");
}

#[test]
fn scope_filter_selects_one_scope_of_a_shared_name() {
    let p = fixture_project(CORPUS);

    let alpha = run_cx(
        p.path(),
        &[
            "--json", "symbols", "--name", "run", "--scope", "alpha::*", "--all",
        ],
    );
    assert_eq!(alpha.code, 0, "stderr: {}", alpha.stderr);
    let pairs = qualified_pairs(&alpha);
    assert_eq!(
        pairs,
        vec![
            ("src/lib.rs".to_string(), "alpha::run".to_string()),
            ("src/scope_a.cpp".to_string(), "alpha::run".to_string()),
        ],
        "{pairs:?}"
    );

    let nested = run_cx(
        p.path(),
        &[
            "--json",
            "symbols",
            "--name",
            "run",
            "--scope",
            "ange::EcsWorld::*",
            "--all",
        ],
    );
    let pairs = qualified_pairs(&nested);
    assert_eq!(pairs.len(), 2, "declaration + definition: {pairs:?}");
    assert!(
        pairs.iter().all(|(_, q)| q == "ange::EcsWorld::run"),
        "{pairs:?}"
    );
}

#[test]
fn scope_filter_narrows_definition_to_one_symbol() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &[
            "--json",
            "definition",
            "--name",
            "run",
            "--scope",
            "beta::Runner::*",
            "--all",
        ],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let rows = out.results();
    assert_eq!(rows.len(), 2, "declaration + definition\n{}", out.stdout);
    assert!(
        rows.iter()
            .all(|r| r["qualified"].as_str().unwrap() == "beta::Runner::run"),
        "{}",
        out.stdout
    );
    // One logical symbol, so no ambiguity warning despite two rows.
    assert!(
        out.json()["warnings"].as_array().unwrap().is_empty(),
        "{}",
        out.stdout
    );
    // Implementation first.
    assert_eq!(rows[0]["role"].as_str().unwrap(), "definition");
    assert_eq!(rows[1]["role"].as_str().unwrap(), "declaration");
}

/// A scope filter must not match symbols whose scope cx failed to resolve:
/// silence is better than a false positive.
#[test]
fn scope_filter_never_matches_unresolved_scopes() {
    let p = fixture_project(CORPUS);
    let out = run_cx(
        p.path(),
        &[
            "--json",
            "symbols",
            "--file",
            "docs/design.md",
            "--scope",
            "*",
            "--all",
        ],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(
        out.json_len(),
        0,
        "unresolved scope must not match a wildcard: {}",
        out.stdout
    );
}

#[test]
fn definition_output_carries_the_qualified_name() {
    let p = fixture_project(CORPUS);

    let json = run_cx(
        p.path(),
        &["--json", "definition", "--name", "validate_param", "--all"],
    );
    let rows = json.results();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|r| r["qualified"].as_str().unwrap() == "ange::validate_param"),
        "{}",
        json.stdout
    );

    // Plain-text output shows it too, and only when it is resolved.
    let text = run_cx(
        p.path(),
        &["definition", "--name", "validate_param", "--all"],
    );
    assert!(
        text.stdout.contains("qualified: ange::validate_param"),
        "{}",
        text.stdout
    );
    let unresolved = run_cx(p.path(), &["definition", "--name", "Design", "--all"]);
    assert!(
        !unresolved.stdout.contains("qualified:"),
        "unresolved scope prints no qualified line: {}",
        unresolved.stdout
    );
}

/// Two scopes with the same short name must remain separately addressable
/// through `--from` as well, which is the pre-existing narrowing tool.
#[test]
fn from_and_scope_agree_on_the_same_symbol() {
    let p = fixture_project(CORPUS);

    let by_file = run_cx(
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
    let by_scope = run_cx(
        p.path(),
        &[
            "--json",
            "definition",
            "--name",
            "run",
            "--scope",
            "alpha::run",
            "--from",
            "src/scope_a.cpp",
            "--all",
        ],
    );
    assert_eq!(by_file.json_len(), 1, "{}", by_file.stdout);
    assert_eq!(by_scope.json_len(), 1, "{}", by_scope.stdout);
    assert_eq!(
        by_file.results()[0]["body"],
        by_scope.results()[0]["body"],
        "both narrowings select the same definition"
    );
}
