#!/usr/bin/env python3
"""Score frozen evidence packages after execution; never imported by the runner.

This measures retrieval sufficiency, NOT agent answer accuracy. Source evidence
can permit a human to reject a wrong raw graph; both outcomes remain recorded.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import statistics


def coverage(docs):
    for doc in docs:
        for warning in doc.get("warnings", []):
            if warning.startswith("relation_coverage: "):
                return json.loads(warning.split(": ", 1)[1])
    return None


def source_proofs(workflow, expected, corpus):
    docs = []
    invalid = []
    for item in workflow["definitions"] + workflow["source_audits"]:
        begin = item.get("line", item.get("start"))
        original = (corpus / item["file"]).read_text().splitlines()
        body = item["body"].splitlines()
        if not body:
            continue
        subset = original[begin - 1:begin - 1 + len(body)]
        # A tree-sitter range can begin after indentation on the first line.
        valid = len(subset) == len(body) and subset[0].lstrip() == body[0].lstrip() and subset[1:] == body[1:]
        if valid:
            docs.append((item["file"], begin, begin + len(body) - 1))
        else:
            invalid.append((item["file"], begin))
    missing = [p for p in expected if not any(f == p["file"] and a <= p["start"] and b >= p["end"]
                                             for f, a, b in docs)]
    return not missing and not invalid, missing, invalid


def raw_check(workflow, gold, source_ok):
    rows, docs = workflow["primary_rows"], workflow["primary_docs"]
    failures = []
    if workflow["arm"] == "B" and workflow["definitions"]:
        return source_ok, [] if source_ok else ["source proof missing"]
    if "sites" not in gold:
        expected_file = gold["proofs"][0]["file"]
        wrong_files = sorted({r["file"] for r in rows if r["file"] != expected_file})
        if wrong_files:
            failures.append({"wrong_source_files": wrong_files})
    if "line_counts" in gold:
        actual = {str(k): v for k, v in sorted(Counter(r["line"] for r in rows).items())}
        if actual != gold["line_counts"]:
            failures.append({"call_position_counts": {"expected": gold["line_counts"], "actual": actual}})
    for line in gold.get("forbid_bound_lines", []):
        for row in rows:
            if row["line"] == line and row["to"]:
                failures.append({"false_receiver_target": {"line": line, "to": row["to"]}})
    if "required_names" in gold:
        missing = sorted(set(gold["required_names"]) - {r["to"] for r in rows})
        if missing:
            failures.append({"required_resolved_helpers_missing": missing})
    if "forbidden_owner_lines" in gold:
        bad = [r["line"] for r in rows if r["line"] in gold["forbidden_owner_lines"]]
        if bad:
            failures.append({"lambda_calls_attributed_to_outer": bad})
    if "forbidden_target" in gold:
        bad = gold["forbidden_target"]
        if any(r["line"] == bad["line"] and r["to"] == bad["to"] for r in rows):
            failures.append({"invented_recursion": bad})
        # This question requires proof of the receiver's container type. A bare
        # unresolved edge, or an edge to a loader, cannot supply that definition.
        failures.append("raw relation lacks receiver-type source proof")
    if gold.get("require_subject_ambiguity"):
        warned = any("distinct symbols named" in w for d in docs for w in d["warnings"])
        if rows or not warned:
            failures.append("overload subject silently unioned or ambiguity undisclosed")
    if gold.get("require_unsupported_or_source"):
        c = coverage(docs)
        if not (c and c["issue_counts"].get("unsupported_language", 0) and c["complete_within_model"] is False):
            failures.append("unsupported Python empty result lacks disclosure")
    if "sites" in gold:
        selected = [r for r in rows if r["file"].startswith("src/")
                    and (workflow["arm"] != "B" or r["evidence"] == "call")]
        actual = sorted((r["file"], r["line"]) for r in selected)
        if actual != sorted(map(tuple, gold["sites"])):
            failures.append({"production_callers": {"actual": actual, "expected": gold["sites"]}})
    return not failures, failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ["runs", "gold", "lock", "corpus", "output"]:
        parser.add_argument("--" + key, type=Path, required=True)
    args = parser.parse_args()
    lock, gold = json.loads(args.lock.read_text()), json.loads(args.gold.read_text())
    for file, expected in lock["source_sha256"].items():
        assert hashlib.sha256((args.corpus / file).read_bytes()).hexdigest() == expected, file
    runs = json.loads(args.runs.read_text())
    scored = []
    for r in runs["workflows"]:
        g = gold[r["task"]]
        source_ok, missing, invalid = source_proofs(r, g["proofs"], args.corpus)
        raw_ok, failures = raw_check(r, g, source_ok)
        final_ok = source_ok or (r.get("stop_on_model_evidence", False) and raw_ok)
        if "sites" in g:
            actual = sorted((x["file"], x["line"]) for x in r["final_rows"])
            final_ok = source_ok and actual == sorted(map(tuple, g["sites"]))
        final_ok &= r["error"] is None
        scored.append({"task": r["task"], "arm": r["arm"], "repeat": r["repeat"],
                       "raw_sufficient": raw_ok and r["error"] is None, "raw_failures": failures,
                       "retrieval_sufficient": final_ok, "missing_source_proofs": missing,
                       "invalid_source_docs": invalid, "error": r["error"], "operations": r["operations"],
                       "communication_bytes": r["communication_bytes"], "wall": r["wall"], "max_rss": r["max_rss"]})
    summary = {}
    for arm in ["A", "B", "C"]:
        selected = [r for r in scored if r["arm"] == arm]
        by_repeat = {}
        for repeat in sorted({r["repeat"] for r in selected}):
            batch = [r for r in selected if r["repeat"] == repeat]
            by_repeat[repeat] = {"raw_success": [r["task"] for r in batch if r["raw_sufficient"]],
                "retrieval_success": [r["task"] for r in batch if r["retrieval_sufficient"]],
                "operations": sum(r["operations"] for r in batch),
                "communication_bytes": sum(r["communication_bytes"] for r in batch),
                "wall": sum(r["wall"] for r in batch)}
        success_count = sum(r["retrieval_sufficient"] for r in selected)
        summary[arm] = {"repeats": by_repeat,
            "bytes_per_sufficient_attempt_including_failures": sum(r["communication_bytes"] for r in selected) / success_count if success_count else None,
            "per_task": {task: {"raw_sufficient": all(r["raw_sufficient"] for r in selected if r["task"] == task),
                "retrieval_sufficient": all(r["retrieval_sufficient"] for r in selected if r["task"] == task),
                "operations": statistics.median(r["operations"] for r in selected if r["task"] == task),
                "bytes": statistics.median(r["communication_bytes"] for r in selected if r["task"] == task),
                "wall_median": statistics.median(r["wall"] for r in selected if r["task"] == task),
                "wall_max": max(r["wall"] for r in selected if r["task"] == task)} for task in gold}}
    output = {"scope": "evidence retrieval, not model answer accuracy", "gold_sha256": hashlib.sha256(args.gold.read_bytes()).hexdigest(),
              "summary": summary, "scored": scored}
    args.output.write_text(json.dumps(output, ensure_ascii=False, indent=2) + "\n")
    for arm, s in summary.items():
        print(arm, json.dumps(s["repeats"], ensure_ascii=False))


if __name__ == "__main__":
    main()
