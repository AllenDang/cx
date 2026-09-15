#!/usr/bin/env python3
"""Run frozen ANGE evidence-retrieval workflows without reading their gold.

Inputs are an external tasks.json and an immutable disposable corpus. A and B
use the baseline binary; C uses the candidate. This does not run an agent or
claim that retrieval sufficiency equals an LLM answering correctly.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import time


class BudgetExceeded(Exception):
    pass


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def coverage(doc):
    for warning in doc.get("warnings", []):
        if warning.startswith("relation_coverage: "):
            return json.loads(warning.split(": ", 1)[1])
    return None


class Workflow:
    def __init__(self, root, binary, env, dest, fallback_source=False):
        self.root, self.binary, self.env, self.dest = root, binary, env, dest
        self.fallback_source = fallback_source
        self.dest.mkdir(parents=True)
        self.steps = []
        self.start = time.perf_counter()

    def check_budget(self):
        if len(self.steps) > 12:
            raise BudgetExceeded("more than 12 operations")
        if sum(s["request_bytes"] + s["stdout_bytes"] for s in self.steps) > 128 * 1024:
            raise BudgetExceeded("more than 128 KiB request+stdout")
        if time.perf_counter() - self.start > 30:
            raise BudgetExceeded("more than 30 seconds")

    def cli(self, args, purpose):
        self.check_budget()
        argv = [str(self.binary), *args]
        request = {"tool": "cx", "argv": argv}
        start = time.perf_counter()
        result = subprocess.run(["/usr/bin/time", "-l", *argv], cwd=self.root, env=self.env,
                                capture_output=True, timeout=30)
        stderr = result.stderr.decode(errors="replace")
        rss = re.search(r"(\d+)\s+maximum resident set size", stderr)
        cpu = re.search(r"([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys", stderr)
        step = {"purpose": purpose, "request": request, "request_bytes": len(json.dumps(request).encode()),
                "wall": time.perf_counter() - start, "exit_code": result.returncode,
                "stdout_bytes": len(result.stdout), "stderr_bytes": len(result.stderr),
                "rss": int(rss[1]) if rss else None,
                "user": float(cpu[2]) if cpu else None, "sys": float(cpu[3]) if cpu else None}
        n = len(self.steps)
        (self.dest / f"{n:02}.stdout").write_bytes(result.stdout)
        (self.dest / f"{n:02}.stderr").write_bytes(result.stderr)
        self.steps.append(step)
        self.check_budget()
        if result.returncode:
            raise RuntimeError(f"cx exit {result.returncode}; step {n}")
        doc = json.loads(result.stdout)
        return doc

    def query(self, args, purpose):
        return self.cli(["--root", str(self.root), "--json", *args], purpose)

    def collect(self, args, purpose):
        doc = self.query(args, purpose)
        docs = [doc]
        rows = list(doc["results"])
        while doc["page"]["truncated"]:
            if not doc["results"] or not doc["next_queries"]:
                raise RuntimeError("non-progressing pagination")
            tokens = shlex.split(doc["next_queries"][0])
            previous = doc["page"]["offset"] + len(doc["results"])
            doc = self.cli(tokens[1:], purpose + "_page")
            if doc["page"]["offset"] != previous:
                raise RuntimeError("next_queries did not advance by emitted rows")
            rows.extend(doc["results"])
            docs.append(doc)
        return rows, docs

    def definitions(self, task, helper=None):
        args = ["definition", "--name", helper or task["name"], "--from", task["file"],
                "--role", "definition", "--max-lines", "200", "--all"]
        if not helper and task.get("scope"):
            args += ["--scope", task["scope"]]
        rows, _ = self.collect(args, "helper_definition" if helper else "definition_audit")
        if not rows and self.fallback_source:
            # Same result-driven fallback for every arm; no task IDs or gold.
            rows = [self.read_source(task["file"], 1, 2000, "definition_source_fallback", 50 * 1024)]
        return rows

    def read_context(self, file, line):
        return self.read_source(file, max(1, line - 5), 11, "caller_source_audit")

    def read_source(self, file, begin, limit, purpose, byte_limit=None):
        self.check_budget()
        request = {"tool": "read", "file": file, "offset": begin, "limit": limit}
        if byte_limit is not None:
            request["byte_limit"] = byte_limit
        start = time.perf_counter()
        path = (self.root / file).resolve()
        if not path.is_relative_to(self.root):
            raise RuntimeError("source path escapes corpus")
        source = path.read_bytes()
        lines = source.decode().splitlines(keepends=True)
        end = min(len(lines), begin + limit - 1)
        content = ""
        used = 0
        for i in range(begin - 1, end):
            size = len(lines[i].encode())
            if byte_limit is not None and used + size > byte_limit:
                end = i
                break
            content += lines[i]
            used += size
        doc = {"file": file, "start": begin, "end": end, "body": content,
               "source_sha256": hashlib.sha256(source).hexdigest()}
        if byte_limit is not None:
            doc.update(evidence="source_text", truncated=end < len(lines))
        data = (json.dumps(doc, ensure_ascii=False, indent=2) + "\n").encode()
        n = len(self.steps)
        (self.dest / f"{n:02}.stdout").write_bytes(data)
        (self.dest / f"{n:02}.stderr").write_bytes(b"")
        self.steps.append({"purpose": purpose, "request": request,
                           "request_bytes": len(json.dumps(request).encode()),
                           "stdout_bytes": len(data), "stderr_bytes": 0, "wall": time.perf_counter() - start,
                           "exit_code": 0, "rss": None, "user": None, "sys": None})
        self.check_budget()
        return doc

    def finish(self, result):
        result.update(steps=self.steps, wall=time.perf_counter() - self.start,
                      operations=len(self.steps),
                      communication_bytes=sum(s["request_bytes"] + s["stdout_bytes"] for s in self.steps),
                      max_rss=max((s["rss"] or 0 for s in self.steps), default=0))
        (self.dest / "workflow.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
        return result


def task_workflow(task, arm, workflow):
    result = {"task": task["id"], "arm": arm, "definitions": [], "source_audits": [],
              "primary_rows": [], "primary_docs": [], "final_rows": [], "error": None}
    try:
        if arm == "B" and task["kind"] == "callees":
            result["definitions"] = workflow.definitions(task)
            for helper in task.get("helper_names", []):
                result["definitions"] += workflow.definitions(task, helper)
            return workflow.finish(result)
        if arm == "B":
            args = ["references", "--name", task["name"], "--file", task["filter_prefix"],
                    "--context", "--limit", "50"]
        else:
            args = [task["kind"], "--name", task["name"], "--limit", "50"]
            if task.get("scope"):
                args += ["--scope", task["scope"]]
        rows, docs = workflow.collect(args, "primary")
        result["primary_rows"], result["primary_docs"] = rows, docs
        if task["kind"] == "callers":
            if arm != "B" and not rows and task.get("scope"):
                rows, _ = workflow.collect(["callers", "--name", task["name"], "--limit", "50"], "unscoped_fallback")
            rows = [r for r in rows if r["file"].startswith(task["filter_prefix"])
                    and (arm != "B" or r["evidence"] == "call")]
            result["final_rows"] = rows
            for file, line in sorted({(r["file"], r["line"]) for r in rows}):
                result["source_audits"].append(workflow.read_context(file, line))
            return workflow.finish(result)
        result["final_rows"] = rows
        cov = coverage(docs[0])
        issues = cov.get("issue_counts", {}) if cov else {}
        ambiguous = any(re.search(r"\d+ distinct symbols named", w) for d in docs for w in d["warnings"])
        modern_position_evidence = (cov and cov["model"] == "direct_syntax_v2"
            and cov["files_analyzed"] > 0 and not ambiguous
            and not any(issues.get(k, 0) for k in ["parse_error", "read_failed", "content_changed", "missing_grammar"])
            and cov["files_skipped_missing_grammar"] == 0)
        explicit_unsupported = (cov and issues.get("unsupported_language", 0) > 0
                                and cov["complete_within_model"] is False)
        can_stop = (task["profile"] in {"leaf", "ownership", "multiplicity"} and bool(modern_position_evidence))
        can_stop |= task["profile"] == "unsupported" and bool(explicit_unsupported)
        result["stop_on_model_evidence"] = bool(can_stop)
        if not can_stop:
            result["definitions"] = workflow.definitions(task)
            for helper in task.get("helper_names", []):
                result["definitions"] += workflow.definitions(task, helper)
    except (RuntimeError, BudgetExceeded, subprocess.TimeoutExpired) as exc:
        result["error"] = str(exc)
    return workflow.finish(result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["tasks", "corpus", "baseline", "candidate", "grammars", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--bounded-missing-definition-read", action="store_true",
                        help="Supplementary v2 policy: bounded read when definition has no rows")
    args = parser.parse_args()
    tasks = json.loads(args.tasks.read_text())  # Deliberately no gold/scorer access.
    root, output = args.corpus.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    binaries = {"A": args.baseline.resolve(), "B": args.baseline.resolve(), "C": args.candidate.resolve()}
    envs = {}
    cold = []
    for arm, binary in binaries.items():
        cache = output / ("cache-" + arm)
        cache.mkdir()
        envs[arm] = dict(os.environ, CX_CACHE_DIR=str(cache),
                         TREE_SITTER_LANGUAGE_PACK_LIBS_DIR=str(args.grammars.resolve()),
                         TREE_SITTER_LANGUAGE_PACK_MANIFEST_URL="file:///cx-no-task-download.json")
        wf = Workflow(root, binary, envs[arm], output / ("cold-" + arm))
        doc = wf.query(["symbols", "--limit", "1"], "cold_index")
        if doc["freshness"]["files_checked"] == 0 or doc["freshness"]["files_skipped_missing_grammar"]:
            raise RuntimeError("invalid cold corpus/grammar setup")
        cold.append(wf.finish({"arm": arm, "binary_sha256": digest(binary), "freshness": doc["freshness"]}))
    workflows = []
    for repeat in range(args.repeats):
        arms = ["A", "B", "C"]
        arms = arms[repeat % 3:] + arms[:repeat % 3]
        for task in tasks:
            for arm in arms:
                wf = Workflow(root, binaries[arm], envs[arm], output / f'{repeat}-{task["id"]}-{arm}',
                              args.bounded_missing_definition_read)
                record = task_workflow(task, arm, wf)
                record["repeat"] = repeat
                workflows.append(record)
                print(repeat, task["id"], arm, record["operations"], record["communication_bytes"], record["error"], flush=True)
                (output / "runs.json").write_text(json.dumps({"tasks_sha256": digest(args.tasks), "cold": cold,
                    "policy": "v2_bounded_missing_definition_read" if args.bounded_missing_definition_read else "v1_frozen",
                    "workflows": workflows}, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
