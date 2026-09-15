# Stage R implementation and validation report

> Historical scoped implementation-gate report. The subsequent
> [real-task ANGE evaluation](ANGE_TASK_REPORT.md) found unresolved correctness
> defects and **failed** the adoption check. The PASS below records those earlier
> commands, not the current readiness to advance beyond Stage R.
> The [subsequent fixes](ANGE_FIX_REPORT.md) now close those identified defects;
> their new validation is reported separately, without rewriting this history.

## Verdict and scope

**Engineering verdict: PASS — scoped Stage R implementation gates only.**
The final Rust gates, exact fixtures, 11 mutation controls, fixed ANGE checks,
and measured reference-host budgets passed. This is **not** a full-project
release PASS: clean-checkout release/install verification, packaged Pi-binary
integration, and other-platform gates were not performed. Nothing was committed
or published, and `impact`/`changes`/`context` were not started.

**Effectiveness conclusion: evidence insufficient for real task benefit.**
Correctness improvements are demonstrated by independent fixtures and assertion
kills, not by an A/B/C held-out task experiment. There is no claim of lower total
agent cost, better test selection, or compiler-level analysis.

## Subject and preservation

- Baseline: `18337372cc95092fd9daca45936159e3d28652d9`, branch `master`.
- Initial status: only untracked `docs/AGENT_TASK_ANALYSIS_PLAN.md`; preserved
  without changes. Its SHA-256 is
  `3e0929af5bff465e478609dd0f8ef04edc4ae317d040143945e1bb3c377692d7`.
- Host: Apple arm64/macOS, Darwin 25.6.0 (`RELEASE_ARM64_T6000`).
- rustc `1.98.1 (48a229cea 2026-09-01)`; cargo
  `1.98.1 (797e8a9bc 2026-08-05)`; Node 26.8.2; npm 11.19.1.
- Started: `2026-09-15T03:39:44Z`.
- ANGE: `70fe1922de2f05bec4a94f5967c68a92a8320b1a`, materialized by
  `git archive`, not by editing its main worktree. Incremental measurements use
  an additional disposable copy, restored byte-for-byte and removed.
- No delegation, daemon, schema/index version bump, dependency change, or release.

Raw evidence is local, outside the repository:

```text
/tmp/cx-stage-r.uLMFKA/
  baseline.txt
  red*.stdout / red*.stderr / red*.exit
  final2-gates.json / final2-*.stdout / final2-*.stderr / final2-*.exit
  mutations-final/report.json / <mutation>.stdout / <mutation>.stderr
  performance-final/report.json / {baseline,candidate}/<query>.<stream>
  repeat100/report.json / <iteration>.<stream>
  bin/{baseline,candidate}
```

The report retains earlier failed/invalid attempts as diagnostics; only final
artifacts above establish the verdict. Logs include command/exit/time and both
streams; performance logs additionally include CPU and RSS. Temporary Cargo build
caches are disposable, not part of the evidence needed to interpret the results.

## Implementation and contracts

See [the foundation contract](../../docs/STAGE_R_RELATION_FOUNDATION.md).
Shared relation inputs are command-local, hash-consistent, and keyed by precise
sites. No resolved-edge cache needs cross-file invalidation. Query-specific
parsing avoids the cost of eagerly parsing the whole repository, while all
supported indexed source contents are checked before candidate use.

Old JSON envelope and edge fields remain intact. Coverage is a bounded structured
JSON record inside a warning string. Unsupported/degraded scope is explicit;
name-only, unknown-receiver, macro, alias, and ambiguous cases are not upgraded
into stronger evidence. Finite output pages preserve every candidate and advance
by the actual row count; irreducible overflow is explicitly reported.

## Red controls and oracle corrections

- Initial `relation_identity`: **0 passed / 12 failed**, before production edits.
  Failures reproduced same-label overload merging, equal-name subject union,
  declaration suppression, nested/closure ownership, unknown receivers/macros,
  qualifier substring/fallback, incorrect lexical confidence, same-line call
  deduplication, stale-content mixing, absent coverage, and cross-file candidate
  invalidation.
- Added binding controls failed before their blockers were implemented.
- Valid sites outside parse errors and bounded coverage failed before the
  corresponding changes.
- Two multiline declaration/overload cases failed before full-source signatures
  and combined local/include candidate visibility were implemented.
- The large-candidate page control failed with **29,411 bytes** before page
  budgeting; the final test follows every emitted next page and compares the
  concatenation with the full independent 20-call fixture result.
- Final new targets: **20 identity tests**, **6 coverage/budget tests**;
  **7 snapshot/typed-identity unit tests** and **1 additional legacy-target test**.
  A total of 34 new regression controls were added.

Corrections to hand-written test/harness expectations were recorded rather than
hidden: the closure's `||` begins at byte 34, not 32; call offsets include the
space following `{` (26/22, not 25/21). These were checked directly against the
source literal bytes. Cold and warm results share facts/generation, but cold
`files_updated` is 1 and warm is 0. Missing-grammar isolation must disable the
language pack's bundled-library lookup **and** its automatic manifest download.
These are oracle/harness corrections, not resolver-output-derived gold.

Two old assertions changed for independently justified identity fixes: 10 actual
callable entities replace 8 display labels, and cross-language `alpha::run` no
longer denotes one body. The original corpus is unchanged; a uniquely qualified
C++ body still tests successful scope selection.

## Mutation results

All 11 compiled mutants were killed by the intended test, not by compilation or
unrelated failures. `mutations-final/report.json` records exact patches, source
hashes, commands, named tests, and full actual failure blocks.

| Mutant | Intended control | Actual failing assertion |
| --- | --- | --- |
| Collapse site identity to name | identical-label overloads | `to = run`, expected empty |
| Choose first ambiguous target | identical-label overloads | `to = run`, expected empty |
| Ignore source content hash | metadata blind edit | stale row count 1, expected 0 |
| Delete coverage warning | partial syntax report | mandatory coverage disclosure missing |
| Accept recovery tree as complete | partial syntax report | `to = leaf`, expected empty |
| Attribute closure to outer | closure ownership | outer callees count 1, expected 0 |
| Drop unrelated declarations | distinct scoped declaration | empty target, expected `a::run` |
| Resolve despite incomplete scan | missing grammar snapshot | `result.to.is_none()` fails |
| Bind receiver/macro by name | indirect syntax calls | macro resolves to `run`, expected empty |
| Reuse truncated declaration outline | multiline declaration | empty callees, expected `target` |
| Remove output page budget | large candidate evidence | 29,411-byte output exceeds page bound |

No surviving mutant in this set. BFS direction/visited, old-side diff, ranking,
and test-selection mutants are **not applicable/implemented** in Stage R; no
claim is made that those future mechanisms have been validated.

The first mutation harness control was INVALID because its source copy lacked a
`.git` marker required by existing root-discovery unit tests. It was repaired and
rerun; those unrelated failures were not counted as kills.

## Final Rust gates

All commands below exited **0**, with separate stdout/stderr and timing records
under `final2-*`:

| Gate | Executed tests |
| --- | ---: |
| `cargo fmt --all -- --check` | n/a |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | n/a |
| `cargo test --locked --bins` | 211 |
| `cargo test --locked --tests` | 387 total |
| `cargo test --locked --all-targets --all-features` | 387 total |
| Individually: fixture_corpus / path_identity / freshness | 27 / 7 / 14 |
| Individually: qualified_identity / map / relations / integration | 11 / 12 / 15 / 62 |
| Individually: relation_identity / relation_coverage | 20 / 6 |
| `cargo package --locked --allow-dirty` | build verification successful |
| `git diff --check` | successful |

The all-targets run also executes concurrency (1) and HTML (1). No ignored or
failed tests in the final runs. `--lib`/`--doc` are inapplicable: this is a
binary-only crate. Pi scripts were not used to pretend that the old release
asset contained the new Rust behavior.

An intermediate full-suite failure exposed benchmark Python `main` functions
being indexed in the production repository. The evaluation directory now carries
`.cx-ignore`; the unchanged integration assertion passes. The transient Clippy
`int_plus_one` warning was fixed without suppression. Earlier logs remain in the
evidence directory.

## Fixed ANGE exact checks

Both binaries use the same versioned grammar directory; per-library SHA-256s
are recorded in `performance-final/report.json`. No grammar downloads are timed.
The input archive SHA-256 is
`fa2d30dd9f923f14cb7e71dffc62e6ca1d7f34f6908b63894ded6dd013e019e5`;
the source-content manifest SHA-256 is
`0123675298afa0a900990fc02173fe9711e7eea93f221591e0f3bfc4c8cc4e43`.

| Check | Expected | Final |
| --- | ---: | ---: |
| Definition of fixed subject in param_validator.cpp | 1 | 1 |
| Structural references | 28 | 28 in 10 file rows |
| Incoming call sites, each `evidence: call` | 26 | 26 |
| Outgoing syntactic sites, source-audited | 31 | 31 |
| Pure-query generation changes in 100 calls | 0 | 0 |
| Residual candidate processes after 100 calls | 0 | 0 |

The first cost harness incorrectly treated the grouped references `page.total`
as occurrence count. It was corrected to assert 10 file rows **and** the sum of
`refs == 28`; the 28-occurrence gold was not weakened. A subsequent diagnostic
found multiline declaration outlines causing an ambiguous empty callee result.
After reducing this to red fixtures, that production problem was fixed, and the
runner now rejects an empty callee response and checks all 31 source positions.
The extra site relative to baseline's 30 is the second `key()` call on line 362.

Coverage is not globally complete: the final caller query discloses 7 recovery-
parsed files and 1,797 unsupported-language files, so all its targets remain
conservative syntax evidence. The callee source parses without recovery but
retains the unsupported-language disclosure; 26 of its 31 targets are unresolved.
Correct counts are not a claim of compiler-resolved or runtime-complete reach.

## Cost, including regressions

Reference host; release binaries; one warmup then **9 samples** each. p95 equals
max with this sample size. Measurements are sequential, not concurrent with
compilation. These are bytes, **not model tokens**. A/B/C complete-task budgets,
failures/fallback costs, and randomized task trials have not been measured.

| Query | Baseline median / p95 ms | Candidate median / p95 ms | Candidate max RSS MiB | Output bytes baseline → candidate |
| --- | ---: | ---: | ---: | ---: |
| Root overview | 90.6 / 91.0 | 90.7 / 95.1 | 47.6 | 1,813 → 1,813 |
| File overview | 89.7 / 92.1 | 90.7 / 94.1 | 48.6 | 5,501 → 5,501 |
| Definition | 90.0 / 92.5 | 90.3 / 92.8 | 47.6 | 3,683 → 3,683 |
| Symbols | 99.0 / 100.3 | 100.4 / 104.5 | 47.7 | 1,334 → 1,334 |
| References | 192.0 / 196.3 | 192.7 / 199.5 | 56.1 | 1,926 → 1,926 |
| Callers (`--all`) | 196.2 / 197.8 | 200.4 / 203.4 | 70.1 | 7,094 → 9,450 |
| Callees (default page) | 101.6 / 104.0 | 185.4 / 189.3 | 71.3 | 8,536 → 16,372 |
| Map depth 2 | 99.3 / 102.9 | 101.8 / 105.0 | 52.0 | 10,637 → 10,637 |

Callees is about **83% slower** in median, though still well below the 750 ms
reference-host budget. It now checks candidate source identities across the
indexed corpus. Its default page is shorter than the full 31-site answer; that
page result is not advertised as a cheaper completed task. The full candidate
callee response is 25,135 bytes and is available via pagination or `--all`.
Default callers is 9,448 bytes, references 1,924 bytes; default callees is 16,372
bytes. No necessary candidate list was cut to achieve these numbers.

| Metric | Baseline | Candidate |
| --- | ---: | ---: |
| Cold index wall | 2.058 s | 2.041 s |
| Cold max RSS | 129.0 MiB | 132.4 MiB |
| Index DB | 16,846,848 B | 16,846,848 B |
| One-file metadata refresh | 302.7 ms | 305.0 ms |
| One-path refresh | 177.9 ms | 178.7 ms |
| Whole-corpus verified refresh | 476.7 ms | 477.0 ms |

All measured reference-host absolute budgets are met. In the extra 100 mixed
queries, generation remained 1, DB size stayed 16,846,848 bytes, maximum RSS was
70.4 MiB, and every process exited successfully with none remaining.

An earlier eager full-parse implementation cost **4.36 s** and discarded valid
sites in recovery-parsed files (2 callers instead of 26). It was rejected, not
claimed as progress. Query-selected parsing plus honest partial extraction was
retained after red fixtures and the repeated final measurement.

Binary SHA-256s:

```text
baseline  822c07bd545cd20802e4561ccd05f6370819f821b7a7867db2a938c738e27460
candidate 10093335b01ebcc723f5e210fe50212d3e253f77aded52ea9e071ec864217244
```

## Remaining work and non-claims

- No six-repository/72-question frozen task set, held-out scoring, mechanical B
  baseline, tokenizer measurement, or agent-in-the-loop experiment. No general
  task-benefit verdict can be drawn from these one-hop measurements.
- No compiler-equivalent declaration association, general name binder, template
  instantiation, transitive-include reasoning, or runtime call/test coverage.
- Indexed candidate symbols are not all re-parsed for every one-hop request;
  parse coverage is explicitly query-scoped. New-file races and metadata blind
  spots outside the read set are not turned into a fictitious verified worktree.
- Legacy equal-qualified subjects still cannot be selected by exact file/site;
  they are refused honestly. Future graph-wide traversal needs its own complete
  snapshot/coverage and traversal-budget contract.
- Extremely large indivisible evidence can exceed the 16 KiB page target, with
  explicit overflow disclosure. Tests pin preservation and forward progress.
- No clean-checkout install/release or packaged Pi integration was performed.
  macOS evidence does not substitute for Linux/Windows validation.
- Next step requires user confirmation: independently design Stage I subjects,
  typed traversal, depth/node/edge limits, and gold **before** adding `impact`.

Modified production files: `src/main.rs`, `src/relation_index.rs`,
`src/relations.rs`, `src/language/extract.rs`, `src/language/mod.rs`, `src/query.rs`.
Tests/docs/runners are additive except the two explained legacy identity
expectations. The user's original plan remains untracked and untouched; the
implementation remains uncommitted.
