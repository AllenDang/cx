# Task-tool cache and compact-output optimization

## Result

The same frozen ANGE developer pilot remains 4/4 sufficient for every arm. Against the
strong mechanical baseline, the optimized task tools now improve all measured complete-flow
costs in this sample:

| Group | Sufficient | Calls | Request + stdout | cl100k tokens | Total wall median |
| --- | ---: | ---: | ---: | ---: | ---: |
| A old relation-first | 4/4 | 23 | 46,438 B | 13,293 | 4.901 s |
| B strong mechanical baseline | 4/4 | 21 | 44,411 B | 12,659 | 4.631 s |
| C optimized context/impact/changes | **4/4** | **6** | **36,620 B** | **9,722** | **2.122 s warm median** |

Relative warm C vs B: **71.4% fewer calls, 17.5% fewer bytes, 23.2% fewer measured tokens,
and 54.2% lower wall time**. The first C repeat lazily built the task sidecar and took 5.207 s,
versus B's 4.650 s; subsequent complete-flow repeats were 2.122/2.102 s. Across all three
repeats including preparation, C totals 9.431 s versus B 13.910 s. Preparation is reported,
not hidden as a free prebuilt index. This supersedes the earlier cost conclusion while preserving it.

Token counting is offline `tiktoken 0.12.0`, `cl100k_base`, summing each request JSON and
stdout separately. It excludes tool schemas, system prompts, stderr, model output and cache
protocol overhead, so it is a precise communication-token measure for these artifacts, not
an end-to-end agent token bill.

## What changed

1. **Content-bound raw call facts built lazily.** Calls, qualifiers, owner ranges, parse
   degradation and line starts come from one AST/content pass when the first task query needs
   them. Impact resolves cached call sites, reading and hash-validating only files relevant to
   visited names. Basic indexing and basic queries do not load or build these facts.
2. **Compact sidecar format.** File-local call names/qualifiers use dictionaries and u32
   locations/flags, then one zstd-compressed sidecar is atomically generation-bound to redb.
   `INDEX_VERSION=16`, task-fact format 2. Missing/stale/corrupt sidecars rebuild lazily;
   source refresh invalidates them; `cx cache clean` removes both files.
3. **Context metadata fast path.** Exact names, paths, or complete symbol/path subwords first
   search indexed metadata and read/hash only selected evidence files. Under metadata freshness,
   full candidate-universe verification is explicitly partial; Pi defaults impact/context to
   verified. Descriptive body-only searches still take the full lexical route.
4. **Compact task JSON by default.** JSON is compact-encoded. Impact keeps one primary witness
   and evidence flags; changes omits raw hunks/hashes/modes/signatures unless `--detail full`;
   context keeps match provenance but omits repeated excerpt text/signatures. `analysis.detail`
   and `detail_omitted` disclose every omitted class. Full output remains tested and available.

No task, gold, ranking weight or expected answer was modified. An intermediate name-indexed
binding/line cache improved latency without communication; it is preserved. Subsequent fact
caching/output compaction produced the final result.

## Per-task final C

| Task | Calls | Bytes | cl100k tokens | Wall median |
| --- | ---: | ---: | ---: | ---: |
| Reader-suffix navigation | 2 | 6,798 | 1,772 | 0.724 s warm (3.826 s first build) |
| Trailing-count 3-hop navigation | 2 | 13,722 | 3,597 | 0.739 s |
| Utility extraction addition | 1 | 8,014 | 2,177 | 0.318 s |
| Utility extraction rollback | 1 | 8,086 | 2,176 | 0.321 s |

All result root/reach/depth or file/function/change sets match the frozen gold exactly.

## Storage and preparation trade-off

On the 5,005-file current ANGE corpus after one task query:

- Baseline/optimized redb: 33,689,600 B (unchanged).
- Compressed task sidecar: 8,760,235 B.
- Total after opt-in task preparation: 42,449,835 B, **26.0%** above baseline.
- Basic cold indexing does not build the sidecar.

On the fixed 3,908-file acceptance corpus, the final basic candidate cold index was 2.267 s /
140.3 MiB and kept the same 16,846,848 B redb with no sidecar, versus baseline 2.421 s /
135.2 MiB. Basic warm queries remain inside existing budgets; metadata/path/verified
incremental refreshes were 384/217/533 ms. Thus the opt-in cache no longer relaxes base
cold/RSS/index limits.

A naive first implementation put uncompressed task facts into redb and doubled the DB to
about 67.4 MB; it was rejected. An eager compressed version still made base cold RSS about
196 MiB; it was also rejected. The final sidecar is lazy and keeps base storage/memory unchanged.

The sidecar is not a semantic graph cache: it stores raw AST call facts only. Candidate/import
resolution and BFS remain query-local, so additions of same-name targets still invalidate
resolution via the normal index refresh. Facts are keyed by file content hash, extractor format
and generation. Query evidence files are reopened beneath the root and hash-checked.

## Final gates

- 443 Rust tests passed, 0 failed/ignored; fmt and locked all-target Clippy `-D warnings` passed.
- 42 Pi tests and TypeScript typecheck passed; local 0.8.0 manifest/native verification passed.
- `cargo package --locked --allow-dirty` and `git diff --check` passed.
- 31/31 directed mutants were killed by their intended assertions, including missing/stale
  task-cache acceptance, source-identity bypass, reverse/evidence-state errors, old-side loss
  and context evidence/ranking mutations.
- The fixed 3,908-file acceptance corpus remained inside basic cold/warm/RSS/index and
  incremental refresh budgets. Task sidecar cost is opt-in and separately measured above.

Evaluation binary SHA-256: `82d82ddd84ab3c2c5808e8a591b93dc19b3a729dac7b1c1b430de1c530dece2b`.
The independently rebuilt manifest-verified native binary differs by build path, but ran the
same final source and all Pi/native equivalence gates.

## Correctness / honesty rules retained

- Metadata fast paths do not claim complete candidate-universe verification; use verified mode.
- Ambiguous/receiver/macro/computed calls are not promoted to definite impact edges.
- Compact output never changes discovered sets or traversal budgets; it only omits disclosed
  repeated detail. Full output fixtures compare the same identities/witnesses.
- Changes still uses immutable Git blobs/raw captured worktree bytes and separate old/new impact.
- Context body-only/comment/string tasks still classify source text rather than treating it as calls.

Raw final optimized evidence is in `/tmp/cx-impact.7d8n3Y/combined-lazy-final/`,
`optimization-lazy-score-final.json`, `performance-ultimate/`, and `final3-mutations/`.
Earlier `combined*` directories remain diagnostic history.
