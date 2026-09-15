#!/usr/bin/env python3
"""Stage R assertion-kill controls; only edits a disposable source copy.

Usage: python3 bench/task_analysis/mutations.py --output /tmp/cx-r-mutations
Logs commands, exit status, time, stdout/stderr, and actual failure blocks.
Compilation failures and unrelated test failures are NOT kills. No agents or
source checkout is launched; Cargo uses locked dependencies and test grammars.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
# Each patch is a deliberately wrong mechanism, not a modification to its oracle.
MUTATIONS = [
    ("collapse_site_identity", "src/relation_index.rs",
     "set.sort_by(|a, b| a.id.cmp(&b.id));",
     "set.sort_by(|a, b| a.id.cmp(&b.id));\nset.dedup_by(|a, b| a.label() == b.label());",
     ["--test", "relation_identity"], "overloads_with_identical_labels_remain_ambiguous"),
    ("choose_ambiguous_target", "src/relation_index.rs", "if set.len() == 1 {", "if !set.is_empty() {",
     ["--test", "relation_identity"], "overloads_with_identical_labels_remain_ambiguous"),
    ("ignore_content_identity", "src/relation_index.rs",
     "Ok(source) if content_hash(&source) != data.meta.content_hash =>",
     "Ok(source) if false && content_hash(&source) != data.meta.content_hash =>",
     ["--test", "relation_identity"], "metadata_blind_edit_cannot_mix_old_symbols_with_new_calls"),
    ("drop_coverage_disclosure", "src/relations.rs", "let mut warnings = vec![analysis.warning()];",
     "let mut warnings = Vec::new();", ["--test", "relation_coverage"],
     "valid_call_outside_parse_error_is_retained_as_partial_syntax"),
    ("accept_parse_recovery_as_complete", "src/relation_index.rs",
     "if facts.has_parse_errors {", "if false && facts.has_parse_errors {",
     ["--test", "relation_coverage"], "valid_call_outside_parse_error_is_retained_as_partial_syntax"),
    ("attribute_closure_to_outer", "src/relations.rs", "if site.anonymous_owner {", "if false && site.anonymous_owner {",
     ["--test", "relation_identity"], "closure_calls_are_not_attributed_to_enclosing_function"),
    ("discard_unrelated_declarations", "src/relation_index.rs",
     "set.sort_by(|a, b| a.id.cmp(&b.id));",
     "set.sort_by(|a, b| a.id.cmp(&b.id));\nset.retain(|c| c.symbol.role != SymbolRole::Declaration);",
     ["--test", "relation_identity"], "unrelated_definition_does_not_hide_a_declaration"),
    ("resolve_after_incomplete_candidate_scan", "src/relation_index.rs",
     'if !self.trustworthy_candidates || site.indirect || language == "html" {',
     'if site.indirect || language == "html" {', ["--bin", "cx"],
     "relation_index::tests::missing_grammar_count_prevents_complete_success"),
    ("resolve_receiver_and_macro_by_name", "src/relation_index.rs",
     'if !self.trustworthy_candidates || site.indirect || language == "html" {',
     'if !self.trustworthy_candidates || language == "html" {', ["--test", "relation_identity"],
     "receiver_and_macro_calls_never_become_unique_function_edges"),
    ("use_truncated_cached_declaration_signature", "src/language/extract.rs",
     "return String::from_utf8_lossy(text)\n            .trim()\n            .trim_end_matches(';')",
     "return String::from_utf8_lossy(text.split(|b| *b == b'\\n').next().unwrap_or(text))\n            .trim()\n            .trim_end_matches(';')",
     ["--test", "impact"], "cached_multiline_declaration_links_imported_call_to_definition"),
    ("remove_output_page_budget", "src/query.rs", "if pg.limit.is_some() {", "if false && pg.limit.is_some() {",
     ["--test", "relation_coverage"], "large_candidate_evidence_is_paginated_not_deleted"),
    ("count_builtin_cast_as_call", "src/language/extract.rs",
     "&& !is_builtin_cast(lang, callee, source)", "&& (true || !is_builtin_cast(lang, callee, source))",
     ["--bin", "cx"], "language::call_tests::cpp_casts_do_not_create_calls_but_operands_do"),
    ("drop_turbofish_function_head", "src/language/extract.rs",
     '"generic_function" => split_callee(node.child_by_field_name("function")?, source),',
     '"generic_function" => None,', ["--bin", "cx"], "language::call_tests::rust_turbofish_retains_function_and_receiver"),
    ("drop_reference_return_queries", "src/language/queries/cpp.rs",
     "(reference_declarator\n    (function_declarator", "(pointer_declarator\n    declarator: (function_declarator",
     ["--bin", "cx"], "language::call_tests::cpp_reference_return_definitions_and_declarations_keep_qualified_identity"),
    ("refuse_unique_body_for_unmatched_declaration", "src/relations.rs",
     "let hosts = if definitions.is_empty() {\n        matching\n    } else {\n        definitions\n    };",
     "let hosts = matching;", ["--test", "ange_regressions"], "unique_definition_body_does_not_require_alias_equivalence_proof"),
    ("drop_scoped_unresolved_frontier", "src/relations.rs",
     "!resolution.to.is_some_and(matches) && !resolution.candidates.iter().any(|c| matches(c))",
     "!resolution.to.is_some_and(matches)", ["--test", "ange_regressions"], "scoped_callers_keep_matching_unresolved_frontier_without_binding_it"),
    ("bury_critical_coverage_files", "src/relation_index.rs",
     "a.reason.cmp(&b.reason).then(a.file.cmp(&b.file))", "a.file.cmp(&b.file)",
     ["--test", "ange_regressions"], "critical_file_failures_survive_the_bounded_issue_sample"),
    ("accept_missing_task_sidecar", "src/index.rs",
     "let Ok(file) = fs::File::open(task_cache_path(root)) else {\n        return false;\n    };",
     "let Ok(file) = fs::File::open(task_cache_path(root)) else {\n        return true;\n    };",
     ["--bin", "cx"], "index::tests::missing_task_sidecar_rebuilds_lazily_without_base_generation_change"),
    ("impact_forward_instead_of_reverse", "src/impact.rs",
     "self.incoming\n                    .entry(target_node.id.clone())", "self.incoming\n                    .entry(caller.id.clone())",
     ["--test", "impact"], "reverse_chain_diamond_and_witnesses_are_exact"),
    ("impact_merge_cross_file_nodes", "src/impact.rs", "file: site.file.clone(),", "file: PathBuf::new(),",
     ["--test", "impact"], "same_named_sites_do_not_cross_file_boundaries_and_root_can_be_selected"),
    ("impact_walk_ambiguous_candidates", "src/impact.rs", "if let Some(target) = resolution.to {",
     "if let Some(target) = resolution.to.or_else(|| resolution.candidates.first().copied()) {",
     ["--test", "impact"], "unknown_receiver_is_a_frontier_not_an_impact_path"),
    ("impact_promote_possible_paths", "src/impact.rs", "let next_possible = possible || edge.possible_reason.is_some();", "let next_possible = false;",
     ["--test", "impact"], "syntax_only_paths_are_possible_not_supported"),
    ("impact_remove_visited", "src/impact.rs", "if states.contains_key(&key) {", "if false && states.contains_key(&key) {",
     ["--test", "impact"], "mutual_cycle_away_from_seed_keeps_minimum_distances"),
    ("impact_collapse_evidence_states", "src/impact.rs", "if states.contains_key(&key) {", "if states.keys().any(|(id, _)| *id == edge.caller) {",
     ["--test", "impact"], "short_possible_path_does_not_suppress_longer_supported_path"),
    ("impact_hide_depth_stop", "src/impact.rs", 'stops.insert("max_depth");', 'let _ = "max_depth";',
     ["--test", "impact"], "traversal_and_output_budgets_are_distinct"),
    ("task_ignore_source_identity", "src/impact.rs",
     "crate::index::content_hash(bytes) == self.index.entries[&root.id.file].meta.content_hash",
     "true",
     ["--test", "impact"], "snapshot_mismatch_and_scope_escape_are_structured_failures"),
    ("changes_ignore_old_symbols", "src/changes.rs", "let (mut before, bc) = entities(file, old);", "let (mut before, bc) = (Vec::<Entity>::new(), true);",
     ["--test", "changes"], "deleting_a_function_keeps_old_site_and_before_impact"),
    ("changes_bad_ref_becomes_head", "src/changes.rs", 'let rev = format!("{reference}^{{commit}}");', 'let rev = "HEAD^{commit}".to_string();',
     ["--test", "changes"], "unsafe_refs_non_git_and_unborn_head_are_failures_not_clean_reports"),
    ("context_remove_exact_name_priority", "src/context.rs", "if query == symbol.name || symbol.qualified_name.as_deref() == Some(query) {", "if false && (query == symbol.name || symbol.qualified_name.as_deref() == Some(query)) {",
     ["--test", "context"], "exact_name_beats_an_earlier_metadata_subword_twin"),
    ("context_fill_without_evidence", "src/context.rs", "if c.score > 0 {", "c.score += 1; if c.score > 0 {",
     ["--test", "context"], "exact_name_beats_body_noise_and_zero_evidence_stays_empty"),
    ("context_comments_become_code", "src/language/mod.rs", 'Some("comment_text")', 'Some("code_text")',
     ["--test", "context"], "subwords_and_comment_only_evidence_find_the_implementation"),
]


def run(work, output, name, argv, env):
    start = time.perf_counter()
    result = subprocess.run(argv, cwd=work, env=env, capture_output=True)
    record = {"argv": argv, "exit_code": result.returncode, "wall_seconds": time.perf_counter() - start}
    (output / (name + ".stdout")).write_bytes(result.stdout)
    (output / (name + ".stderr")).write_bytes(result.stderr)
    (output / (name + ".json")).write_text(json.dumps(record, indent=2) + "\n")
    return result, record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    records = []
    with tempfile.TemporaryDirectory(prefix="cx-r-mutations-") as tmp:
        work = Path(tmp) / "cx"
        work.mkdir()
        (work / ".git").mkdir()  # project-root unit tests require the marker
        for name in ["Cargo.toml", "Cargo.lock", "src", "tests"]:
            src, dst = ROOT / name, work / name
            if src.is_dir():
                shutil.copytree(src, dst)
            else:
                shutil.copy2(src, dst)
        env = dict(os.environ, CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", str(output / "target")))
        # Copied source mtimes can predate cached mutant builds. Never let Cargo
        # report a stale mutant as Fresh; clean only this package in this cache.
        clean, _ = run(work, output, "baseline-clean", ["cargo", "clean", "--package", "cx-cli"], env)
        if clean.returncode:
            raise SystemExit("INVALID: disposable package cache cleanup failed")
        result, _ = run(work, output, "baseline", ["cargo", "test", "--locked", "--bin", "cx",
            "--test", "relation_identity", "--test", "relation_coverage", "--test", "ange_regressions", "--test", "impact", "--test", "changes", "--test", "context"], env)
        if result.returncode:
            raise SystemExit("INVALID: baseline tests failed; see baseline logs")
        for name, path, old, new, target, test in MUTATIONS:
            file = work / path
            original = file.read_text()
            count = original.count(old)
            expected = {"drop_coverage_disclosure": 2, "drop_reference_return_queries": 3, "bury_critical_coverage_files": 2}.get(name, 1)
            if count != expected:
                raise SystemExit(f"INVALID: {name} patch has {count} matches, expected {expected}")
            file.write_text(original.replace(old, new))
            try:
                clean, _ = run(work, output, name + "-clean", ["cargo", "clean", "--package", "cx-cli"], env)
                if clean.returncode:
                    raise SystemExit("INVALID: disposable mutant cache cleanup failed")
                result, record = run(work, output, name,
                    ["cargo", "test", "--locked", *target, test, "--", "--exact"], env)
                text = result.stdout.decode(errors="replace")
                err = result.stderr.decode(errors="replace")
                killed = (result.returncode == 101 and f"test {test} ... FAILED" in text
                          and "panicked at" in text and "assertion" in text + err
                          and "1 failed" in text and "could not compile" not in err)
                # Disclosure deletion fails at the mandatory-field expectation.
                if name == "drop_coverage_disclosure":
                    killed = (result.returncode == 101 and f"test {test} ... FAILED" in text
                              and "mandatory relation_coverage disclosure missing" in text
                              and "1 failed" in text and "could not compile" not in err)
                if name == "accept_missing_task_sidecar":
                    killed = (result.returncode == 101 and f"test {test} ... FAILED" in text
                              and "called `Option::unwrap()` on a `None` value" in text
                              and "1 failed" in text and "could not compile" not in err)
                record.update(name=name, test=test, killed=killed,
                              source_sha256=hashlib.sha256(original.encode()).hexdigest(),
                              failure=text[text.find("failures:"):])
                records.append(record)
                print(name, "KILLED" if killed else "SURVIVED/INVALID", flush=True)
            finally:
                file.write_text(original)
    (output / "report.json").write_text(json.dumps(records, indent=2) + "\n")
    raise SystemExit(0 if all(r["killed"] for r in records) else 1)


if __name__ == "__main__":
    main()
