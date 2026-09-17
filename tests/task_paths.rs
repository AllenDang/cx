//! Task wire paths have the same separators in compact/full output on every OS.
mod support;
use serde_json::Value;
use std::fs;
use std::process::Command;
use support::run_cx;

fn check_paths(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                if matches!(name.as_str(), "file" | "before_file" | "after_file")
                    && let Some(path) = value.as_str()
                {
                    assert!(!path.contains('\\'), "non-portable wire path: {path}");
                }
                check_paths(value);
            }
        }
        Value::Array(items) => items.iter().for_each(check_paths),
        _ => {}
    }
}

#[test]
fn nested_task_paths_are_portable_in_full_and_compact_reports() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let file = project.path().join("src/code.rs");
    fs::write(&file, "fn leaf() { let n = 1; }\nfn caller() { leaf(); }\n").unwrap();
    let git = |args: &[&str]| {
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
                "-c",
                "core.autocrlf=false",
            ])
            .args(args)
            .current_dir(project.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-q"]);
    git(&["add", "--all"]);
    git(&["commit", "-qm", "base"]);
    fs::write(&file, "fn leaf() { let n = 2; }\nfn caller() { leaf(); }\n").unwrap();
    for detail in ["full", "compact"] {
        for query in [
            vec!["--json", "context", "--query", "leaf", "--detail", detail],
            vec![
                "--json",
                "impact",
                "--name",
                "leaf",
                "--file",
                "src/code.rs",
                "--detail",
                detail,
            ],
            vec!["--json", "changes", "--impact", "--detail", detail],
        ] {
            let out = run_cx(project.path(), &query);
            assert_eq!(out.code, 0, "{} {}", out.stdout, out.stderr);
            assert!(out.json_len() > 0, "{}", out.stdout);
            let value: Value = serde_json::from_str(&out.stdout).unwrap();
            check_paths(&value);
        }
    }
}
