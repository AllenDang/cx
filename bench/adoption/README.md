# Natural tool-adoption pilot

Measures tool choice during real edits, not whether a model can obey “use cx”.

First-run findings and acceptance-test amendment: [RESULTS.md](RESULTS.md).
The original long-description candidate was not adopted. The continued study
found and integrated a different, explicit tool-side navigation policy:
[final confirmation](round4/RESULTS.md). The design below describes round 1.

## Frozen design

- Repository: [tkem/cachetools](https://github.com/tkem/cachetools), MIT,
  `v5.5.2`, commit `c403f9f4185e58090b904c1915345b9ba46d5a08`.
- Models: exact configured provider/model IDs in `cases.json`. No fallback models.
- Three Chinese edit prompts: negative size validation, decorator invalidation,
  non-touching LRU lookup. Each has a host-owned behavioral check in `acceptance.py`.
- 3 models × 3 tasks × 2 descriptions = **18 fresh sessions**, one repetition per cell.
- Six waves of three models, with AB/BA order alternating by task/model index.
  This counterbalances order partially; it is not randomized or perfectly balanced.
- Every run gets an independent clone of the same commit, pre-warmed index,
  medium reasoning, the same seven standard tools and eleven cx tools. The native
  supervisor tool is also available. No inherited conversation, project/global
  instruction files, skills, or unrelated extensions.
- **Only `description` changes.** Tool names/order, schemas, implementations,
  `promptSnippet`, and `promptGuidelines` are held constant. `baseline.json`
  captures metadata from cx commit `9a497c8b7ee952ab1e0e7827fb55ecd569c0d7c9`;
  `candidate.json` is frozen before launching.
- No task prompt names cx or asks for any particular navigation tool. The normal
  Pi system prompt still includes the existing extension-supplied guidance in
  both arms. This isolates description changes, not absence of all cx guidance.
- `pi-subagents` supplies the same Task/output-binding wrapper to each prompt;
  output paths vary by run. `checkExposure` accepts only this exact wrapper.
- Maximum ten minutes per child; no tool quota or forced tool choice. On a failed
  workflow child, stop at the wave boundary and inspect infrastructure. Never
  silently retry with another runner/model or count missing exposure as non-use.

## Metrics and evidence

`observer.ts` records tool execution events, usage, elapsed time, actual model,
active tool registry, and assembled system/task prompts outside the target tree.
It does not rewrite tool results or suggest which tool to call. Host warmups do
not enter model context or usage metrics. No provider headers/credentials or
reasoning text are collected by this observer; native subagent transcripts are
managed separately by Pi.

Primary descriptive metrics:

1. **Task adoption:** valid trials with at least one cx call / valid trials.
2. **Navigation share:** cx calls / navigation calls (read, grep, find, ls, cx,
   plus shell commands containing a common search/read executable).
3. First navigation tool and first cx call position.
4. All-tool share, per-tool counts, cx errors/empty responses, elapsed time,
   reported input/output/cache tokens and cost.
5. Host-run original regression suite plus frozen behavioral tests; a model's
   claim of success is not the quality gate.

The shell navigation classifier is explicitly heuristic (e.g. a combined
search+test command is one navigation call). All-tool counts are retained as a
non-heuristic denominator. Maintenance calls count as cx calls but remain
visible per-tool; inspect whether growth is useful rather than repetitive.
Invalid/missing/failed-provider trials remain visible and are excluded from
adoption denominators, **not converted to zero**. Paired comparisons also exclude
the corresponding healthy partner of an invalid trial; `summary.json` records
eligible `pairedIds` separately from all observed rows. Analyze only after the
workflow is terminal: an `agent_end` event alone is not process-settlement proof.
Inspect native workflow status alongside the summary. Track untracked test files
in the retained clones; `tracked.patch` is not a complete backup of those files.

## Reproduce

Prerequisites: this package's Node dependencies and bundled native assets;
Python 3 with pytest; Pi with pi-subagents; all three exact models configured and
authenticated. The preparation script makes no LLM calls. Do not overwrite an
old experiment directory.

```sh
ROOT=/tmp/cx-adoption-new
mkdir -p "$ROOT"
git clone --depth 1 --branch v5.5.2 https://github.com/tkem/cachetools.git "$ROOT/upstream"
(cd "$ROOT/upstream" && PYTHONPATH=src python3 -m pytest -q)
node --import tsx bench/adoption/prepare.ts "$ROOT"
node --test bench/adoption/*.test.mjs
npx tsc -p bench/adoption/tsconfig.json
```

Verify each `acceptance.py` case fails against upstream for the expected new
behavior before testing models. The original upstream suite should pass.
Preparation emits `manifest.json`, description snapshots, `extension.ts`, a
project-local `edit-trial` agent, and `run.js`. Every source clone is clean at
creation and HEAD is checked against the pinned commit.

In Pi, inspect `subagent` capabilities/models with `cwd` set to ROOT, validate
ROOT/run.js, then launch **one** async workflow:

```js
subagent({
  workflowScriptPath: "/tmp/cx-adoption-new/run.js",
  cwd: "/tmp/cx-adoption-new",
  async: true,
  context: "fresh",
  globalConcurrencyLimit: 3,
  maxSubagentSpawnsPerRun: 18,
  timeoutMs: 4200000,
  artifacts: true
})
```

Consume native completion notifications; do not run a polling loop. Once all
children are terminal, verify exposure and execute the host gates:

```sh
node bench/adoption/analyze.mjs "$ROOT"
```

The report is `ROOT/summary.json`; raw artifacts are `ROOT/evidence/rNN/` and
native Pi workflow output references. Keep these local: exposure/system prompt
artifacts contain machine paths. Keep the manifest, snapshots and all original
results, including failures. Do not edit tool runtime files during a study.

## Limits

This is a small, single-library Python pilot, not a statistically powered result
for all editing sessions. cachetools has few source files, so whole-file reads
may be reasonable. No broad unknown-repository tasks, negative-control docs-only
edits, language diversity or repeated sampling are included. Pre-warming removes
cold-start cost. Tool inventory is controlled and less cluttered than a normal
Pi setup. Provider defaults differ despite the same requested reasoning level.
Model service reproducibility is not guaranteed. A higher call count without
maintained correctness or with more errors is not an improvement. Before claiming
a general adoption gain, repeat on held-out tasks/languages with multiple trials
and paired uncertainty estimates; do not tune and declare victory on the same
small task set.
