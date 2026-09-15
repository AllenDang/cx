#!/usr/bin/env python3
"""Follow-up diagnostics; never modify the input corpus or cx implementation.

The small C++ controls and injected edit are NOT counted as real tasks. Findings
have explicit expected/actual fields; successful runner exit is not a product PASS.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from ange_tasks import Workflow


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ["corpus", "baseline", "candidate", "grammars", "output"]:
        parser.add_argument("--" + key, type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    corpus = args.corpus.resolve()
    file = "src/engine/texture_ops/tex_pass_plan.cpp"
    original_hash = hashlib.sha256((corpus / file).read_bytes()).hexdigest()
    findings = []

    def new_workflow(root, arm, label):
        cache = output / (label + "-cache-" + arm)
        cache.mkdir()
        env = dict(os.environ, CX_CACHE_DIR=str(cache),
                   TREE_SITTER_LANGUAGE_PACK_LIBS_DIR=str(args.grammars.resolve()),
                   TREE_SITTER_LANGUAGE_PACK_MANIFEST_URL="file:///cx-no-diagnostic-download.json")
        binary = args.baseline if arm == "A" else args.candidate
        return Workflow(root, binary.resolve(), env, output / (label + "-" + arm))

    controls = {
        "builtin_cast": "#include <cctype>\nvoid probe(int c) { (void)std::isspace(static_cast<unsigned char>(c)); }\n",
        "reference_return": "#include <unordered_map>\nstd::unordered_map<int,int> &cache() { static std::unordered_map<int,int> value; return value; }\nvoid probe() { (void)cache().find(0); }\n",
    }
    for label, source in controls.items():
        with tempfile.TemporaryDirectory(prefix="cx-ange-control-") as tmp:
            root = Path(tmp).resolve()
            path = root / "control.cpp"
            path.write_text(source)
            (output / (label + ".cpp")).write_text(source)
            cmd = ["c++", "-std=c++17", "-fsyntax-only", str(path)]
            compiler = subprocess.run(cmd, capture_output=True)
            (output / (label + "-compiler.stdout")).write_bytes(compiler.stdout)
            (output / (label + "-compiler.stderr")).write_bytes(compiler.stderr)
            (output / (label + "-compiler.json")).write_text(json.dumps({"argv": cmd, "exit_code": compiler.returncode}) + "\n")
            if compiler.returncode:
                raise RuntimeError("invalid C++ control, not a cx failure")
            for arm in ["A", "C"]:
                wf = new_workflow(root, arm, label)
                probe = wf.query(["definition", "--name", "probe", "--from", "control.cpp", "--all"], "positive_definition_control")
                positive = len(probe["results"]) == 1
                if label == "builtin_cast":
                    real_call = wf.query(["callers", "--name", "isspace", "--all"], "positive_call_control")
                    positive &= len(real_call["results"]) == 1
                argv = (["callers", "--name", "char", "--all"] if label == "builtin_cast" else
                        ["definition", "--name", "cache", "--from", "control.cpp", "--all"])
                doc = wf.query(argv, label)
                expected = 0 if label == "builtin_cast" else 1
                findings.append(wf.finish({"label": label, "arm": arm, "expected": expected,
                    "actual": len(doc["results"]), "mechanism_pass": positive and len(doc["results"]) == expected,
                    "positive_controls_pass": positive, "query_result": doc}))

    # Reduce only the file set, not source contents, to expose which of the two
    # queried files caused the sampled coverage's parse_error counter.
    with tempfile.TemporaryDirectory(prefix="cx-ange-coverage-") as tmp:
        root = Path(tmp).resolve()
        for suffix in ["cpp", "h"]:
            source = corpus / f"src/engine/texture_ops/tex_pass_plan.{suffix}"
            shutil.copy2(source, root / source.name)
        wf = new_workflow(root, "C", "coverage-localization")
        doc = wf.query(["callees", "--name", "tex_pass_plan_trailing_count", "--scope",
                        "ANGE::tex_pass_plan_trailing_count", "--all"], "coverage_localization")
        findings.append(wf.finish({"label": "coverage_localization", "query_result": doc,
                                  "scope": "two original files only; diagnostic, not task score"}))

    with tempfile.TemporaryDirectory(prefix="cx-ange-missed-refresh-") as tmp:
        root = Path(tmp).resolve() / "corpus"
        shutil.copytree(corpus, root, symlinks=True)
        target = root / file
        before = target.read_bytes()
        stat = target.stat()
        old = b"bool tex_pass_plan_materialises(const TexPassPlan &passes) {"
        new = b"bool tex_pass_plan_materialized(const TexPassPlan &passes) {"
        assert len(old) == len(new) and before.count(old) == 1
        workflows = {arm: new_workflow(root, arm, "missed-refresh") for arm in ["A", "C"]}
        primes = {}
        for arm, wf in workflows.items():
            primes[arm] = wf.query(["callees", "--name", "tex_pass_plan_materialises", "--scope",
                                   "ANGE::tex_pass_plan_materialises", "--all"], "prime_original_index")
        try:
            target.write_bytes(before.replace(old, new, 1))
            os.utime(target, ns=(stat.st_atime_ns, stat.st_mtime_ns))
            assert target.stat().st_size == stat.st_size and target.stat().st_mtime_ns == stat.st_mtime_ns
            for arm, wf in workflows.items():
                missed = wf.query(["callees", "--name", "tex_pass_plan_materialises", "--scope",
                                   "ANGE::tex_pass_plan_materialises", "--all"], "without_refresh")
                refreshed = wf.query(["refresh", file], "explicit_refresh")
                current = wf.query(["definition", "--name", "tex_pass_plan_materialized", "--from", file, "--all"], "current_definition")
                wrong_owner = [r for r in missed["results"] if r.get("from") == "ANGE::tex_pass_plan_materialises"]
                findings.append(wf.finish({"label": "missed_refresh", "arm": arm,
                    "original_owner_should_not_be_reported": True, "wrong_old_owner_rows": len(wrong_owner),
                    "prime_row_count": len(primes[arm]["results"]),
                    "mechanism_pass": len(primes[arm]["results"]) == 3 and not wrong_owner
                        and any("content_changed" in w for w in missed["warnings"]),
                    "missed": missed, "refresh": refreshed, "recovery": current,
                    "recovery_pass": len(current["results"]) == 1 and "tex_pass_plan_materialized" in current["results"][0]["body"]}))
        finally:
            target.write_bytes(before)
            os.utime(target, ns=(stat.st_atime_ns, stat.st_mtime_ns))
            assert target.read_bytes() == before
    assert hashlib.sha256((corpus / file).read_bytes()).hexdigest() == original_hash
    (output / "findings.json").write_text(json.dumps(findings, ensure_ascii=False, indent=2) + "\n")
    for item in findings:
        print({k: item[k] for k in ["label", "arm", "expected", "actual", "mechanism_pass", "wrong_old_owner_rows", "recovery_pass"] if k in item})


if __name__ == "__main__":
    main()
