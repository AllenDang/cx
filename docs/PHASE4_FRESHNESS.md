# Phase 4 — freshness contract

> Roadmap reference: `docs/AGENT_NATIVE_DEVELOPMENT_ROADMAP.md` §4.4, §7, §11 Phase 4.
> `INDEX_VERSION` 9 → 10. `freshness` added to the JSON envelope (additive; `schema_version` stays 1).

## Problem

An agent could get a correct-looking answer and have no way to tell whether it
reflected the edit it just made. Nothing in the output revealed how many files
were checked, how many were re-parsed, whether the check used mtime or content,
or which index state answered the query (§4.4). Worse, the only check available
compared mtime alone, so an edit that preserved mtime was invisible — silently.

## Change

### Generation counter

`META_TABLE` now stores a monotonic `generation`, bumped inside the same write
transaction that persists index changes, and only adopted in memory *after* the
commit succeeds. A pure read never advances it.

### Three verification modes

| Mode | How | Selected by |
| --- | --- | --- |
| `metadata` | size + high-resolution mtime, no file reads | default |
| `verified` | content hash of every indexable file | `--fresh verified` |
| `paths` | content hash of exactly the named files | `cx refresh <paths>` |

`FileEntry` gained `size` and `content_hash`. Both are recorded from the bytes
the indexer already read in order to parse, so they cost no extra I/O, and the
stored values always describe the *parsed* content rather than a possibly-racing
earlier `stat`. `metadata` mode now compares size as well as mtime, which closes
the common coarse-clock case for free.

### Reported freshness

Every envelope carries:

```json
"freshness": {
  "generation": 8, "mode": "metadata",
  "files_checked": 48, "files_updated": 0,
  "files_removed": 0, "files_skipped_missing_grammar": 0
}
```

The mode reported is the mode that actually ran — a `metadata` answer never
claims to be content-verified.

### `cx refresh`

```text
$ cx refresh src/a.cpp src/b.h
[2]{file,status}:
  src/a.cpp,updated
  src/b.h,unchanged
cx: generation 9 | mode paths | checked 2 | updated 1 | removed 0
```

Per-path status (`updated` / `removed` / `unchanged` / `not_indexed`) is the
mechanical evidence §7 asks for: the agent named the files, cx says what happened
to each and which generation they landed in, and a later query reporting that
same generation proves the edit is included. With no arguments, `refresh`
degrades to a whole-project content verification.

### Scan restructure

The old code walked the tree twice when an update was needed (once in
`needs_update`, again in `incremental_update`). There is now a single
`scan_disk` → `DiskScan` → `apply_scan` pipeline, which also removed the
duplicated staleness logic that had drifted between the two walks.

## A bug this phase found

`cx refresh <path>` initially derived the project root from its first path
argument (matching the other commands). For a path outside the project that
silently retargeted cx at whatever directory the stray path lived in — building a
brand-new index there instead of reporting the mistake. `refresh` now resolves
the root from `--root`/cwd only, and a stray path is reported as
`outside the project root`. Caught by
`refresh_ignores_paths_outside_the_project`.

## Honesty about the blind spot

`metadata` cannot see an edit that preserves both size and mtime. Rather than
quietly hashing everything by default (§7 explicitly warns against unmeasured
full hashing) or pretending the check is stronger than it is, cx:

- keeps `metadata` as the fast default,
- labels every answer with the mode that produced it,
- offers two ways to close the gap (`--fresh verified`, `cx refresh <paths>`),
- and has a test asserting the stale answer *is* returned with
  `mode: metadata` / `files_updated: 0` — the gap is pinned, not papered over.

## Tests

`tests/freshness.rs`, 13 tests covering exactly the §7 acceptance list:

- generation advances on change, stays put across pure reads
- **content change with identical size and identical mtime**: missed by
  `metadata` (asserted), caught by `--fresh verified`, and caught by
  `cx refresh <path>`
- content change with same size but new mtime → caught by `metadata`
- size change with preserved mtime → caught by `metadata`
- new / deleted / renamed files, with `files_updated` and `files_removed` counts
- refresh per-path status, whole-project refresh, stray path outside the root,
  and the "nothing changed" report
- 4 concurrent readers agree on one generation
- a writer plus 4 concurrent readers: all exit 0, index still correct afterwards
- an unknown extension is not counted as a missing grammar

Plus 2 corpus tests flipped (`freshness_is_reported_with_every_result` replaces
`freshness_is_not_observable_in_output`), the envelope key-set test updated, and
2 new unit tests for `content_hash` and the extended `FileEntry` roundtrip.

Suite: 171 unit + 27 corpus + 13 freshness + 62 integration + 7 path identity =
280 passing, `clippy --all-targets -D warnings` clean.

## Cost

Measured on a synthetic tree sized to the roadmap's ANGE C/C++ corpus
(2,000 files, 31 MB), warm query, release build:

| Mode | Warm query |
| --- | --- |
| `metadata` | 114–119 ms |
| `verified` | 141–144 ms (+23%) |
| `cx refresh <one path>` | ~40 ms |

Hashing 31 MB costs roughly 25 ms (~1.2 GB/s), which is why `verified` is a
viable opt-in but still the wrong default. On the cx repo itself (48 files) the
two modes are indistinguishable at 13 ms.

`./scripts/bench.sh . 9` shows no regression for the default path; the extra
per-file work in `metadata` mode is one `len()` call on metadata already fetched.
