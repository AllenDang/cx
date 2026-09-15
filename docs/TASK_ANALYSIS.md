# impact / changes / context — cx 0.8.0 development build

These are implemented commands, not a roadmap. Build with `cargo build --release`.
The local Pi bundle contains the same commands; nothing has been published or installed
into another Pi checkout automatically.

## Quick examples

```sh
# Unknown implementation: retrieve lexical evidence and bounded original code.
cx --json context --query 'reader suffix' --include-body --limit 5

# Known function: trace reverse calls, up to three hops.
cx --json impact --name reader_suffix_ok \
  --file src/engine/texture_ops/tex_pass_plan.cpp --max-depth 3

# Tracked working bytes vs HEAD, with independent old/new impact.
cx --json changes --impact --max-depth 2

# Git index vs HEAD; or an explicit merge-base comparison of commits.
cx --json changes --staged
cx --json changes --base main --head HEAD --merge-base
```

Use `target/release/cx` instead of `cx` if this development binary is not installed.
All commands support `--root`, `--json`, `--limit`, `--offset`, `--no-tests`, and a
`--byte-budget`. `--all` removes the row limit, not the explicit byte budget.

Task commands default to `--detail compact`: identities, primary witness/provenance,
completeness and uncertainty remain, while repeated raw hunks/hashes/signatures/path variants
are listed in `analysis.detail_omitted`. Use `--detail full` for audit/debug output.

Pi tools: **cx_impact**, **cx_changes**, **cx_context**. They are registered by
`extensions/pi-cx/index.ts`; the local manifest-verified bundle is under
`vendor/pi-cx/<platform>`. Load this workspace's extension to use them. An older
installed 0.7.x extension does not acquire new tools merely because Rust was built here.
For local testing: `pi -e ./extensions/pi-cx/index.ts` (this starts Pi; it is not run
implicitly by a cx query).

## impact

Select one root by lexical `--name`, optionally qualified-name `--scope` and exact
`--file`. `--line` selects its starting line; `--byte-offset` selects its exact start
byte. Both site selectors require file and are mutually exclusive. Ambiguous roots
return candidates and `subject_ambiguous`, never a union.

Defaults: depth 3, max nodes 1,000 including the seed, max edges 20,000. Depth is
0..32, nodes 1..10,000, edges 1..1,000,000. `max-edges` bounds matching call sites
resolved when expanding incoming name groups. It is not an output limit.

Compact results carry exact identity/depth, `evidence`, and one primary witness. Full detail
separately returns supported and possible records. Every witness edge preserves resolution and
call location. Name-only unique targets and unique targets under relevant parse degradation are
possible evidence; receiver/macro/ambiguous targets remain an unexpanded frontier. A shorter
possible path does not erase a supported path.

The root is separate, recursion is deduplicated by site/evidence state, and anonymous
or file-scope callers are not relabelled as outer functions. Bindings to those synthetic
containers are not invented. `--no-tests` filters displayed results, not the candidate
universe; witnesses can still expose the evidence needed to understand a path.

Call sites, parse degradation and line starts are built lazily in one content/AST pass on the first
task query, then cached by content hash/extractor version. Impact resolves them by visited name,
reopening and hash-validating only relevant evidence files. Basic indexing/queries do not build or
load the cache. Candidate/import resolution and BFS remain query-local: the cache is not a persisted
semantic graph, daemon, compiler graph or runtime proof. `INDEX_VERSION=16`; task facts use a
generation/hash-bound zstd sidecar invalidated by refresh and removed by `cx cache clean`.

## changes

Default comparison is **raw tracked working-tree bytes vs HEAD**, including staged and
unstaged content together, excluding untracked files. `--staged` compares the Git index
to `--base` (HEAD by default). `--head REF` compares two commits; `--merge-base` requires
head and resolves the common ancestor explicitly. Invalid/noncommit refs, unborn HEAD,
conflicts and Git failures are errors, not clean results.

Git runs through argv, with OID validation and NUL path records. Queries do not run an
external diff, textconv, clean/smudge filter, fsmonitor hook or project test command, and
do not checkout/reset the user's branch. Working files are captured sequentially; no
atomic worktree snapshot or `.gitattributes` normalization is promised.

Rows preserve before/after file modes and hashes, changed line hunks, old/new symbol sites,
and file-level changes. Matching by unique name/scope/kind across versions is explicitly
heuristic. Unchanged neighbors are removed by source identity, not just hunk overlap.
Unique identical-content file moves are paired; other moves remain additions/deletions.
Binary, symlink, submodule, mode-only and oversized/unmodelled files are retained as
classified file changes instead of silently dropped.

`--impact` composes the same graph engine over separate before/after source views. Deleted
functions retain before impact; absent sides are `not_applicable`. Non-function and
unsupported-language impact is disclosed. Before and after counts are never summed as
one exact affected set. This option costs more than changes-only, especially on large repos.

Bounds: source bodies above 8 MiB are not symbol-mapped; retained source snapshots are
bounded at 512 MiB. Line diff work is bounded and falls back to an explicitly coarse hunk.
Submodule working-directory dirtiness is not inspected; its presence makes a working-tree
report partial. A subdirectory root excludes parent source contents; commit/index changes
outside it have metadata counts, while outside working-tree changes remain unknown.

## context

Searches exact names/qualified names/paths, camel/snake subwords, signatures, and source
text. Hits disclose metadata, code, comment, string or generic/unparsed text provenance.
They do not become call evidence. No match permits an empty result; there is no popularity
filler, embedding service, inferred test runner or cross-language semantic-search claim.

Vendor, generated and fixture paths are excluded by default; opt in explicitly with
`--include-vendor`, `--include-generated`, `--include-fixtures`. Tests remain searchable
unless `--no-tests` is set. Exact-name priority, filtering and deterministic ties are tested.
Exact name/path/complete subword matches use indexed metadata and read/hash only selected evidence
files. Pi defaults context/impact to verified freshness. If CLI metadata freshness is used, this
fast route explicitly marks the unseen candidate universe partial rather than claiming completeness.

`--include-body` adds original text, at most 40 lines/2,048 bytes per excerpt, with full and
shown ranges plus truncation flags. Overlapping bodies are not duplicated. Results carry
source identities and definition/overview follow-ups. Default byte budget is 16 KiB;
impact/changes default to 32 KiB. Pi limits byteBudget to 32 KiB to stay inside its transport cap.
Compact task JSON is emitted without pretty-print whitespace. Full match excerpts/signatures remain
available through `--detail full`; compact output discloses omitted classes.

## Shared output and errors

JSON preserves the eight envelope fields and adds `analysis`. For these three commands,
`page.total` is null when analysis is incomplete; `analysis.discovered_count` is the exact
materialized-row count. `page.truncated` says another **output page** exists. Increasing
offset cannot resume an exhausted graph traversal.

Returned page commands carry a `--snapshot` guard. If inputs change, restart pagination
rather than merging answers from two source versions. Depths in partial analysis are
witnessed upper bounds, not proof that a missing shorter route cannot exist.

A byte budget includes the envelope and required metadata. Rows/paths/identities are not
silently shortened. If one indivisible report cannot fit, `budget_too_small` returns a
bounded error and required-size information. Source excerpts and signature previews have
explicit elision metadata. Pi saves transport-overflow output privately and marks all
unavailable totals/analysis unknown instead of suggesting a fictitious traversal page.

Exit 0 means a query result, possibly partial; exit 1 is a structured failure; exit 2 is
invalid CLI input. None means “safe modification” or “no tests needed”. Missing grammars
are not downloaded implicitly by queries; `cx lang add` is the explicit installation path.

## Validation / actual usefulness

See `bench/task_analysis/OPTIMIZATION_REPORT.md`. On the frozen combined pilot the optimized tools
kept 4/4 evidence sufficiency while beating the strong mechanical baseline in calls, request+stdout
bytes, cl100k communication tokens and wall time. This is a small developer pilot, not universal
statistical or end-to-end agent proof.
