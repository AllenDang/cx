//! HTML navigation uses the original host identity throughout the CLI.
mod support;
use support::run_cx;

#[test]
fn html_navigation_relations_map_and_refresh() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    let path = dir.path().join("index.html");
    std::fs::write(&path, "é<script>function drop() { update(); }</script>\r\n<script type=module>import { helper } from './lib/helper'; function update() { drop(); helper(); }</script>\n<script>function update() {}</script>").unwrap();
    std::fs::create_dir(dir.path().join("lib")).unwrap();
    std::fs::write(
        dir.path().join("lib/helper.js"),
        "export function helper() {}\n",
    )
    .unwrap();
    for args in [
        vec!["--json", "overview", "index.html"],
        vec!["--json", "symbols", "--name", "update", "--all"],
        vec!["--json", "definition", "--name", "update", "--all"],
        vec!["--json", "references", "--name", "drop", "--all"],
    ] {
        let out = run_cx(dir.path(), &args);
        assert_eq!(out.code, 0, "{args:?}: {} {}", out.stdout, out.stderr);
        assert!(out.json_len() > 0, "{args:?}: {}", out.stdout);
        assert!(out.stdout.contains("index.html"), "{}", out.stdout);
    }
    let map = run_cx(dir.path(), &["--json", "map"]);
    assert_eq!(map.code, 0, "{}", map.stderr);
    assert!(
        map.results()
            .iter()
            .any(|r| r["depends_on"].as_str().is_some_and(|s| s.contains("lib"))),
        "{}",
        map.stdout
    );
    for args in [
        vec!["--json", "callers", "--name", "update", "--all"],
        vec!["--json", "callees", "--name", "drop", "--all"],
    ] {
        let out = run_cx(dir.path(), &args);
        assert_eq!(out.code, 0, "{}", out.stderr);
        let rows = out.results();
        let edge = rows.iter().find(|r| r["from"] == "drop").unwrap();
        assert_eq!(edge["to"], "");
        assert_eq!(edge["resolution"], "syntax");
        assert_eq!(edge["line"], 1);
        assert!(
            edge["ambiguous_candidates"]
                .as_str()
                .unwrap()
                .contains("update")
        );
    }
    std::fs::write(&path, "<p>Now empty</p>").unwrap();
    let out = run_cx(dir.path(), &["--json", "refresh", "index.html"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let out = run_cx(dir.path(), &["--json", "overview", "index.html"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.json_len(), 0);
}
