#!/usr/bin/env python3
"""Read-only input corpus; all indexing and incremental edits use a private copy.

macOS /usr/bin/time -l runner for the fixed ANGE Stage R gate. Not an A/B/C
held-out task benchmark. Counts are from ACCEPTANCE_TEST_STANDARD.md, not cx.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import tempfile
import time

SUBJECT = "validate_stmt_against_action_spec"
FILE = "src/engine/action/param_validator.cpp"
QUERIES = {
    "root_overview": ["overview", "."],
    "file_overview": ["overview", FILE],
    "definition": ["definition", "--name", SUBJECT, "--role", "definition", "--all"],
    "symbols": ["symbols", "--name", "EcsWorld"],
    "references": ["references", "--name", SUBJECT, "--all"],
    "callers": ["callers", "--name", SUBJECT, "--all"],
    "callees": ["callees", "--name", SUBJECT],
    "map": ["map", "--depth", "2"],
}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--grammars", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=9)
    args = parser.parse_args()
    if args.runs < 9:
        parser.error("at least 9 warm samples required")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    grammar_dir = args.grammars.resolve()
    report = {"grammars": {p.name: sha(p) for p in sorted(grammar_dir.glob("*")) if p.is_file()}, "arms": {}}
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    with tempfile.TemporaryDirectory(prefix="cx-stage-r-corpus-") as temp:
        corpus = Path(temp) / "corpus"
        shutil.copytree(args.corpus, corpus, symlinks=True, ignore=shutil.ignore_patterns(".git"))
        # Traverse before cold indexing, as prescribed by the acceptance standard.
        source_hashes = {str(p.relative_to(corpus)): sha(p) for p in corpus.rglob("*") if p.is_file() and not p.is_symlink()}
        report["corpus_manifest_sha256"] = hashlib.sha256(json.dumps(source_hashes, sort_keys=True).encode()).hexdigest()
        for arm, binary in binaries.items():
            dest = out / arm
            dest.mkdir()
            cache = dest / "cache"
            cache.mkdir()
            env = dict(os.environ, CX_CACHE_DIR=str(cache), TREE_SITTER_LANGUAGE_PACK_LIBS_DIR=str(grammar_dir),
                       TREE_SITTER_LANGUAGE_PACK_MANIFEST_URL="file:///nonexistent-cx-stage-r-manifest.json")
            steps = []

            def run(label, argv):
                command = ["/usr/bin/time", "-l", str(binary), "--root", str(corpus), "--json", *argv]
                start = time.perf_counter()
                result = subprocess.run(command, cwd=corpus, env=env, capture_output=True)
                wall = time.perf_counter() - start
                (dest / (label + ".stdout")).write_bytes(result.stdout)
                (dest / (label + ".stderr")).write_bytes(result.stderr)
                stderr = result.stderr.decode(errors="replace")
                rss = re.search(r"(\d+)\s+maximum resident set size", stderr)
                record = {"label": label, "argv": command, "exit_code": result.returncode, "wall": wall,
                          "rss": int(rss[1]) if rss else None, "stdout_bytes": len(result.stdout)}
                steps.append(record)
                (dest / (label + ".json")).write_text(json.dumps(record, indent=2) + "\n")
                if result.returncode:
                    raise RuntimeError(f"{arm}/{label}: exit {result.returncode}")
                return json.loads(result.stdout), record

            cold, cold_cost = run("cold", ["symbols", "--limit", "1"])
            assert cold["freshness"]["files_updated"] > 0
            assert cold["freshness"]["files_skipped_missing_grammar"] == 0
            db_bytes = sum(p.stat().st_size for p in cache.rglob("*.db"))
            task_cache_bytes = sum(p.stat().st_size for p in cache.rglob("*.tasks.zst"))
            total_index_bytes = db_bytes + task_cache_bytes
            measurements = {}
            generation = cold["freshness"]["generation"]
            for label, argv in QUERIES.items():
                warmup, _ = run(label + "-warmup", argv)
                if label in {"definition", "references", "callers"}:
                    # Default references groups occurrences by file: 28 syntax
                    # occurrences in 10 file rows, not 28 page rows.
                    assert warmup["page"]["total"] == {"definition": 1, "references": 10, "callers": 26}[label], (arm, label, warmup["page"])
                if label == "references":
                    assert sum(row["refs"] for row in warmup["results"]) == 28
                if label == "definition":
                    assert warmup["results"][0]["file"] == FILE
                if label == "callers":
                    assert all(row["evidence"] == "call" for row in warmup["results"])
                if label == "callees":
                    assert warmup["results"], "a declaration/definition mismatch must not look like a leaf"
                    assert len(json.dumps(warmup, indent=2).encode()) + 1 <= 16384
                    if arm == "candidate":
                        full, _ = run("callees-exact", ["callees", "--name", SUBJECT, "--all"])
                        # Independently counted in param_validator.cpp lines 333–398:
                        # line 362 calls key() twice; the old row dedup loses one.
                        expected = {337:2,338:2,346:3,347:2,348:2,353:2,354:1,356:1,
                            362:3,363:1,364:2,371:1,379:3,380:1,382:1,386:1,390:2,397:1}
                        actual = {}
                        for row in full["results"]:
                            assert row["file"] == FILE
                            actual[row["line"]] = actual.get(row["line"], 0) + 1
                        assert actual == expected
                        assert full["page"]["total"] == 31
                samples = []
                for n in range(args.runs):
                    doc, record = run(f"{label}-{n}", argv)
                    assert doc["results"] == warmup["results"]
                    assert doc["freshness"]["generation"] == generation
                    samples.append(record)
                walls = sorted(r["wall"] for r in samples)
                measurements[label] = {"median": statistics.median(walls), "p95": walls[math.ceil(.95 * len(walls)) - 1],
                    "max": max(walls), "rss": max(r["rss"] for r in samples), "stdout_bytes": samples[0]["stdout_bytes"]}
            # Default output budget checks (the all-results measurements above
            # are intentionally separate from the default page contract).
            for label in ["callers", "references"]:
                doc, record = run(label + "-default", [label, "--name", SUBJECT])
                assert record["stdout_bytes"] <= 16384
                assert doc["page"]["total"] == {"callers": 26, "references": 10}[label]
                if label == "references":
                    assert sum(row["refs"] for row in doc["results"]) == 28
            target = corpus / FILE
            before = target.read_bytes()
            incremental = {}
            for mode, argv in [
                ("metadata", ["overview", FILE]),
                ("paths", ["refresh", FILE]),
                ("verified", ["--fresh", "verified", "symbols", "--limit", "1"]),
            ]:
                try:
                    target.write_bytes(before + b"\n")
                    doc, cost = run("incremental-" + mode, argv)
                    assert doc["freshness"]["files_updated"] == 1
                    incremental[mode] = cost
                finally:
                    target.write_bytes(before)
                run("restore-" + mode, ["refresh", FILE])
            assert {str(p.relative_to(corpus)): sha(p) for p in corpus.rglob("*") if p.is_file() and not p.is_symlink()} == source_hashes
            report["arms"][arm] = {"binary_sha256": sha(binary), "cold": cold_cost, "db_bytes": db_bytes,
                "task_cache_bytes": task_cache_bytes, "total_index_bytes": total_index_bytes,
                "warm": measurements, "incremental": incremental, "steps": steps}
            (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
            print(arm, json.dumps({"cold": cold_cost["wall"], "db_bytes": db_bytes, "task_cache_bytes": task_cache_bytes, "total_index_bytes": total_index_bytes, "warm": measurements}), flush=True)
    print("source copy restored byte-for-byte and removed; original corpus not modified")


if __name__ == "__main__":
    main()
