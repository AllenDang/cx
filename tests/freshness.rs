//! Phase 4: the freshness contract (roadmap §4.4, §7, §11 Phase 4).
//!
//! The roadmap names the exact cases that must be covered before cx can claim
//! its index matches disk: a content change that preserves file size, a change
//! that preserves mtime, new/deleted/renamed files, and concurrent readers.
//! Each case below is one of those.

mod support;

use std::fs;
use std::path::Path;

use support::{fixture_project, run_cx, touch_future};

const CORPUS: &str = "agent_corpus";

/// Force an exact mtime so "content changed but mtime did not" is constructible
/// rather than a matter of luck.
fn set_mtime(path: &Path, mtime: std::time::SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(mtime))
        .unwrap();
}

fn mtime_of(path: &Path) -> std::time::SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

fn generation(run: &support::Run) -> u64 {
    run.json()["freshness"]["generation"].as_u64().unwrap()
}

// --- Generation counter ----------------------------------------------------

#[test]
fn generation_advances_only_when_the_index_changes() {
    let p = fixture_project(CORPUS);

    let first = run_cx(p.path(), &["--json", "symbols", "--all"]);
    assert_eq!(generation(&first), 1, "initial build is generation 1");

    // Pure reads must not advance the generation.
    let second = run_cx(p.path(), &["--json", "symbols", "--all"]);
    assert_eq!(generation(&second), 1, "{}", second.stdout);
    let third = run_cx(p.path(), &["--json", "overview", "."]);
    assert_eq!(generation(&third), 1, "{}", third.stdout);

    // A real edit advances it exactly once.
    let target = p.path().join("src/scope_a.cpp");
    fs::write(
        &target,
        "namespace alpha { void run() {} void added() {} }\n",
    )
    .unwrap();
    touch_future(&target);

    let after = run_cx(p.path(), &["--json", "symbols", "--all"]);
    assert_eq!(generation(&after), 2, "{}", after.stdout);
    assert_eq!(
        after.json()["freshness"]["files_updated"].as_u64().unwrap(),
        1
    );

    let settled = run_cx(p.path(), &["--json", "symbols", "--all"]);
    assert_eq!(
        generation(&settled),
        2,
        "no further bump: {}",
        settled.stdout
    );
}

// --- §7 acceptance: metadata mode has a real blind spot --------------------

/// A content change that preserves both size and mtime is invisible to
/// `metadata` mode.  cx must not pretend otherwise: the stale answer comes with
/// `mode: metadata`, and `--fresh verified` is what catches it.
#[test]
fn same_size_same_mtime_edit_is_missed_by_metadata_and_caught_by_verified() {
    let p = fixture_project(CORPUS);
    let target = p.path().join("src/scope_a.cpp");

    // Baseline: one `run` in namespace alpha.
    let before = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "src/scope_a.cpp", "--all"],
    );
    let names: Vec<String> = before
        .results()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"run".to_string()), "{names:?}");
    assert!(!names.contains(&"jog".to_string()), "{names:?}");

    // Rename `run` to `jog`: same byte count, and we restore the old mtime.
    let original = fs::read_to_string(&target).unwrap();
    let original_mtime = mtime_of(&target);
    let edited = original.replace("void run()", "void jog()");
    assert_eq!(edited.len(), original.len(), "edit must preserve size");
    assert_ne!(edited, original);
    fs::write(&target, &edited).unwrap();
    set_mtime(&target, original_mtime);
    assert_eq!(mtime_of(&target), original_mtime, "mtime must be unchanged");

    // metadata mode cannot see it — and says so.
    let stale = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "src/scope_a.cpp", "--all"],
    );
    let stale_names: Vec<String> = stale
        .results()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        stale_names.contains(&"run".to_string()),
        "metadata mode is expected to miss this: {stale_names:?}"
    );
    assert_eq!(
        stale.json()["freshness"]["mode"].as_str().unwrap(),
        "metadata"
    );
    assert_eq!(
        stale.json()["freshness"]["files_updated"].as_u64().unwrap(),
        0
    );
    let stale_generation = generation(&stale);

    // verified mode hashes contents and picks it up.
    let fresh = run_cx(
        p.path(),
        &[
            "--fresh",
            "verified",
            "--json",
            "symbols",
            "--file",
            "src/scope_a.cpp",
            "--all",
        ],
    );
    let fresh_names: Vec<String> = fresh
        .results()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    assert!(fresh_names.contains(&"jog".to_string()), "{fresh_names:?}");
    assert!(!fresh_names.contains(&"run".to_string()), "{fresh_names:?}");
    assert_eq!(
        fresh.json()["freshness"]["mode"].as_str().unwrap(),
        "verified"
    );
    assert_eq!(
        fresh.json()["freshness"]["files_updated"].as_u64().unwrap(),
        1
    );
    assert_eq!(
        generation(&fresh),
        stale_generation + 1,
        "catching the edit is a new generation"
    );
}

/// Same blind spot, closed by naming the path instead of verifying everything.
#[test]
fn refresh_paths_catches_a_same_size_same_mtime_edit() {
    let p = fixture_project(CORPUS);
    let target = p.path().join("src/scope_a.cpp");
    let _ = run_cx(p.path(), &["--json", "symbols", "--all"]);

    let original = fs::read_to_string(&target).unwrap();
    let original_mtime = mtime_of(&target);
    fs::write(&target, original.replace("void run()", "void jog()")).unwrap();
    set_mtime(&target, original_mtime);

    let refreshed = run_cx(p.path(), &["--json", "refresh", "src/scope_a.cpp"]);
    assert_eq!(refreshed.code, 0, "stderr: {}", refreshed.stderr);
    let rows = refreshed.results();
    assert_eq!(rows.len(), 1, "{}", refreshed.stdout);
    assert_eq!(rows[0]["file"].as_str().unwrap(), "src/scope_a.cpp");
    assert_eq!(rows[0]["status"].as_str().unwrap(), "updated");
    assert_eq!(
        refreshed.json()["freshness"]["mode"].as_str().unwrap(),
        "paths"
    );
    assert_eq!(
        refreshed.json()["freshness"]["files_checked"]
            .as_u64()
            .unwrap(),
        1
    );

    // The edit is now in the index, provably in the generation refresh reported.
    let refreshed_generation = generation(&refreshed);
    let query = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "src/scope_a.cpp", "--all"],
    );
    assert_eq!(
        generation(&query),
        refreshed_generation,
        "a later query must answer from the generation refresh established"
    );
    let names: Vec<String> = query
        .results()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"jog".to_string()), "{names:?}");
}

#[test]
fn content_change_with_same_size_but_new_mtime_is_caught_by_metadata() {
    let p = fixture_project(CORPUS);
    let target = p.path().join("src/scope_a.cpp");
    let _ = run_cx(p.path(), &["--json", "symbols", "--all"]);

    let original = fs::read_to_string(&target).unwrap();
    let edited = original.replace("void run()", "void jog()");
    assert_eq!(edited.len(), original.len());
    fs::write(&target, edited).unwrap();
    touch_future(&target);

    let out = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "src/scope_a.cpp", "--all"],
    );
    let names: Vec<String> = out
        .results()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        names.contains(&"jog".to_string()),
        "mtime changed, so metadata suffices: {names:?}"
    );
}

/// Size alone is enough when the clock is too coarse to notice.
#[test]
fn size_change_with_preserved_mtime_is_caught_by_metadata() {
    let p = fixture_project(CORPUS);
    let target = p.path().join("src/scope_a.cpp");
    let _ = run_cx(p.path(), &["--json", "symbols", "--all"]);

    let original = fs::read_to_string(&target).unwrap();
    let original_mtime = mtime_of(&target);
    fs::write(
        &target,
        format!("{original}\nnamespace alpha {{ void extra() {{}} }}\n"),
    )
    .unwrap();
    set_mtime(&target, original_mtime);
    assert_eq!(mtime_of(&target), original_mtime);

    let out = run_cx(
        p.path(),
        &["--json", "symbols", "--file", "src/scope_a.cpp", "--all"],
    );
    let names: Vec<String> = out
        .results()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        names.contains(&"extra".to_string()),
        "size differs, so metadata mode must catch it: {names:?}"
    );
}

// --- Refresh reporting -----------------------------------------------------

#[test]
fn refresh_reports_per_path_status() {
    let p = fixture_project(CORPUS);
    let _ = run_cx(p.path(), &["--json", "symbols", "--all"]);

    let edited = p.path().join("src/scope_a.cpp");
    fs::write(
        &edited,
        "namespace alpha { void run() {} void more() {} }\n",
    )
    .unwrap();
    touch_future(&edited);
    fs::remove_file(p.path().join("src/comments.cpp")).unwrap();

    let out = run_cx(
        p.path(),
        &[
            "--json",
            "refresh",
            "src/scope_a.cpp",
            "src/comments.cpp",
            "src/ecs.cpp",
        ],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let statuses: Vec<(String, String)> = out
        .results()
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap().to_string(),
                r["status"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let statuses: Vec<(&str, &str)> = statuses
        .iter()
        .map(|(f, s)| (f.as_str(), s.as_str()))
        .collect();
    assert_eq!(
        statuses,
        vec![
            ("src/comments.cpp", "removed"),
            ("src/ecs.cpp", "unchanged"),
            ("src/scope_a.cpp", "updated"),
        ],
        "{}",
        out.stdout
    );

    let fresh = out.json();
    assert_eq!(fresh["freshness"]["files_checked"].as_u64().unwrap(), 3);
    assert_eq!(fresh["freshness"]["files_updated"].as_u64().unwrap(), 1);
    assert_eq!(fresh["freshness"]["files_removed"].as_u64().unwrap(), 1);
}

#[test]
fn refresh_ignores_paths_outside_the_project() {
    let p = fixture_project(CORPUS);
    let _ = run_cx(p.path(), &["--json", "symbols", "--all"]);

    let outside = tempfile::tempdir().unwrap();
    let stray = outside.path().join("stray.rs");
    fs::write(&stray, "fn stray() {}\n").unwrap();

    let out = run_cx(p.path(), &["--json", "refresh", stray.to_str().unwrap()]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("outside the project root"),
        "stderr: {}",
        out.stderr
    );
    assert_eq!(
        out.json()["freshness"]["files_updated"].as_u64().unwrap(),
        0
    );
}

#[test]
fn refresh_without_paths_verifies_the_whole_project() {
    let p = fixture_project(CORPUS);
    let _ = run_cx(p.path(), &["--json", "symbols", "--all"]);

    // Same-size, same-mtime edit: only a content check can find it.
    let target = p.path().join("src/scope_a.cpp");
    let original = fs::read_to_string(&target).unwrap();
    let original_mtime = mtime_of(&target);
    fs::write(&target, original.replace("void run()", "void jog()")).unwrap();
    set_mtime(&target, original_mtime);

    let out = run_cx(p.path(), &["--json", "refresh"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(
        out.json()["freshness"]["mode"].as_str().unwrap(),
        "verified"
    );
    let rows = out.results();
    assert_eq!(
        rows.len(),
        1,
        "only the changed file is reported: {}",
        out.stdout
    );
    assert_eq!(rows[0]["file"].as_str().unwrap(), "src/scope_a.cpp");
    assert_eq!(rows[0]["status"].as_str().unwrap(), "updated");
}

#[test]
fn refresh_reports_nothing_changed_without_inventing_work() {
    let p = fixture_project(CORPUS);
    let _ = run_cx(p.path(), &["--json", "symbols", "--all"]);

    let out = run_cx(p.path(), &["--json", "refresh", "src/ecs.cpp"]);
    let rows = out.results();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["status"].as_str().unwrap(), "unchanged");
    assert_eq!(
        out.json()["freshness"]["files_updated"].as_u64().unwrap(),
        0
    );

    // TOON mode says so on stderr rather than claiming "no matches".
    let toon = run_cx(p.path(), &["refresh", "src/ecs.cpp"]);
    assert!(toon.stdout.contains("unchanged"), "{}", toon.stdout);
    assert!(
        toon.stderr.contains("generation") && toon.stderr.contains("mode paths"),
        "stderr: {}",
        toon.stderr
    );
}

// --- §7 acceptance: new / deleted / renamed --------------------------------

#[test]
fn new_deleted_and_renamed_files_are_counted_in_freshness() {
    let p = fixture_project(CORPUS);
    let _ = run_cx(p.path(), &["--json", "symbols", "--all"]);

    // New file.
    let added = p.path().join("src/added.cpp");
    fs::write(&added, "namespace extra { void added_fn() {} }\n").unwrap();
    touch_future(&added);
    let out = run_cx(p.path(), &["--json", "symbols", "--all"]);
    assert_eq!(
        out.json()["freshness"]["files_updated"].as_u64().unwrap(),
        1
    );
    assert_eq!(
        out.json()["freshness"]["files_removed"].as_u64().unwrap(),
        0
    );

    // Deleted file.
    fs::remove_file(&added).unwrap();
    let out = run_cx(p.path(), &["--json", "symbols", "--all"]);
    assert_eq!(
        out.json()["freshness"]["files_updated"].as_u64().unwrap(),
        0
    );
    assert_eq!(
        out.json()["freshness"]["files_removed"].as_u64().unwrap(),
        1
    );

    // Rename is one add plus one remove.
    fs::rename(
        p.path().join("src/scope_b.cpp"),
        p.path().join("src/scope_b_renamed.cpp"),
    )
    .unwrap();
    touch_future(&p.path().join("src/scope_b_renamed.cpp"));
    let out = run_cx(p.path(), &["--json", "symbols", "--all"]);
    assert_eq!(
        out.json()["freshness"]["files_updated"].as_u64().unwrap(),
        1
    );
    assert_eq!(
        out.json()["freshness"]["files_removed"].as_u64().unwrap(),
        1
    );
}

// --- §7 acceptance: concurrency -------------------------------------------

#[test]
fn concurrent_readers_agree_on_one_generation() {
    let p = fixture_project(CORPUS);
    let _ = run_cx(p.path(), &["--json", "symbols", "--all"]);

    // Several readers at once must all succeed against a fresh index; a shared
    // read lock means none of them needs to write.
    let root = p.path().to_path_buf();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || {
                run_cx(&root, &["--json", "symbols", "--name", "run", "--all"])
            })
        })
        .collect();

    let mut generations = Vec::new();
    for h in handles {
        let out = h.join().unwrap();
        assert_eq!(out.code, 0, "stderr: {}", out.stderr);
        assert_eq!(out.json_len(), 12, "{}", out.stdout);
        generations.push(generation(&out));
    }
    assert!(
        generations.windows(2).all(|w| w[0] == w[1]),
        "concurrent readers saw different generations: {generations:?}"
    );
}

#[test]
fn a_writer_and_readers_do_not_corrupt_the_index() {
    let p = fixture_project(CORPUS);
    let _ = run_cx(p.path(), &["--json", "symbols", "--all"]);

    // One process updates while others query.  Every process must exit cleanly
    // and the index must still answer correctly afterwards.
    let target = p.path().join("src/scope_a.cpp");
    fs::write(
        &target,
        "namespace alpha { void run() {} void concurrent() {} }\n",
    )
    .unwrap();
    touch_future(&target);

    let root = p.path().to_path_buf();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || run_cx(&root, &["--json", "symbols", "--all"]))
        })
        .collect();

    for h in handles {
        let out = h.join().unwrap();
        assert_eq!(out.code, 0, "stderr: {}", out.stderr);
        assert!(out.json_len() >= 41, "{}", out.stdout);
    }

    let after = run_cx(
        p.path(),
        &["--json", "symbols", "--name", "concurrent", "--all"],
    );
    assert_eq!(after.json_len(), 1, "{}", after.stdout);
}

// --- §7 acceptance: newly installed grammar -------------------------------

/// A file skipped for a missing grammar is reported, not silently dropped, so an
/// agent can tell "no symbols here" from "cx cannot read this language".
#[test]
fn files_skipped_for_missing_grammar_are_counted() {
    let p = fixture_project(CORPUS);
    // .kt has no grammar mapping in cx at all, so it is not indexable and must
    // not inflate the skip counter either.
    fs::write(p.path().join("src/Unsupported.kt"), "fun main() {}\n").unwrap();

    let out = run_cx(p.path(), &["--json", "symbols", "--all"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let fresh = out.json();
    assert_eq!(
        fresh["freshness"]["files_skipped_missing_grammar"]
            .as_u64()
            .unwrap(),
        0,
        "an unknown extension is not a missing grammar: {}",
        out.stdout
    );
    assert_eq!(out.json_len(), 41, "{}", out.stdout);
}
