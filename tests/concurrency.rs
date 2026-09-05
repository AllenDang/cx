//! Cross-process acceptance for cold-cache agent queries.

use std::fs;
use std::process::{Command, Stdio};
use std::time::Duration;

fn command(root: &std::path::Path, cache: &std::path::Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cx"));
    cmd.current_dir(root)
        .env("CX_CACHE_DIR", cache)
        .args(args)
        .arg("--root")
        .arg(root)
        .arg("--json")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

#[test]
fn parallel_cold_cache_queries_both_succeed_on_one_generation() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join(".git")).unwrap();
    let src = project.path().join("src");
    fs::create_dir(&src).unwrap();
    for i in 0..1_200 {
        fs::write(
            src.join(format!("unit_{i}.rs")),
            format!("pub fn unit_{i}() {{}}\n"),
        )
        .unwrap();
    }
    let cache = tempfile::tempdir().unwrap();

    let first = command(
        project.path(),
        cache.path(),
        &["overview", ".", "--limit", "200"],
    )
    .spawn()
    .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let second = command(
        project.path(),
        cache.path(),
        &["map", "--depth", "2", "--limit", "80"],
    )
    .spawn()
    .unwrap();

    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    assert!(
        first.status.success(),
        "overview failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        second.status.success(),
        "map failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let overview: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    let map: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert!(!overview["results"].as_array().unwrap().is_empty());
    assert!(!map["results"].as_array().unwrap().is_empty());
    assert_eq!(
        overview["freshness"]["generation"], map["freshness"]["generation"],
        "parallel cold-cache queries must observe one committed generation"
    );
}
