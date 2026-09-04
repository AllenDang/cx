# cx acceptance report — commit `0e853f3`

> Standard executed: [`docs/ACCEPTANCE_TEST_STANDARD.md`](ACCEPTANCE_TEST_STANDARD.md)
> Verdict: **PASS**
> Run date: 2026-09-04T14:02:54Z

## 1. Subject

| Field | Value |
| --- | --- |
| cx commit | `0e853f33438b669697d9a5bcee019d13919bf147` |
| branch | `master` |
| checkout | detached `git worktree` at that commit |
| worktree state | **clean** — `git status --porcelain` = 0 changes |
| host | Apple arm64 (M1 Pro / T6000), macOS 25.6.0 |
| rustc / cargo | 1.98.1 (48a229cea) / 1.98.1 (797e8a9bc) |
| ANGE repo | `$HOME/Documents/GodotProjects/ange` |
| **ANGE commit (pinned)** | **`70fe1922de2f05bec4a94f5967c68a92a8320b1a`** |
| ANGE corpus | 13,109 tracked files; 2,090 tracked C/C++ files |

Because the host is Apple arm64/macOS, the §7.2 performance budgets are **graded**
rather than merely recorded.

Two notes on scope, so this report is not read as more than it is:

- The acceptance ran against `0e853f3`. The later commit `b323b0c` is a
  markdown-delimiter-only change to the standard document (verified 0
  non-delimiter diff lines); it touches no code, test, or budget.
- The ANGE main worktree was dirty with pre-existing user edits throughout. It
  was never written to; all work happened in a detached disposable worktree at
  the pinned commit, removed afterwards. Its `HEAD` and `git status` were
  identical before and after.

## 2. Correctness gates

| Command | Exit | Result |
| --- | ---: | --- |
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | 0 | clean |
| `cargo test --locked --bins` | 0 | 194 passed |
| `cargo test --locked --tests` | 0 | 340 passed / 8 targets |
| `cargo test --locked --all-targets --all-features` | 0 | 340 passed |
| `cargo build --locked --release` | 0 | — |
| `cargo package --locked` | 0 | no `--allow-dirty` needed |
| `cargo install --locked --path .` | 0 | installed binary re-ran a fixture query |

`--lib` and `--doc` were **skipped by rule, not by convenience**: the §3.0
`cargo metadata` check reports no library target (cx declares 1 `bin` + 7 `test`
targets), so those two commands are not applicable. Unit tests live in the binary
target and are covered by `--bins` with 194 non-zero tests.

### Test-target integrity (§3.1 — a target reporting 0 tests is a failure)

| Target | Tests |
| --- | ---: |
| `bins` (unit) | 194 |
| `fixture_corpus` | 27 |
| `path_identity` | 7 |
| `freshness` | 13 |
| `qualified_identity` | 11 |
| `map` | 12 |
| `relations` | 14 |
| `integration` | 62 |
| **total** | **340** |

## 3. Exact correctness counts on the pinned ANGE commit

Subject symbol: `validate_stmt_against_action_spec`.

Ground truth was established **independently of cx**, with `git grep` over the
pinned worktree, and classified by whether the matching line is comment-only:

| Ground truth (C/C++ files only) | Count |
| --- | ---: |
| total occurrences | 35 |
| code positions | 28 |
| comment-only lines | 7 |

cx results against that ground truth:

| Assertion | Expected | Actual | Result |
| --- | ---: | ---: | --- |
| definition | 1 | **1** | PASS |
| references (structural positions) | 28 | **28** | PASS |
| callers (call sites) | 26 | **26** | PASS |
| comment-only false positives | 0 | **0** | PASS |

Supporting detail, all read back from the stored JSON:

- **definition** — `src/engine/action/param_validator.cpp`, qualified
  `ANGE::validate_stmt_against_action_spec`, body contains `PARAM-002`, 66 lines
  (so it is the implementation, not a header prototype).
- **references** — evidence split is exactly `call: 26`, `definition: 1`,
  `declaration: 1`; every row's resolution is `syntax` and nothing claims
  `type_resolved`. The returned `(file, line)` set is an **exact set match** with
  the 28 ground-truth code positions: 0 missed, 0 extra.
- **callers** — every row carries `evidence: "call"`; definitions and
  declarations are not counted as callers.
- **comment false positives** — all 7 comment-only lines were individually
  verified absent from the result set.
- **cold index** — exit 0, `page.total` 52,977 symbols,
  `files_checked` 3,905, `files_updated` 3,901,
  `files_skipped_missing_grammar` 4.

One honest discrepancy worth recording: an earlier run on the same pinned commit
reported 52,925 symbols and 3 skipped files, versus 52,977 and 4 here. The cause
is the installed grammar set (that run had 8 grammars including `rust`; this one
had 7), which changes which files are indexable. It does not touch any pinned
count above — all four are C/C++ and matched exactly — but `page.total` is
grammar-set dependent and should not be treated as a fixed value.

## 4. Performance

Release binary, pinned ANGE worktree, grammars pre-downloaded, 1 untimed warmup
followed by 9 timed runs, exit codes read directly rather than through a pipe.

### Cold index (§7.2 budget: 5.0 s wall / 160 MiB RSS / 32 MiB DB)

| Run | Wall | Peak RSS |
| --- | ---: | ---: |
| 1 | 2.38 s | 142.9 MiB |
| 2 | 2.42 s | 138.0 MiB |
| 3 | 2.64 s | 140.8 MiB |
| index DB | — | 16,846,848 B = **16.07 MiB** |

All three within budget.

A measurement trap worth naming: the very first cold index on a freshly created
worktree took **31.67 s / 256.7 MiB**, because the OS page cache was empty for
13,109 just-written files. §7.1 explicitly forbids passing that off as indexing
cost, and a `find`-based warmup does not fix it — `find` only `stat`s files, it
never reads their contents. The numbers above are from runs where file contents
were already cached.

### Warm queries

| Query | median | p95 | Peak RSS | Output | Budget (median/p95/RSS) |
| --- | ---: | ---: | ---: | ---: | --- |
| root overview | 100 ms | 100 ms | 48.0 MiB | 1,813 B | 250 / 400 / 96 |
| file overview | 100 ms | 100 ms | 49.3 MiB | 5,501 B | 250 / 400 / 96 |
| definition | 100 ms | 150 ms | 49.6 MiB | 3,683 B | 250 / 400 / 96 |
| symbol search | 110 ms | 110 ms | 50.2 MiB | 1,335 B | 250 / 400 / 96 |
| references | 210 ms | 210 ms | 57.6 MiB | 6,879 B | 500 / 1000 / 128 |
| **map --depth 2** | **110 ms** | **130 ms** | **51.4 MiB** | 10,637 B | 500 / 1000 / 128 |
| callers | 210 ms | 220 ms | 59.8 MiB | 7,094 B | 750 / 1500 / 192 |
| callees | 110 ms | 110 ms | 56.1 MiB | 8,538 B | 750 / 1500 / 192 |

`map --depth 2` individual runs: 110, 110, 110, 110, 110, 110, 110, 120, 130 ms.

### Regression fixed in this commit

The prior acceptance run failed the `map` budget. Measured on the same pinned
corpus, same host:

| `cx map --depth 2` | median | p95 | Peak RSS |
| --- | ---: | ---: | ---: |
| before (per-import corpus scan) | 3400 ms | 3440 ms | 50.3 MiB |
| after (prebuilt suffix lookup) | **110 ms** | **130 ms** | **51.4 MiB** |
| budget | 500 ms | 1000 ms | 128 MiB |

`callers` improved 380 → 210 ms and `callees` 350 → 110 ms as a side effect of
building the lookup once per query instead of once per call site.

Output is **byte-identical across the change**: 36 subsystem rows, every
`depends_on` set, every `external_imports` count, and the same 74 imports
reported as matching several files and therefore left unresolved. `callers`,
`references` and `definition` payloads are byte-identical too.

### Incremental and residency

Measured on `0e853f3` against the pinned corpus, 9 runs each:

| Metric | median | p95 | Budget |
| --- | ---: | ---: | ---: |
| `cx refresh <one path>` | 20 ms | 20 ms | 500 ms |
| metadata auto-discovery of one edit | 345 ms | 349 ms | 750 ms |
| `--fresh verified` over the corpus | 210 ms | 250 ms | 2.0 s |

100 mixed queries (`overview`, `symbols`, `definition`, `map`, `callers`):

- 100/100 exit 0
- **0 residual `cx` processes**, no watcher or daemon started
- index generation 1 → 1 (pure reads never bump it)
- index DB size unchanged
- peak RSS first-10 avg 51.3 MiB, last-10 avg 51.1 MiB, max 61.0 MiB — no growth
  with iteration

### Default output budgets (§7.3)

| Query | Bytes | Cap | `page.total` |
| --- | ---: | ---: | ---: |
| root overview | 1,813 | 4,096 | 14 |
| EcsWorld symbol search | 1,334 | 8,192 | 4 |
| references default page | 1,924 | 16,384 | 10 |
| map default page | 3,658 | 16,384 | 11 |
| callers default page | 7,092 | 16,384 | 26 |
| callees default page | 8,536 | 16,384 | 30 |

## 5. Contract results

All verified on an isolated copy of `tests/fixtures/agent_corpus` with a
throwaway `CX_CACHE_DIR`.

- **JSON envelope** — root is always an object with the same 8 keys
  (`schema_version`, `query`, `freshness`, `page`, `results`, `warnings`,
  `next_queries`, `error`) across success, empty, paginated and error cases.
  `schema_version` = 1. Empty success = `results: []`, `error: null`, exit 0.
  Query failure = non-null `error.code`, exit 1. Argument error = exit 2 with
  empty stdout. Under `--json`, stderr does not restate result facts; without
  `--json` the readable truncation hint remains
  (`cx: 4/12 symbols | … | --offset 4 for more | --all`).
- **Symbol roles** — `validate_param` yields exactly 1 definition + 1
  declaration with the definition ordered first; `--role definition` returns only
  the implementation and `--role declaration` only the prototype; Markdown
  headings use the `heading` role.
- **Qualified identity** — 12 `run` locations resolve to 9 distinct qualified
  names, including `alpha::run`, `beta::run`, `ange::EcsWorld::run`,
  `beta::Runner::run`, and TypeScript `Tickable.run` / `AlphaRunner.run` using
  the `.` separator. Unmodelled languages report an empty qualified name meaning
  *unresolved*, never *top-level*.
- **Freshness** — pure reads do not advance the generation. With a byte-identical
  `st_mtime_ns` and identical size, `metadata` mode honestly reports
  `files_updated: 0` and serves the stale symbol while labelling itself
  `mode: metadata`; `--fresh verified` and `cx refresh <path>` both catch the
  same edit. A path outside the project root is rejected
  (`… is outside the project root, skipping`) and creates no second index.
- **References / relations** — 6 caller edges on the fixture, 2 unresolved; every
  unresolved edge lists its candidates and no edge claims `type_resolved`;
  `alpha::run()` and `beta::run()` on the *same line* remain two distinct edges;
  ambiguous `callees --name run` returns no rows plus the candidate list rather
  than reading an arbitrary body; `--depth` is rejected (exit 2).
- **Repository map** — vendor, generated and test paths are excluded before
  ranking, each reported with its count and the flag that restores it; the
  ranking basis (`ranked by dependents desc, then symbols desc, then name`)
  appears in `warnings` under `--json` and on stderr otherwise; low-information
  names (`run`, `get`, `new`, `name`) never appear in API samples.
- **Canonical paths** — `/tmp/…` and `/private/tmp/…` produce an identical
  `cx cache path`, identical query results, and a single index DB.

## 6. How the evidence artifacts are produced

The raw artifacts live in a `mktemp -d` directory created per run and are
therefore **ephemeral**; they are deliberately not committed. This run wrote to:

```text
/var/folders/2d/vy1wz1qs6qbf0vfc3wg9sprm0000gn/T/cx-accept2.hV0wZ2
```

Layout:

```text
$RUN/subject/env.txt        pwd, UTC timestamp, arch, rustc, cargo, commit
$RUN/subject/status.txt     git status --short of the clean checkout
$RUN/ev/<name>/cmd          exact argv of that command
$RUN/ev/<name>/exit_code    exit status, captured directly (never via a pipe)
$RUN/ev/<name>/stdout       full stdout
$RUN/ev/<name>/stderr       full stderr
$RUN/cold_{1,2,3}.txt       /usr/bin/time -l output per cold-index run
$RUN/map_t.txt              /usr/bin/time -l output, 9 map runs
$RUN/rss100.txt             peak RSS of each of the 100 mixed queries
$RUN/refresh.txt            /usr/bin/time -l output, 9 `cx refresh` runs
$RUN/verified.txt           /usr/bin/time -l output, 9 `--fresh verified` runs
$RUN/meta.txt               wall ms of 9 edit-then-query metadata cycles
$RUN/ground_truth.txt       git grep -n output used as independent ground truth
$RUN/install/bin/cx         binary from cargo install, re-queried
```

31 per-command evidence directories were captured for this run.

One harness limitation, recorded rather than hidden: the warm-query helper reused
a single scratch file, so only the **last** query's raw `/usr/bin/time` output
survives in `$RUN/w.txt`. Per-query warm medians in §4 are the values reported at
run time. `map`, cold index, the incremental metrics and the 100-query series each
have their own persisted series files and are fully re-derivable from them.

The incremental figures were measured in a second pass, in its own worktree at the
same commit `0e853f3`, because the first clean-checkout pass did not cover them.
They are not carried over from any earlier run: the standard forbids reusing
historical numbers as evidence, and the earlier dirty-tree run had reported
351 ms for metadata auto-discovery against the 345 ms measured here.

### Reproducing from scratch

```bash
# 1. Clean checkout of the graded commit
RUN="$(mktemp -d "${TMPDIR:-/tmp}/cx-accept.XXXXXX")"
git -C /path/to/cx worktree add --detach "$RUN/cx" 0e853f3
cd "$RUN/cx"

# 2. Gate A (a binary-only crate: --bins/--tests/--all-targets, no --lib/--doc)
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --bins
cargo test --locked --tests
cargo test --locked --all-targets --all-features
cargo build --locked --release
export CX_BIN="$RUN/cx/target/release/cx"

# 3. Pinned ANGE worktree + isolated cache and grammars
ANGE_REPO="$HOME/Documents/GodotProjects/ange"
ANGE_COMMIT=70fe1922de2f05bec4a94f5967c68a92a8320b1a
git -C "$ANGE_REPO" cat-file -e "${ANGE_COMMIT}^{commit}"     # must exist
git -C "$ANGE_REPO" worktree add --detach "$RUN/ange" "$ANGE_COMMIT"
export CX_CACHE_DIR="$RUN/cache"; mkdir -p "$CX_CACHE_DIR"
"$CX_BIN" lang add cpp && "$CX_BIN" lang list                 # cpp must be installed

# 4. Independent ground truth (does not involve cx)
git -C "$RUN/ange" grep -n validate_stmt_against_action_spec -- . > "$RUN/ground_truth.txt"
# keep only .cpp/.h/.cc/.hpp lines; a line whose trimmed text starts with
# // or * or /* is comment-only  ->  35 total, 28 code, 7 comment-only

# 5. The four graded counts
S=validate_stmt_against_action_spec
"$CX_BIN" --root "$RUN/ange" --json definition --name $S --role definition --all  # 1
"$CX_BIN" --root "$RUN/ange" --json references --name $S --context --all          # 28
"$CX_BIN" --root "$RUN/ange" --json callers    --name $S --all                    # 26
# comment false positives: intersect the returned (file,line) set with the
# 7 comment-only lines  ->  must be empty, and the set must equal the 28

# 6. Performance: warm the page cache first, then 1 warmup + 9 timed runs
CX_BIN="$CX_BIN" ./scripts/bench.sh "$RUN/ange" 9

# 7. Cleanup (also on failure)
git -C "$ANGE_REPO" worktree remove --force "$RUN/ange"
git -C /path/to/cx worktree remove --force "$RUN/cx"
```

`scripts/bench.sh` covers cold index, the warm queries, **`map --depth 1/2`,
`callers`, `callees`**, and one-file incremental refresh. It writes into a
throwaway `CX_CACHE_DIR` and must be pointed at a disposable worktree, because it
mutates one indexed file to time incremental refresh and then restores it.

### Guard against the regression this commit fixed

`src/map.rs::include_resolution_does_not_scale_with_corpus_size` resolves 6,000
imports against 4,000 indexed files. It asserts the workload's own outcome counts
(3,000 resolved, 3,000 external) before checking the clock, so a no-op cannot
pass on timing alone. It completes in ~20 ms; a reintroduced per-import corpus
scan needs tens of seconds in a debug build.

```bash
cargo test --bins include_resolution_does_not_scale -- --nocapture
```

## 7. Residual risks

- Performance is graded only on Apple arm64/macOS. On any other host the correct
  verdict is `PASS_CORRECTNESS_PERF_UNGRADED`, not `PASS`.
- `page.total` for a whole-corpus `symbols` query depends on which grammars are
  installed and is not a fixed acceptance value; only the four counts in §3 are.
- The new CI `fmt` job and the widened `clippy --all-targets --all-features` pass
  on rustc 1.98.1. A newer toolchain in CI may surface formatting or lint
  diagnostics this one does not; that would be a toolchain difference, not a
  regression in this commit.
- `Cargo.toml` remains at `0.7.2` even though the `--json` envelope is a breaking
  change for consumers. The version bump is a deliberate open decision, and
  `auto-tag` will skip while the existing `v0.7.2` tag matches.
