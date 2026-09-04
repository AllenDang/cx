//! Shared helpers for integration tests: fixture corpus materialization and
//! command construction.
//!
//! Fixture trees live under `tests/fixtures/`.  That directory carries a
//! `.cx-ignore` marker so cx never indexes fixtures while indexing the cx
//! repository itself; the marker is dropped when a fixture is copied into a
//! temp project.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Path to the checked-in fixture corpus directory.
pub fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Copy a fixture tree into a fresh temp project with a `.git` marker so cx
/// resolves it as the project root.  Returns the temp dir (dropped = removed).
pub fn fixture_project(name: &str) -> tempfile::TempDir {
    let src = fixture_dir(name);
    assert!(src.is_dir(), "missing fixture: {}", src.display());
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&src, dir.path());
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    dir
}

/// Recursively copy `src` into `dst`, skipping cx ignore markers.
fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == ".cx-ignore" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
        }
    }
}

/// A `cx` command that runs with `dir` as the working directory.
pub fn cx_in(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cx"));
    cmd.current_dir(dir);
    cmd
}

/// Captured output of one cx invocation.
pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// Parse stdout as JSON, failing with both streams on error.
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| {
            panic!(
                "stdout is not valid JSON ({e})\n--- stdout ---\n{}\n--- stderr ---\n{}",
                self.stdout, self.stderr
            )
        })
    }

    /// Number of elements when stdout is a JSON array.
    pub fn json_len(&self) -> usize {
        match self.json() {
            serde_json::Value::Array(a) => a.len(),
            other => panic!("expected JSON array, got: {other}"),
        }
    }
}

/// Run cx in `dir` with `args` and capture the result.
pub fn run_cx(dir: &Path, args: &[&str]) -> Run {
    let out = cx_in(dir).args(args).output().unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    }
}

/// Set a file's mtime far enough in the future that coarse-granularity
/// filesystems still register the change.
pub fn touch_future(path: &Path) {
    let future = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(future))
        .unwrap();
}
