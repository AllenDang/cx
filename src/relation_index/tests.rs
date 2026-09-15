//! Independent snapshot controls. The reader callback places mutations exactly
//! between index creation and read, or between read and parse; no sleep races.
use super::*;
use crate::index::{FreshnessMode, FreshnessRequest};
use serde_json::json;
use std::fs;

fn indexed(files: &[(&str, &str)]) -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    for (name, source) in files {
        fs::write(dir.path().join(name), source).unwrap();
    }
    let root = dir.path().canonicalize().unwrap();
    let index = Index::load_or_build(
        &root,
        &FreshnessRequest::new(FreshnessMode::Verified, vec![]),
    );
    assert!(index.refresh_error.is_none());
    assert_eq!(index.entries.len(), files.len());
    (dir, index)
}

#[test]
fn mutation_at_reader_barrier_never_produces_mixed_facts() {
    let (_dir, index) = indexed(&[("a.rs", "fn leaf() {}\nfn entry() { leaf(); }\n")]);
    let mut reads = 0;
    let graph = RelationIndex::build_with_reader(&index, |path| {
        reads += 1;
        fs::write(path, "fn leaf() {}\nfn other() { leaf(); }\n")?;
        fs::read(path)
    });
    assert_eq!(reads, 1);
    assert_eq!(graph.calls.len(), 0);
    assert!(!graph.trustworthy_candidates);
    assert_eq!(
        serde_json::to_value(&graph.coverage.issues).unwrap(),
        json!([{"file":"a.rs","reason":"content_changed"}])
    );
}

#[test]
fn deletion_at_reader_barrier_is_not_an_empty_complete_graph() {
    let (_dir, index) = indexed(&[("a.rs", "fn leaf() {}\n")]);
    let graph = RelationIndex::build_with_reader(&index, |path| {
        fs::remove_file(path)?;
        fs::read(path)
    });
    assert_eq!(graph.calls.len(), 0);
    assert!(!graph.coverage.complete_within_model);
    assert_eq!(
        serde_json::to_value(&graph.coverage.issues).unwrap(),
        json!([{"file":"a.rs","reason":"read_failed"}])
    );
}

#[test]
fn source_changed_after_read_uses_the_immutable_read_not_a_third_version() {
    let (_dir, index) = indexed(&[("a.rs", "fn leaf() {}\nfn entry() { leaf(); }\n")]);
    let mut reads = 0;
    let graph = RelationIndex::build_with_reader(&index, |path| {
        reads += 1;
        let bytes = fs::read(path)?;
        fs::write(path, "fn completely_different() {}\n")?;
        Ok(bytes)
    });
    assert_eq!(reads, 1);
    assert!(graph.coverage.complete_within_model);
    let sites = &graph.calls[Path::new("a.rs")];
    assert_eq!(sites.len(), 1);
    assert_eq!(
        (sites[0].name.as_str(), sites[0].line, sites[0].byte_offset),
        ("leaf", 2, 26) // includes the space after the opening brace
    );
    assert_eq!(graph.named("leaf")[0].id.range, (0, 12));
}

#[test]
fn failed_candidate_file_does_not_make_a_remaining_target_unique() {
    let (_dir, index) = indexed(&[
        ("a.rs", "fn leaf() {}\n"),
        ("b.rs", "fn leaf() {}\n"),
        ("entry.rs", "fn entry() { leaf(); }\n"),
    ]);
    let graph = RelationIndex::build_with_reader(&index, |path| {
        if path.file_name().unwrap() == "b.rs" {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        } else {
            fs::read(path)
        }
    });
    let site = &graph.calls[Path::new("entry.rs")][0];
    let result = graph.resolve(Path::new("entry.rs"), "rust", None, site);
    assert!(result.to.is_none());
    assert_eq!(
        result
            .candidates
            .iter()
            .map(|c| c.id.file.as_path())
            .collect::<Vec<_>>(),
        vec![Path::new("a.rs"), Path::new("b.rs")]
    );
}

#[test]
fn local_targets_have_distinct_typed_identities_and_stable_input_order() {
    let (_dir, mut index) = indexed(&[
        ("a.rs", "fn leaf() {}\nfn a() { leaf(); }\n"),
        ("b.rs", "fn leaf() {}\nfn b() { leaf(); }\n"),
    ]);
    let graph = RelationIndex::build(&index);
    let mut ids = Vec::new();
    for (file, offset) in [("a.rs", 22), ("b.rs", 22)] {
        let site = &graph.calls[Path::new(file)][0];
        assert_eq!(site.byte_offset, offset);
        let resolved = graph.resolve(Path::new(file), "rust", None, site);
        ids.push(resolved.to.unwrap().id.clone());
    }
    assert_ne!(ids[0], ids[1]);
    assert_eq!(
        ids.iter()
            .map(|id| (id.file.as_path(), id.language.as_str(), id.range))
            .collect::<Vec<_>>(),
        vec![
            (Path::new("a.rs"), "rust", (0, 12)),
            (Path::new("b.rs"), "rust", (0, 12))
        ]
    );
    let warning = graph.warning();
    drop(graph);
    let mut entries: Vec<_> = index.entries.drain().collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    index.entries.extend(entries);
    assert_eq!(RelationIndex::build(&index).warning(), warning);
}

#[test]
fn matching_cpp_declaration_retains_its_exact_source_site() {
    let (_dir, index) = indexed(&[
        ("api.hpp", "void leaf();\n"),
        ("api.cpp", "#include \"api.hpp\"\nvoid leaf() {}\n"),
    ]);
    let graph = RelationIndex::build(&index);
    let candidates = graph.named("leaf");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id.file, Path::new("api.cpp"));
    assert_eq!(
        candidates[0].declarations,
        vec![DefinitionSiteId {
            file: PathBuf::from("api.hpp"),
            language: "cpp".into(),
            range: (0, 12)
        }]
    );
}

#[test]
fn missing_grammar_count_prevents_complete_success() {
    let (_dir, mut index) = indexed(&[("a.rs", "fn leaf() {}\nfn a() { leaf(); }\n")]);
    // Model the index's explicit skip evidence, without mutating the shared
    // process-global grammar cache. CLI isolation control covers real skips.
    index.freshness.files_skipped_missing_grammar = 1;
    let graph = RelationIndex::build(&index);
    assert!(!graph.coverage.complete_within_model);
    assert_eq!(graph.coverage.files_skipped_missing_grammar, 1);
    let result = graph.resolve(
        Path::new("a.rs"),
        "rust",
        None,
        &graph.calls[Path::new("a.rs")][0],
    );
    assert!(result.to.is_none());
    assert_eq!(result.candidates.len(), 1);
}
