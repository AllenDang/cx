# Stage R evaluation evidence

This directory is excluded from cx indexing by `.cx-ignore`. Runners and future
independent gold must not become production search inputs. No held-out task set,
embedding index, or generated “gold” is supplied here.

- [Contract](../../docs/STAGE_R_RELATION_FOUNDATION.md)
- [Initial scoped implementation report](REPORT.md)
- [Follow-up real-task ANGE report — adoption FAIL](ANGE_TASK_REPORT.md)
- [Completed ANGE fixes — correctness restored, cost caveat retained](ANGE_FIX_REPORT.md)
- [Combined impact/changes/context implementation and pilot](TASK_MECHANISMS_REPORT.md)
- [Call/lexical fast paths and compact-output optimization](OPTIMIZATION_REPORT.md)
- [User/API contracts for the three commands](../../docs/TASK_ANALYSIS.md)
- [Parent plan](../../docs/AGENT_TASK_ANALYSIS_PLAN.md)

## Real-task evidence workflows

The follow-up uses an external, pre-frozen task manifest and separate gold. Do not
place private source/gold in a production index. Preserve the original protocol
and v1 results when running the openly amended v2 missing-definition fallback.

```sh
python3 bench/task_analysis/ange_tasks.py \
  --tasks /tmp/cx-ange-tasks.M10rZU/tasks.json \
  --corpus /tmp/cx-ange-tasks.M10rZU/corpus \
  --baseline /tmp/cx-stage-r.uLMFKA/bin/baseline \
  --candidate /tmp/cx-stage-r.uLMFKA/bin/candidate \
  --grammars "$HOME/Library/Caches/cx/grammars/tree-sitter-language-pack/v1.16.1/libs" \
  --output /tmp/cx-ange-replay-new
# Add --bounded-missing-definition-read only for the declared v2 strategy.

python3 bench/task_analysis/score_ange_tasks.py \
  --runs /tmp/cx-ange-replay-new/runs.json \
  --gold /tmp/cx-ange-tasks.M10rZU/gold.json \
  --lock /tmp/cx-ange-tasks.M10rZU/corpus.lock.json \
  --corpus /tmp/cx-ange-tasks.M10rZU/corpus \
  --output /tmp/cx-ange-replay-score.json

PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover \
  -s bench/task_analysis -p test_ange_scoring.py -v
```

`diagnose_ange_tasks.py` accepts the same corpus/binaries/grammars/output arguments
but no tasks/gold. It creates compiler-checked miniature controls and a private
full-corpus copy for the missed-refresh experiment; its process exit only says
the diagnostic ran. Inspect `mechanism_pass`, expected/actual counts, and recovery
fields in `findings.json` for the product result.

These scripts exercise CLI/read workflows, not real model sessions. Scoring checks
evidence sufficiency and original source ranges, never generates an agent answer
from gold. The report retains the primary graph errors even when later source
reads can let a reviewer correct them. Files under `/tmp` need separate archival
if the evidence must survive cleanup/reboot.

## Regression gates

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --bins
cargo test --locked --tests
cargo test --locked --all-targets --all-features
cargo test --locked --test relation_identity
cargo test --locked --test relation_coverage
cargo test --locked --test relations
cargo package --locked --allow-dirty
```

The independent small-program oracles live next to their literal sources in
`tests/relation_identity.rs` and `tests/relation_coverage.rs`; all materialization
and edits use `TempDir`. `src/relation_index/tests.rs` tests typed identities and
uses an injected reader as a deterministic race barrier. None calls the production
resolver to manufacture expected values.

## Targeted mutation controls

```sh
python3 bench/task_analysis/mutations.py --output /tmp/cx-r-mutations-new
```

The output path must not exist. The runner copies only Cargo inputs, `src`, and
`tests` into a disposable directory and adds a `.git` marker for project-root
unit tests. It runs the green control first, then 18 isolated wrong-mechanism
patches. Only the named failing assertion counts as a kill, not a compile error,
other test failure, or timeout. Each original file is restored between patches;
the source copy is removed at the end. Output logs/manifest and Cargo artifacts
remain in the output directory. An explicit `CARGO_TARGET_DIR` may reuse a
**disposable** build cache; do not point it at a concurrent build.

The language-pack dependency can install missing grammars. Have test grammars
available before timing. The missing-grammar CLI test overrides both the bundled
library path and manifest URL to a nonexistent local file, so it neither depends
on network failure nor modifies shared grammars.

## Fixed ANGE cost checks

Build the candidate and a separate baseline binary from commit
`18337372cc95092fd9daca45936159e3d28652d9`. Never overwrite a user's checkout to
build the baseline. Materialize ANGE commit
`70fe1922de2f05bec4a94f5967c68a92a8320b1a` using `git archive` or a disposable
worktree, then run on macOS:

```sh
python3 bench/task_analysis/measure_stage_r.py \
  --corpus /tmp/disposable-fixed-ange \
  --baseline /tmp/cx-baseline \
  --candidate "$PWD/target/release/cx" \
  --grammars "$HOME/Library/Caches/cx/grammars/tree-sitter-language-pack/v1.16.1/libs" \
  --output /tmp/cx-r-cost-new --runs 9
```

The runner creates another private corpus copy before any incremental edit,
uses separate arm caches and identical grammar libraries, warms each query once,
then records at least nine samples. `/usr/bin/time -l` captures RSS/CPU alongside
wall time, exit status, argv, and full stdout/stderr. It verifies source content
restoration before removing the copy. It never runs `scripts/bench.sh` against a
user checkout. Existing output directories are rejected.

Counts are independently specified by the acceptance document and a source audit:
1 definition, 28 references grouped into 10 file rows, 26 incoming call sites,
and 31 outgoing syntactic call sites in the candidate. The old outgoing count is
30 because two `key()` calls on line 362 were deduplicated. Default callee pages
may now require follow-up pages; `--all` is measured separately for correctness.
This is a one-hop cost/compatibility check, **not** a complete-task A/B/C benchmark
or an efficiency claim. It does not count bytes as model tokens.

Inspect `REPORT.md` for unrun release/held-out gates and the large relative cost
increase of callees, rather than treating exit zero as a universal performance
or task-benefit verdict.
