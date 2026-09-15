# Task analysis interfaces — implementation contract

Implement impact, changes and context before the combined full-suite/effectiveness run.
Feature-local tests still precede implementations. Existing one-hop tools stay available.
No commit, publication, project test execution, daemon or model/network service is implicit.

## Shared contract

New commands preserve the eight JSON envelope fields and add `analysis` (structured).
For these commands only, `page.total` is null when coverage/traversal is incomplete;
`analysis.discovered_count` is the exact number of materialized matching rows.
`page.truncated` means another output page exists, NOT that graph traversal can resume.
A supplied `--snapshot` must match the computed input identity; mismatch is an error.
Finite pages respect a serialized byte budget, preserve identity/uncertainty, and carry
runnable next pages. An indivisible identity/metadata block fails explicitly if it cannot fit.
Exit 0 is a query result (possibly partial), 1 is a structured failure, 2 is CLI misuse.

All source-backed symbols, matches and calls use immutable bytes checked against their
indexed facts (or immutable Git blobs). Metadata freshness is not an atomic worktree claim.
Unknown receiver/macro/computed calls and ambiguous targets are never traversed as definite
edges. Tests may appear as ordinary affected/search results; no test-runner recommendation
or runtime coverage is claimed.

## impact

`impact --name NAME [--scope GLOB] [--file FILE] [--line N|--byte-offset N]
 [--max-depth 3] [--max-nodes 1000] [--max-edges 20000] [--snapshot ID]`

Root is one function site; declarations linked by existing evidence can select its definition.
Otherwise definition sites are preferred; ambiguous roots return selectable candidates,
not a union. Byte offset selects a site's exact start; line selects its starting line.
Limits: depth 0..32, nodes 1..10000 (seed included), edges 1..1000000. max-edges bounds
matching call sites resolved while expanding the reverse graph, not output row count.

A query-local graph caches source bytes, parsed files and resolved incoming name groups.
No reparse per BFS hop. Stable reverse BFS runs over (site, evidence class) states so a
short possible route cannot suppress a supported route. Each affected site is one row with
minimum depth plus separate supported/possible witness records when available. Every path
retains each edge's location and resolution basis. Seed is separate and never counted in reach.
Syntax-only unique associations, and unique associations under relevant parse degradation,
are possible evidence, not compiler proof. Unresolved frontiers are bounded, counted and not
expanded. Depth/node/edge stops are separate from output pagination; partial depths are
witnessed upper bounds, not an assertion that omitted paths cannot be shorter.

## changes

Default: tracked working bytes vs HEAD, excluding untracked. Explicit staged and two-commit
comparisons are separate; merge-base requires a commit comparison. Paths are Git NUL-delimited,
refs must resolve to commit OIDs, no checkout or shell interpolation, no external diff/textconv
or repository filters. Read old/new bytes, map changed line ranges to their respective symbols,
keep file-level and nonregular changes, and label heuristic version matches. Optional impact
reuses the graph engine with separate before/after snapshots, never current symbols for old code.

## context

`context --query TEXT [--include-body] [--byte-budget 16384]` plus paging and explicit
vendor/generated/test filters. Rank exact names/qualified names/paths ahead of subwords and
body/comment/string lexical evidence. Zero lexical evidence permits an empty result. Matches
carry actual source locations/fields; code excerpts remain original bytes with elision metadata.
No embeddings, popularity filler or default graph expansion. Related analysis stays available
through impact/definition follow-ups; no unsupported semantic or cross-language search claim.

## Validation sequence

1. Local semantic fixtures, bounds and compile checks while implementing each mechanism.
2. CLI/Pi schemas, argv, errors, dirty refresh, cancellation and the freshly built binary.
3. One combined Rust/TS/package/mutation run, then fixed-corpus A/B/C task workflows including
   fallback and all pages. Report correctness separately from usefulness and cost.
