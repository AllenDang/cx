# impact / changes / context — combined implementation and usefulness report

> Historical pre-optimization cost report. The same frozen tasks were rerun after
> content-bound call caching and compact output; [the optimization report](OPTIMIZATION_REPORT.md)
> now shows 4/4 with fewer calls, bytes, cl100k tokens and wall time than the strong baseline.

## Verdict

**Engineering validation: PASS for the implemented scope.** `impact`, `changes` and
`context` are present in the 0.8.0 development CLI and typed Pi extension. This is not
a published release or a universal task-benefit claim.

**Initial combined developer pilot:** all groups retrieved sufficient evidence for 4/4 frozen
scenarios. Before optimization, new tools reduced calls but increased bytes/wall. That historical
result and raw evidence remain below. It motivated caching and compact output rather than weakening
correctness. The final same-task result is reported separately in `OPTIMIZATION_REPORT.md`.

This is a mechanism-oriented developer pilot, not a 72-task held-out adoption result. No
agent-in-the-loop/model answer trial was run; bytes are not tokens.

## Delivered behavior

### impact

- Exact root selection by file/scope/site; ambiguity is a structured failure.
- Query-local immutable source snapshot and lazy parsed-name groups.
- Reverse BFS over exact site + evidence state, including mutual recursion and stable
  deterministic witnesses.
- Separate supported/possible shortest paths; ambiguous/receiver/macro edges stay frontier
  facts and are never traversed as definite paths.
- Independent max-depth/max-nodes/max-edges stops; unknown totals are null and cannot be
  confused with output pagination.
- Snapshot-bound follow-up pages and byte-budget errors that preserve identities.

### changes

- Tracked raw working bytes vs HEAD; explicit staged, commit and merge-base modes.
- Commit OID validation, argv/NUL protocols, one bounded cat-file process, no shell,
  checkout, external diff/textconv/filter/fsmonitor or project command execution.
- Old/new hunks and symbols; deleted functions retain old sites and before impact.
- Unique identical-content move pairing; binary/symlink/submodule/mode-only/file-level
  changes remain classified rows.
- Optional impact is independently built for old and new snapshots. No current-only symbol
  substitution, no test execution, no safety conclusion.

### context

- Deterministic exact name/qualified/path, camel/snake subword, signature and source-text
  retrieval. Zero evidence stays empty.
- Comment/string/code/generic source provenance, not call evidence or semantic confidence.
- Default vendor/generated/fixture filtering; tests are explicit and not silently hidden.
- Original UTF-8 body snippets with ranges/elision; overlapping bodies deduplicated.
- Byte-aware pages, source/snapshot identity and definition/overview follow-ups.
- No embeddings, popularity filler, graph centrality or guessed test runners.

Shared detailed contract: `docs/TASK_ANALYSIS.md`.

## Correctness evidence

Final combined suite after documentation/staging changes:

- **438 Rust tests passed / 0 failed / 0 ignored**, including 13 changes, 9 context and 12 impact integration tests.
- `cargo fmt`, locked all-target Clippy with `-D warnings`, bins/tests/all-targets passed.
- TypeScript typecheck passed; **41 Pi tests passed**. The Pi suite uses a local manifest containing the freshly built 0.8.0 binary and pinned grammars; `verify:pi` and native three-tool CLI equivalence passed. An earlier pre-staging run correctly rejected the old 0.7.10 vendor manifest; it was not counted as a product failure or hidden by PATH fallback.
- **31 mutation controls killed** by intended assertions (existing foundation plus reverse
  direction, node identity, ambiguity propagation, possible-path promotion, visited/evidence
  states, depth disclosure, content identity, old-side symbols, bad refs, exact-name ranking,
  zero-evidence filler and comment provenance). Disposable target caches are cleaned between
  mutants so stale builds cannot count as kills.
- Local Pi tests cover schema/argv/path bounds, partial totals, dirty refresh, pre-abort,
  cancellation/timeout, transport overflow and a manifest-SHA-verified current binary whose
  impact/changes/context outputs equal direct CLI outputs.

Security/path controls include malicious refs, names with spaces/newlines/leading dashes,
repository filters and fsmonitor commands, symlink targets, mode changes, non-Git/unborn HEAD,
conflicts/ref failures, root escapes and dirty-path proof. Query parsing is offline: missing
parsers are errors; only explicit `lang add` may install one.

## Frozen combined ANGE pilot

Input checkout: ANGE `24ceb20f158fe6a54eb5a1f6c132e14e3e533532`, clean disposable
shared clone/materialized archive. Tasks/gold/policy and source hashes were frozen before
candidate runs. Two navigation tasks and one real utility-extraction commit in both directions:

1. Task words `reader suffix` → locate `reader_suffix_ok` → exact 2-hop production callers.
2. Task words `trailing count` → locate `tex_pass_plan_trailing_count` → exact 3-hop production callers.
3. Commit `1ca81651…` → `c9491922…`: eight util files added; identify three functions in
   `shader_variant.cpp`.
4. Reverse comparison: same files/functions retained as deleted old-side evidence.

A = old tool relation-first with identity checks and bounded syntax fallback. B = stronger
mechanical composition caching known identities/lookups. C = context+impact or changes.
Three repetitions rotate group order. All pages, fallbacks and failed work count. Each
workflow is bounded to 30 operations, 256 KiB request+stdout and 60 seconds.

### Final result

| Group | Sufficient | Operations / 4 tasks | Request + stdout | Total wall median |
| --- | ---: | ---: | ---: | ---: |
| A old workflow | 4/4 | 23 | 46,438 B | 4.426 s |
| B mechanical strong baseline | 4/4 | 21 | **44,411 B** | **4.140 s** |
| C new tools | 4/4 | **6** | 128,104 B | 7.740 s |

C lowered calls by 71% versus B (21 → 6), but communication was 2.88× and wall 1.87×.
No agent/tool-schema/model-token accounting was run, so the 71% is only a tool-call count.

Per C task:

| Task | Calls | Request + stdout | Wall median |
| --- | ---: | ---: | ---: |
| NAV reader suffix | 2 | 33,186 B | 2.872 s |
| NAV trailing count | 2 | 38,658 B | 4.294 s |
| Utility addition | 1 | 28,094 B | 0.287 s |
| Utility rollback | 1 | 28,166 B | 0.285 s |

The first C run was also preserved. A general internal optimization (cached line starts plus
name-indexed binding blockers, with no task/rank/gold change) reduced the harder impact task
median from 4.845 to 4.294 s; it did not change output size, task answers or the overall
conclusion. Context's full-corpus lexical scan dominates discovery here; a future persistent
inverted index would need its own size/refresh measurement before adoption.

For navigation, all groups found exactly the frozen root/reach/depth sets. New paths retain
supported/possible labels; the old fallback path is counted as possible. For Git cases, A/B
read exact name-status plus the relevant original patch; C found the identical eight files
and three added/deleted functions in one structured query, retaining old/new site facts.
No additional unadjudicated candidate was automatically credited.

## Costs and regressions

The existing fixed old ANGE performance corpus was measured during the immediately preceding
foundation fix and all absolute budgets passed; no foundation storage expansion was introduced
by these commands. New commands are opt-in and do not make overview/definition/context-free
queries build a relation/lexical graph. The combined pilot gives command-local costs above.

Known trade-offs/residual risks:

- Context scans/hash-checks indexed files per invocation; it is not yet efficient enough to be
  a default route for known symbols.
- Impact's supported edges are stronger static evidence, not compiler/runtime proof; possible
  paths can include false positives. Dynamic/interface/template instantiation remains frontier.
- A parse-degraded matching language makes name-only unique targets possible. An unrelated file
  with no queried lexical name does not poison the path.
- Changes raw worktree capture is sequential, not atomic. It detects index races and snapshots
  every tracked file state it reports, but cannot freeze concurrent filesystem writers. Use
  commit/staged mode for immutable Git-side comparisons.
- Only unique identical-content moves are paired; modified moves remain add/delete.
- UTF-8 paths are required; non-UTF8 Git paths fail rather than merge lossily.
- Submodule working-tree dirtiness is not inspected and makes the report partial.
- Context's lexical scoring is engineering-deterministic, not language-semantic or multilingual
  meaning retrieval. No relevance confidence scale is emitted.
- Pi task byteBudget is capped at 32 KiB; transport overflow preserves a secure full-output path
  and explicitly sets totals/analysis unknown rather than inventing traversal continuation.

## Reproduction / evidence

Main raw evidence directory for this run:

```text
/tmp/cx-impact.7d8n3Y/
  red-impact.*
  impact-local.* / changes-second.* / context-second.* / edge-controls.*
  gates.json / gate-*.stdout / gate-*.stderr
  mutations-verified/report.json / <mutation>.*
  combined-{tasks,gold,policy,lock}.json|txt
  combined/runs.json
  combined-final/runs.json
  combined-score.json
  optimized-candidate.json
  release*.stdout / release*.stderr
  verify-pi.*
  candidate
```

The report does not authorize automatic commit, publication, or runtime project test execution.
