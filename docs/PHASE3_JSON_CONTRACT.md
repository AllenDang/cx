# Phase 3 — JSON v1 contract

> Roadmap reference: `docs/AGENT_NATIVE_DEVELOPMENT_ROADMAP.md` §3.4, §6, §11 Phase 3.
> `schema_version = 1`. TOON output is unchanged.

## Problem

Three separate contract defects made `--json` unsafe to build on:

1. **The root type changed with the data.** A bare array when everything fit, an
   object (`{total, offset, limit, results}`) once truncated *or* offset. The
   same command returned two different shapes depending on how many results
   existed (§3.4).
2. **Empty results were unparseable.** A successful query with no matches printed
   *nothing* on stdout and `cx: no matches` on stderr, so an agent could not
   distinguish "nothing matched" from "the command produced no output" (§6.2).
3. **Failures were stderr prose.** `file not in index: …` had no machine-readable
   code, and truncation was only ever visible as a stderr hint — never in the
   payload (§6.2, §6.3).

## Change

One envelope, emitted by every command, with a **fixed key set**:

```json
{
  "schema_version": 1,
  "query":   { "kind": "symbols", "subject": "run" },
  "page":    { "total": 12, "offset": 0, "limit": 4, "truncated": true },
  "results": [ … ],
  "warnings": [],
  "next_queries": [
    "cx --json symbols --name run --limit 4 --offset 4",
    "cx --json symbols --name run --all"
  ],
  "error": null
}
```

Design decisions worth naming:

- **Nothing appears or disappears.** `results`, `warnings`, `next_queries` are
  always arrays; `error` is always present and `null` on success. A consumer
  never branches on key existence. `limit` is `null` rather than omitted when
  unlimited.
- **`next_queries` are runnable, not prose.** They are built by rewriting the
  *actual* argv with pagination flags replaced (`command_with_offset`,
  `command_with_all`), with shell-quoting for globs and spaces. A test executes a
  suggested command and asserts it returns exactly the omitted rows — a
  hand-written hint string could not be verified this way.
- **Empty ≠ error.** Zero results is `results: []`, `error: null`, exit 0. A
  failure sets `error.code` and exits 1. Codes: `file_not_indexed`,
  `unsupported_file_type`, `no_indexed_files`, `grammar_not_installed`.
- **Ambiguity is reported, not resolved silently.** `cx definition --name run`
  matching 12 candidates now emits a warning naming the count, instead of
  quietly returning an ordered list that looks authoritative. Phase 5 replaces
  the warning with qualified identity.
- **Under `--json`, stderr is quiet.** The payload is authoritative, so cx no
  longer duplicates `cx: no matches` / pagination hints on stderr. Without
  `--json`, every existing stderr message and hint is byte-for-byte unchanged —
  pinned by paired TOON assertions in the tests.

Exit codes are documented, not changed: `0` success (including zero results),
`1` query failure, `2` argument-parsing error (from clap).

## Implementation

- `src/output.rs` owns the contract: `SCHEMA_VERSION`, `Envelope`, `QueryInfo`,
  `PageInfo`, `ErrorCode`, `ErrorInfo`, `print_error_json`, and the argv
  rewriters. `PaginatedJson` and `needs_envelope` are gone.
- `src/query.rs` routes every command through two helpers — `emit()` for success
  and `fail()` for failure — so no call site can invent its own shape.
  `resolve_file_filter` now returns a structured `QueryFailure` instead of
  printing and returning an exit code.
- Early `return 0` on empty result sets was removed so empty flows through the
  normal emit path. `definition` keeps its bespoke plain-text rendering for the
  non-JSON case.
- Narrowing hints became named constants (`NARROW_SYMBOLS` etc.) instead of
  repeated literals; `--role` was added to the symbols hint.

## `freshness` is deliberately absent

Roadmap §6.1 shows a `freshness` block in the envelope. It is **not** in
schema_version 1: index generation, freshness mode and files-checked counts are
Phase 4's work, and emitting a half-computed version now would be exactly the
"pretend it is a fact" failure §12 warns about. Adding the key in Phase 4 is
additive and does not bump `schema_version`; the corpus test asserting no
freshness field is still green and will flip in that phase.

## Tests

- 4 new unit tests in `src/output.rs`: the exact key set of a success envelope,
  `limit: null` plus omitted `subject`, and shell-quoting behavior.
- 5 new/flipped corpus tests: `empty_result_emits_an_envelope_with_zero_results`
  (was `…_currently_emits_no_json_body`),
  `json_root_type_is_stable_across_pagination` (was
  `json_root_type_changes_with_pagination`),
  `truncated_page_suggests_runnable_next_queries`,
  `ambiguous_definition_reports_a_warning`,
  `empty_directory_scope_is_an_error_not_an_empty_result`, plus error-code
  assertions on the two failure tests.
- `tests/support/mod.rs` gained envelope accessors (`results()`, `page()`,
  `error_code()`); `tests/integration.rs` gained `json_results()`/`json_page()`.
- Every JSON assertion across 3 test binaries was moved onto the envelope, and
  the tests whose premise was "bare array" were rewritten to assert the stable
  object plus the pagination facts they were really checking.

Suite: 170 unit + 26 corpus + 62 integration + 7 path identity = 265 passing,
`cargo clippy --all-targets` clean.

## Cost

`./scripts/bench.sh . 9`, same host as earlier phases. Latency is unchanged
(a first noisy run showing ~52 ms was a concurrent release build; two clean runs
follow):

| Metric | Phase 2 | Phase 3 |
| --- | --- | --- |
| cold build | 0.10 s / 33.6 MiB | 0.08–0.10 s / 34.7 MiB |
| index size | 1,056,768 B | 1,056,768 B |
| root overview | 34 ms | 31–33 ms |
| definition | 33 ms | 32 ms |
| references | 64 ms | 65–67 ms |
| symbol search | 33 ms | 31–33 ms |

TOON output byte counts moved only because the repository itself gained
documentation (the benchmark indexes cx's own source); the TOON format is
untouched. JSON payloads grow by the envelope's fixed overhead — roughly 200
bytes per response — in exchange for a shape that never has to be sniffed.
