# Phase 0 baseline — fixtures, pinned behavior, benchmark harness

> Roadmap reference: `docs/AGENT_NATIVE_DEVELOPMENT_ROADMAP.md` §11 Phase 0, §13.
> Baseline binary: cx 0.7.2, `INDEX_VERSION = 8`.

Phase 0 records what cx does *today* so every later phase has a mechanical
red/green signal instead of prose. Nothing in cx's behavior changed in this
phase.

## 1. Fixture corpus

`tests/fixtures/agent_corpus/` is a deliberately small tree that contains one
instance of each problem class named in the roadmap:

| Path | Purpose |
| --- | --- |
| `include/ange/ecs.hpp` | C++ forward declaration + class declaration (§4.2) |
| `src/ecs.cpp` | matching definitions, constructor/destructor, `EcsWorld::run` (§4.2, §4.3) |
| `src/scope_a.cpp` | free `run()` in `namespace alpha` (§4.3) |
| `src/scope_b.cpp` | `beta::Runner::run` declaration + definition, plus caller `run_all` (§4.3, §9) |
| `src/comments.cpp` | `run` only in comments and a string literal (§3.3) |
| `src/lib.rs` | two Rust modules each defining `run`, plus a `#[cfg(test)]` module (§4.3) |
| `src/app.ts` | TypeScript interface member, class method, and exported `run` (§5.5 language spread) |
| `tests/ecs_test.cpp` | test-path classification |
| `vendor/thirdparty/blob.cpp` | vendor filtering for the future `map` command (§8) |
| `generated/gen_api.cpp` | generated-code filtering (§8) |
| `docs/design.md` | markdown headings, prose mention of `run` |

`tests/fixtures/.cx-ignore` keeps cx from indexing fixtures while indexing its
own repository; `tests/support/mod.rs` copies a fixture into a temp project
(dropping the marker and adding `.git`) so each test gets an isolated index.

## 2. Pinned behavior (`tests/fixture_corpus.rs`, 19 tests)

Exact counts, not existence checks (§12.9):

- corpus indexes **12 files / 41 symbols**
- `definition --name validate_param --all` → **2** rows: `include/ange/ecs.hpp`
  then `src/ecs.cpp`, with **no** `role` field — the declaration and the
  definition are currently indistinguishable (§4.2)
- `symbols --name run --all` → **12** rows, none carrying `qualified_name` or
  `scope_path` (§4.3)
- `references --name run --context --all` → **17** rows (18 identifier
  occurrences, two on `src/lib.rs:18` collapsed by per-line dedup); zero rows
  from `src/comments.cpp`, proving syntax filtering excludes comment/string
  text (§3.3); no `evidence`/`resolution` fields yet
- `references --name run --all` (summary) → **9** file rows; `src/lib.rs` has
  `refs=3`, `lines="5, 12, 18"`, `callers="run, run_both"`
- `--json` root type **changes with pagination**: bare array when complete,
  `{total, offset, limit, results}` when truncated *or* when `--offset > 0`;
  `truncated` appears only on stderr (§3.4, §6.1)
- three `--limit 5` pages cover all 12 rows without overlap
- empty result set prints **nothing** on stdout and `cx: no matches` on stderr
  with exit 0 — unparseable for an agent (§6.2)
- no freshness/generation field appears anywhere in output (§4.4)
- edit / delete / rename are picked up by the next ordinary query
- `--file` on an unindexed path exits 1; unsupported extension is reported by
  extension name

Tests that Phase 2/3/4/5/7 are expected to flip carry a `KNOWN GAP` doc comment
naming the roadmap section and the phase.

## 3. Benchmark harness

`scripts/bench.sh [project-dir] [runs]` records the §10.3 metric set:
cold index wall/user/sys, peak RSS, index bytes, warm query median/p95, output
bytes per query, and one-file incremental refresh. It uses a throwaway
`CX_CACHE_DIR` (symlinking the host grammar cache so downloads are not timed)
and never asserts thresholds — performance is reported, only correctness is
gated in CI.

### Baseline on the cx repository itself

`./scripts/bench.sh . 5` — cx 0.7.2 release build, macOS arm64, 42 indexed files:

| Metric | Value |
| --- | --- |
| cold build | 0.08 s wall / 0.06 s user / 33.7 MiB peak RSS |
| index size | 1,056,768 B |
| root overview | 32 ms median, 37 ms p95, 760 B output |
| file overview | 32 ms median, 183 B output |
| definition | 31 ms median, 6,878 B output |
| references | 57 ms median, 443 B output |
| symbol search | 31 ms median, 2,750 B output |
| one-file refresh | 0.04 s wall / 16.1 MiB peak RSS |

Numbers are machine-specific; re-run before and after each phase and compare on
the same host. The ANGE-scale reference numbers remain those in the roadmap §1.
