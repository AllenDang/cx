//! Frozen source/Git oracles. Every repository and edit is disposable.
mod support;
use serde_json::Value;
use std::fs;
use std::process::Command;
use support::run_cx;
fn git(root: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}
fn project() -> tempfile::TempDir {
    let p = tempfile::tempdir().unwrap();
    git(p.path(), &["init", "-q"]);
    // These tests compare raw Git blobs with working bytes, including CRLF.
    git(p.path(), &["config", "core.autocrlf", "false"]);
    fs::write(
        p.path().join("a.rs"),
        "fn leaf() { let x = 1; }\nfn entry() { leaf(); }\nfn untouched() {}\n",
    )
    .unwrap();
    git(p.path(), &["add", "--all"]);
    git(p.path(), &["commit", "-qm", "base"]);
    p
}
fn changes(p: &tempfile::TempDir, args: &[&str]) -> support::Run {
    let mut argv = vec!["--json", "changes", "--all", "--detail", "full"];
    argv.extend_from_slice(args);
    run_cx(p.path(), &argv)
}
fn names(row: &Value) -> Vec<String> {
    row["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            s["after"]["name"]
                .as_str()
                .or_else(|| s["before"]["name"].as_str())
                .unwrap()
                .into()
        })
        .collect()
}
#[test]
fn body_edit_maps_only_changed_function_and_uses_both_versions() {
    let p = project();
    fs::write(
        p.path().join("a.rs"),
        "fn leaf() { let x = 2; }\nfn entry() { leaf(); }\nfn untouched() {}\n",
    )
    .unwrap();
    let out = changes(&p, &[]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.json_len(), 1);
    let row = &out.results()[0];
    assert_eq!(names(row), vec!["leaf"]);
    assert_eq!(row["symbols"][0]["change"], "modified");
    assert_eq!(row["hunks"][0]["before_start"], 1);
    assert_eq!(row["hunks"][0]["after_start"], 1);
    assert!(!row["symbols"][0]["before"].is_null());
    assert!(!row["symbols"][0]["after"].is_null());
}
#[test]
fn deleting_a_function_keeps_old_site_and_before_impact() {
    let p = project();
    fs::write(
        p.path().join("a.rs"),
        "fn entry() { leaf(); }\nfn untouched() {}\n",
    )
    .unwrap();
    let out = changes(&p, &["--impact", "--max-depth", "2"]);
    assert_eq!(out.code, 0, "{} {}", out.stdout, out.stderr);
    let row = &out.results()[0];
    assert_eq!(names(row), vec!["leaf"]);
    assert_eq!(row["symbols"][0]["change"], "deleted");
    assert!(row["symbols"][0]["after"].is_null());
    assert_eq!(
        row["symbols"][0]["before_impact"]["results"][0]["symbol"]["name"],
        "entry"
    );
    assert_eq!(
        row["symbols"][0]["after_impact"]["status"],
        "not_applicable"
    );
}
#[test]
fn staged_and_working_modes_do_not_mix_content() {
    let p = project();
    fs::write(
        p.path().join("a.rs"),
        "fn leaf() { let x = 2; }\nfn entry() { leaf(); }\nfn untouched() {}\n",
    )
    .unwrap();
    git(p.path(), &["add", "--all"]);
    fs::write(
        p.path().join("a.rs"),
        "fn leaf() { let x = 1; }\nfn entry() { leaf(); }\nfn untouched() {}\n",
    )
    .unwrap();
    assert_eq!(changes(&p, &[]).json_len(), 0);
    let staged = changes(&p, &["--staged"]);
    assert_eq!(staged.code, 0);
    assert_eq!(staged.json_len(), 1);
    assert_eq!(staged.json()["analysis"]["comparison"], "staged");
}
#[test]
fn additions_and_file_level_changes_are_not_assigned_to_adjacent_functions() {
    let p = project();
    fs::write(p.path().join("a.rs"),"use std::fmt;\nfn leaf() { let x = 1; }\nfn entry() { leaf(); }\nfn untouched() {}\nfn added() {}\n").unwrap();
    let out = changes(&p, &[]);
    assert_eq!(out.code, 0);
    let row = &out.results()[0];
    assert_eq!(names(row), vec!["added"]);
    assert_eq!(row["symbols"][0]["change"], "added");
    assert_eq!(row["file_level"], true);
}
#[test]
fn direct_and_merge_base_comparisons_are_explicit() {
    let p = project();
    let base = git(p.path(), &["rev-parse", "HEAD"]);
    fs::write(p.path().join("left.rs"), "fn left() {}\n").unwrap();
    git(p.path(), &["add", "--all"]);
    git(p.path(), &["commit", "-qm", "left"]);
    let left = git(p.path(), &["rev-parse", "HEAD"]);
    git(p.path(), &["checkout", "-q", "--detach", &base]);
    fs::write(p.path().join("right.rs"), "fn right() {}\n").unwrap();
    git(p.path(), &["add", "--all"]);
    git(p.path(), &["commit", "-qm", "right"]);
    let direct = changes(&p, &["--base", &left, "--head", "HEAD"]);
    assert_eq!(direct.code, 0);
    assert_eq!(direct.json_len(), 2);
    let merged = changes(&p, &["--base", &left, "--head", "HEAD", "--merge-base"]);
    assert_eq!(merged.code, 0);
    assert_eq!(merged.json_len(), 1);
    assert_eq!(merged.results()[0]["file"], "right.rs");
}
#[test]
fn unsafe_refs_non_git_and_unborn_head_are_failures_not_clean_reports() {
    let p = project();
    let bad = changes(&p, &["--base", "HEAD;touch PWNED", "--head", "HEAD"]);
    assert_eq!(bad.code, 1);
    assert_eq!(bad.error_code().as_deref(), Some("git_error"));
    assert!(!p.path().join("PWNED").exists());
    let dir = tempfile::tempdir().unwrap();
    let out = run_cx(dir.path(), &["--json", "changes"]);
    assert_eq!(out.code, 1);
    git(dir.path(), &["init", "-q"]);
    assert_eq!(run_cx(dir.path(), &["--json", "changes"]).code, 1);
}
#[test]
fn nul_protocol_paths_and_non_source_changes_are_preserved() {
    let p = project();
    // Windows forbids control characters in file names; retain the newline
    // oracle on Unix and exercise a legal non-ASCII path on Windows.
    let unusual = if cfg!(windows) {
        "unicode-é.rs"
    } else {
        "line\nbreak.rs"
    };
    for file in ["space name.rs", unusual, "-dash.rs"] {
        fs::write(p.path().join(file), "fn added() {}\n").unwrap();
    }
    fs::write(p.path().join("binary.dat"), b"\0binary\xff").unwrap();
    git(p.path(), &["add", "--all"]);
    let out = changes(&p, &[]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.json_len(), 4);
    let mut files: Vec<_> = out
        .results()
        .iter()
        .map(|r| r["file"].as_str().unwrap().to_string())
        .collect();
    files.sort();
    let mut expected = vec!["-dash.rs", "binary.dat", unusual, "space name.rs"];
    expected.sort();
    assert_eq!(files, expected);
    assert_eq!(
        out.results()
            .iter()
            .find(|r| r["file"] == "binary.dat")
            .unwrap()["classification"],
        "binary"
    );
}

#[test]
fn repository_diff_filters_and_fsmonitor_are_never_executed() {
    let p = project();
    fs::write(
        p.path().join(".gitattributes"),
        "*.rs filter=poison diff=poison\n",
    )
    .unwrap();
    for key in [
        "filter.poison.clean",
        "diff.poison.command",
        "diff.poison.textconv",
        "core.fsmonitor",
    ] {
        git(p.path(), &["config", key, "touch PWNED"]);
    }
    fs::write(p.path().join("a.rs"), "fn leaf() {}\n").unwrap();
    let out = changes(&p, &[]);
    assert_eq!(out.code, 0, "{} {}", out.stdout, out.stderr);
    assert_eq!(out.json_len(), 1);
    assert!(!p.path().join("PWNED").exists());
}

#[cfg(unix)]
#[test]
fn executable_mode_and_symlink_targets_are_classified_not_followed() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let p = project();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.rs"), "fn secret() {}\n").unwrap();
    symlink(outside.path().join("secret.rs"), p.path().join("link.rs")).unwrap();
    git(p.path(), &["add", "--all"]);
    git(p.path(), &["commit", "-qm", "link"]);
    fs::set_permissions(p.path().join("a.rs"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_file(p.path().join("link.rs")).unwrap();
    symlink("missing-target", p.path().join("link.rs")).unwrap();
    let out = changes(&p, &[]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.json_len(), 2);
    let rows = out.results();
    let mode = rows.iter().find(|r| r["file"] == "a.rs").unwrap();
    assert_eq!(mode["before_mode"], "100644");
    assert_eq!(mode["after_mode"], "100755");
    assert_eq!(mode["classification"], "mode_only");
    let link = rows.iter().find(|r| r["file"] == "link.rs").unwrap();
    assert_eq!(link["classification"], "symlink");
    assert!(link["symbols"].as_array().unwrap().is_empty());
}

#[test]
fn identical_content_move_retains_old_and_new_symbol_locations() {
    let p = project();
    git(p.path(), &["mv", "a.rs", "renamed.rs"]);
    let out = changes(&p, &[]);
    assert_eq!(out.code, 0, "{} {}", out.stdout, out.stderr);
    assert_eq!(out.json_len(), 1);
    let row = &out.results()[0];
    assert_eq!(row["change"], "renamed");
    assert_eq!(row["before_file"], "a.rs");
    assert_eq!(row["after_file"], "renamed.rs");
    assert_eq!(row["symbols"].as_array().unwrap().len(), 3);
    assert_eq!(row["symbols"][0]["before"]["file"], "a.rs");
    assert_eq!(row["symbols"][0]["after"]["file"], "renamed.rs");
}

#[test]
fn distant_hunks_crlf_and_unicode_do_not_mark_untouched_neighbors() {
    let p = project();
    let original = "fn one() { let x = \"é\"; }\r\nfn stable() {}\r\nfn two() { let y = 1; }\r\n";
    fs::write(p.path().join("a.rs"), original).unwrap();
    git(p.path(), &["add", "--all"]);
    git(p.path(), &["commit", "-qm", "crlf"]);
    fs::write(
        p.path().join("a.rs"),
        original.replace("é", "中文").replace("y = 1", "y = 2"),
    )
    .unwrap();
    let out = changes(&p, &[]);
    assert_eq!(out.code, 0);
    let row = &out.results()[0];
    assert_eq!(names(row), vec!["one", "two"]);
    assert_eq!(row["hunks"].as_array().unwrap().len(), 2);
}

#[test]
fn same_line_unchanged_neighbor_is_not_changed() {
    let p = project();
    fs::write(
        p.path().join("a.rs"),
        "fn leaf() { let x=1; } fn stable() {}\n",
    )
    .unwrap();
    git(p.path(), &["add", "--all"]);
    git(p.path(), &["commit", "-qm", "same line"]);
    fs::write(
        p.path().join("a.rs"),
        "fn leaf() { let x=2; } fn stable() {}\n",
    )
    .unwrap();
    assert_eq!(names(&changes(&p, &[]).results()[0]), vec!["leaf"]);
}

#[test]
fn separate_changed_roots_do_not_reuse_previous_traversal_budgets() {
    let p = project();
    fs::write(
        p.path().join("a.rs"),
        "fn leaf() { let x = 2; }\nfn entry() { leaf(); leaf(); }\nfn untouched() {}\n",
    )
    .unwrap();
    let out = changes(&p, &["--impact"]);
    assert_eq!(out.code, 0);
    let row = &out.results()[0];
    assert_eq!(names(row), vec!["leaf", "entry"]);
    let entry = row["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["after"]["name"] == "entry")
        .unwrap();
    assert_eq!(entry["before_impact"]["analysis"]["edges_examined"], 0);
    assert_eq!(entry["after_impact"]["analysis"]["edges_examined"], 0);
}

#[test]
fn compact_default_omits_raw_hunks_and_hashes_but_keeps_two_sided_symbols() {
    let p = project();
    fs::write(
        p.path().join("a.rs"),
        "fn leaf() { let x = 2; }\nfn entry() { leaf(); }\nfn untouched() {}\n",
    )
    .unwrap();
    let out = run_cx(p.path(), &["--json", "changes", "--all"]);
    assert_eq!(out.code, 0);
    let row = &out.results()[0];
    assert!(row.get("hunks").is_none());
    assert!(row.get("before_hash").is_none());
    assert_eq!(row["symbols"][0]["before"]["name"], "leaf");
    assert_eq!(row["symbols"][0]["after"]["name"], "leaf");
}
