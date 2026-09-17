# Independent availability finding: unsupported dirty paths poison subsequent queries

Observed during the adoption investigation, not caused by DeepSeek choosing a
bad tool argument. Fresh isolated capability probes succeeded. The parent
session failed before a requested `cx_definition` query executed:

```
dirty-path refresh failed; CX query was not executed: cx refresh returned a malformed path status
```

## Reproduction

After writing benchmark JSON/MJS files, invoke a cx query through the Pi extension
with those paths queued in DirtyPathCoordinator. The bundled native binary's
named refresh returns e.g.:

```json
{"file":"bench/adoption/cases.json","status":"unsupported_file_type"}
```

A direct refresh of `bench/adoption/round3/observer.ts` succeeds (`updated`).
Refreshing all 35 benchmark files returned 18 `unsupported_file_type`, 9
`unchanged`, 8 `updated` rows with `error: null` and process exit 0.

## Source evidence

- `src/index.rs`, named-path scan: reads beneath the canonical root first;
  unknown grammar/type sets `failure = Some("unsupported_file_type")`.
- `src/query.rs::refresh_report`: emits that named-path status verbatim.
- `extensions/pi-cx/tools.ts::assertNamedRefresh`: only accepts updated, removed,
  unchanged and not_indexed. It rejects the benign non-indexable status.
- The caller restores all pending paths on failure, so the same unsupported
  files block every subsequent query. A whole-project refresh does not clear
  them either because it follows with named-path proof.

## Proposed narrow repair

Recognize `unsupported_file_type` as a confirmed **non-indexable path**, not as
an indexed/refreshed source file. Consume that queued path, surface skipped-path
metadata, and allow subsequent source queries. Keep exact per-path coverage,
duplicate/unexpected-path checks, and rejection of outside_root/read_failed/
unverified/unknown statuses. Do not weaken symlink or read-error checks.

When computing dirtyRefresh statistics, intersect skipped rows with the pending
path set: an explicit refresh may include additional caller-specified paths, so
subtracting all skipped rows from only the pending count would be incorrect.

Regression cases: mixed source + JSON automatic refresh; unsupported-only
refresh; explicit refresh with extra unsupported paths; real bundled-binary
integration; security/unverified statuses remain fail-closed with pending paths
retained. Runtime code is left unchanged while the current adoption experiment
is active, to preserve its controlled conditions.
