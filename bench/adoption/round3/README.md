# Round 3: examples and system-tail guidance

The separate read-only capability probe succeeded: DeepSeek called cx_symbols
and cx_definition and received real nonempty source results. Its actual provider
request contained all 11 cx tool schemas, descriptions and guidelines. That
explicitly prompted probe is **excluded from editing adoption statistics**.

Round 2's compact, bilingual and priority metadata arms all produced zero cx
calls. Round 3 therefore tests two **candidate packages**, not a clean factorial
attribution of a single sentence or prompt position:

1. `examples`: compact metadata + explicitly labeled illustrative navigation
   examples at the end of the system prompt.
2. `tail-priority`: priority metadata + an explicit Chinese source-navigation
   policy repeated at the end of the system prompt. If it succeeds, report it as
   system-guided compliance, not unprompted natural adoption or weight training.

Neither candidate modifies the edit request, fabricates tool-result history,
disables ordinary tools, or imposes a call-count quota. Both retain fallbacks and
exempt prose/configuration/command tasks. The source runtime is unchanged during
this experiment.

## Procedure

- Repeat the three DeepSeek new-task controls with baseline metadata and no tail.
- For each arm, require meaningful source retrieval and independent code checks
  on both development tasks before proceeding.
- Then run three DeepSeek source validation tasks, two GPT/Kimi compatibility
  tasks, and one DeepSeek **documentation-only negative control**.
- Require at least 2/3 meaningful DeepSeek source adoptions, all six code/document
  checks passing, and zero cx calls for the documentation-only control.
- Stop at the first passing package. Missing/provider/drift evidence stops the
  workflow; failed adoption/code checks remain recorded and can advance the arm.
- At most 19 edit/document trials + 5 deterministic judgment relays = 24 children.
  Each model task runs in a fresh isolated clone. Relays do not enter usage stats.

The new tasks already had baseline runs in round 2, and are reused across
predeclared packages if needed. Results are exploratory, not proof of statistical
significance or universal generalization. No post-hoc assertion changes or
relabeling failed cases as successful are permitted without explicit amendment.

## Request-level evidence

`observer.ts` registers its prompt-tail handler before the ordinary observer,
so `exposure.json` captures the chained prompt. The inherited payload probe
records only model/tool names and metadata-presence booleans, never headers,
credentials, or raw message bodies. The evaluator checks both the visible
system-prompt suffix and `request-1.json.tailPresent`, as well as transmitted
metadata. Later request snapshots are retained for independent review.

## Reproduction

`prepare.mjs <new-root> <round2-root>` reuses round 2's pinned upstream checkouts,
virtualenv/toolchain paths and exact task/metadata snapshots, but creates fresh
source clones. It refuses dirty or wrong-commit upstreams and an existing manifest.
The source checkout must have installed Node dependencies and bundled native
assets. The generated root contains explicit edit/relay profiles and `run.js`.
Use native pi-subagents, fresh context, async execution, concurrency 3, and a
24-child budget. Validate the script and agent capabilities before launching.

Local preparation used `/tmp/cx-adoption-round3`; workflow
`6108008e-ad97-47ca-86c2-fa29f5bd7c1d`. The separate probe was
`f04d2ca0-13db-49c6-8141-6a08c15cca6d` under the round-2 mission.

The parent also independently reproduced an unsupported-file dirty-refresh
availability bug, documented in [DIRTY_REFRESH_FINDING.md](DIRTY_REFRESH_FINDING.md).
New regression tests pin that bug, but its runtime fix is deliberately postponed
until these model runs settle to avoid changing the experiment midway.
