//! Phase 1: one canonical identity for project root, cache key, index root and
//! query paths (roadmap §4.1, §11 Phase 1).
//!
//! macOS `/tmp` → `/private/tmp` and any symlinked checkout produce two names
//! for the same directory. cx must treat them as one project: one cache file,
//! one set of relative paths, and path arguments accepted in either spelling.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use support::{cx_in, fixture_project, run_cx};

/// A project plus an equivalent alias path that points at the same directory
/// through a symlink.  Returns (real_root, alias_root, keepalive dirs).
fn aliased_project() -> (PathBuf, PathBuf, (tempfile::TempDir, tempfile::TempDir)) {
    let real = tempfile::tempdir().unwrap();
    let link_home = tempfile::tempdir().unwrap();

    fs::create_dir_all(real.path().join("src")).unwrap();
    fs::create_dir_all(real.path().join(".git")).unwrap();
    fs::write(
        real.path().join("src/a.rs"),
        "pub fn alias_fn() -> u32 { 1 }\npub struct AliasType;\n",
    )
    .unwrap();

    let alias = link_home.path().join("alias");
    #[cfg(unix)]
    std::os::unix::fs::symlink(real.path(), &alias).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(real.path(), &alias).unwrap();

    // The real path as the OS reports it after resolving every symlink
    // (temp dirs themselves live under /var → /private/var on macOS).
    let canonical = fs::canonicalize(real.path()).unwrap();
    (canonical, alias, (real, link_home))
}

fn cache_path(root: &Path) -> String {
    let out = cx_in(Path::new("/"))
        .args(["--root", root.to_str().unwrap(), "cache", "path"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn aliased_roots_share_one_cache_file() {
    let (real, alias, _keep) = aliased_project();
    assert_eq!(
        cache_path(&real),
        cache_path(&alias),
        "symlinked alias must hash to the same index"
    );
}

#[test]
fn index_built_under_alias_is_reused_under_real_root() {
    let (real, alias, _keep) = aliased_project();

    let build = run_cx(
        &alias,
        &["--root", alias.to_str().unwrap(), "symbols", "--all"],
    );
    assert_eq!(build.code, 0, "stderr: {}", build.stderr);
    assert!(
        build.stderr.contains("indexing"),
        "first run must build: {}",
        build.stderr
    );

    let reuse = run_cx(
        &real,
        &["--root", real.to_str().unwrap(), "symbols", "--all"],
    );
    assert_eq!(reuse.code, 0, "stderr: {}", reuse.stderr);
    assert!(
        !reuse.stderr.contains("indexing") && !reuse.stderr.contains("updating"),
        "alias and real root must share one fresh index, got: {}",
        reuse.stderr
    );
    assert_eq!(
        build.stdout, reuse.stdout,
        "identical results in both spellings"
    );
}

#[test]
fn absolute_path_argument_in_the_other_spelling_resolves() {
    let (real, alias, _keep) = aliased_project();

    // Build under the alias, then query using a canonical absolute file path.
    let _ = run_cx(
        &alias,
        &["--root", alias.to_str().unwrap(), "symbols", "--all"],
    );

    let real_file = real.join("src/a.rs");
    let out = run_cx(
        &alias,
        &[
            "--root",
            alias.to_str().unwrap(),
            "--json",
            "overview",
            real_file.to_str().unwrap(),
        ],
    );
    assert_eq!(
        out.code, 0,
        "canonical path under an alias root must resolve\nstderr: {}",
        out.stderr
    );
    assert_eq!(out.json_len(), 2, "{}", out.stdout);

    // ...and the reverse: alias-spelled file path under the canonical root.
    let alias_file = alias.join("src/a.rs");
    let out = run_cx(
        &real,
        &[
            "--root",
            real.to_str().unwrap(),
            "--json",
            "overview",
            alias_file.to_str().unwrap(),
        ],
    );
    assert_eq!(
        out.code, 0,
        "alias path under a canonical root must resolve\nstderr: {}",
        out.stderr
    );
    assert_eq!(out.json_len(), 2, "{}", out.stdout);
}

#[test]
fn path_filters_accept_either_spelling() {
    let (real, alias, _keep) = aliased_project();
    let _ = run_cx(
        &alias,
        &["--root", alias.to_str().unwrap(), "symbols", "--all"],
    );

    let file = real.join("src/a.rs");
    for (root, label) in [(&alias, "alias root"), (&real, "real root")] {
        let refs = run_cx(
            root,
            &[
                "--root",
                root.to_str().unwrap(),
                "--json",
                "references",
                "--name",
                "alias_fn",
                "--file",
                file.to_str().unwrap(),
                "--all",
            ],
        );
        assert_eq!(refs.code, 0, "{label}: stderr: {}", refs.stderr);
        assert_eq!(refs.json_len(), 1, "{label}: {}", refs.stdout);

        let def = run_cx(
            root,
            &[
                "--root",
                root.to_str().unwrap(),
                "--json",
                "definition",
                "--name",
                "alias_fn",
                "--from",
                file.to_str().unwrap(),
            ],
        );
        assert_eq!(def.code, 0, "{label}: stderr: {}", def.stderr);
        assert_eq!(def.json_len(), 1, "{label}: {}", def.stdout);
    }
}

#[test]
fn results_use_root_relative_paths_regardless_of_spelling() {
    let (real, alias, _keep) = aliased_project();
    for root in [&alias, &real] {
        let out = run_cx(
            root,
            &[
                "--root",
                root.to_str().unwrap(),
                "--json",
                "symbols",
                "--all",
            ],
        );
        assert_eq!(out.code, 0, "stderr: {}", out.stderr);
        let rows = out.json();
        for row in rows.as_array().unwrap() {
            assert_eq!(row["file"].as_str().unwrap(), "src/a.rs", "{}", out.stdout);
        }
    }
}

#[test]
fn missing_file_error_reports_a_root_relative_path() {
    let p = fixture_project("agent_corpus");
    let out = run_cx(p.path(), &["symbols", "--file", "src/does_not_exist.cpp"]);
    assert_eq!(out.code, 1);
    assert!(
        out.stderr
            .contains("file not in index: src/does_not_exist.cpp"),
        "error must name the path relative to the project root, got: {}",
        out.stderr
    );
}

#[test]
fn dot_and_dotdot_root_spellings_share_one_index() {
    let p = fixture_project("agent_corpus");
    let direct = cache_path(p.path());
    let dotted = cache_path(&p.path().join("src/.."));
    let with_curdir = cache_path(&p.path().join("."));
    assert_eq!(direct, dotted);
    assert_eq!(direct, with_curdir);
}
