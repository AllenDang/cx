# DeepSeek adoption iteration (round 2)

This study follows the negative cross-model result in [round 1](../RESULTS.md).
It changes **tool descriptions, prompt snippets and tool-owned guidelines**, not
model weights or edit-task instructions. It does not disable standard tools or
rewrite their results. An explicit priority policy is labeled separately from
recommendation-driven adoption.

## Preregistered sequence

1. Run DeepSeek on three new tasks using frozen round-1 production metadata.
2. Try `compact`: concise English descriptions and recommended navigation routes.
3. If needed try `bilingual`: the same descriptions, with Chinese/English snippets
   and Chinese recommended routes for the three source-entry tools.
4. Only if needed try `priority`: an explicit source-navigation policy. This is
   **instruction-following**, not evidence of spontaneous natural adoption.

Each candidate first receives two development tasks (negative size and peek)
from round 1, in independent fresh sessions. Both must pass code checks and use
at least one successful, nonempty `cx_symbols`, `cx_definition` or `cx_context`
lookup before the first observed edit. Overview-only or post-edit calls do not
satisfy this criterion. Shell mutation detection is heuristic; inspect winning
transcripts manually before accepting the result.

Candidates passing development receive three new DeepSeek tasks plus one GPT-6
and one Kimi compatibility task. At least **2/3 DeepSeek new tasks** must meet the
source-retrieval criterion, and **all five code results** must pass host checks.
Stop at the first passing candidate; do not run stronger policies unnecessarily.
All three variants and tasks are frozen before launch. Because several variants
may be evaluated on the new tasks, this remains an exploratory sequential study,
not an untouched statistical test set or proof of a universal effect.

Maximum: 24 pre-created editing clones and seven judgment relays (31 children in
the longest path; the workflow launch budget must allow 31). Usually far fewer
run because unsuccessful development arms skip validation and success stops the
workflow. No per-tool usage quota is imposed on editing agents.

## Repositories and tasks

- cachetools v5.5.2: `c403f9f4185e58090b904c1915345b9ba46d5a08`.
  Development: negative-size, peek. New tasks: discard, reset-stats.
- blinker 1.9.0: `669f3a027828d19786e708b511277fabcd6b9532`.
  New task: receiver-count, including weak-reference lifecycle.
- All source starts clean at the specified commit. Each trial has its own clone
  and pre-warmed index. Tools, source runtime and reasoning level are held fixed.
- `heldout.json` contains the task prompts, none of which mentions cx. The edit
  agent instruction and basic tool inventory are unchanged from round 1.

## Evaluation and automation

`observer.ts` records actual tool events and exposed metadata outside the target
workspace. `judge.mjs` checks registry/prompt metadata, executes the sample's full
pytest suite, the **frozen upstream tests**, and host-owned behavior checks. It
saves raw results, status and tracked patches. Provider errors, missing exposure,
wrong models or metadata drift are infrastructure issues, not non-adoption.

A narrow read-only `trial-judge` child runs the exact deterministic evaluator
command and relays its JSON through structured output. It neither edits trials
nor chooses thresholds. This is necessary because raw pi-subagents workflows
cannot read host files or run arbitrary host commands directly. Its output and
artifact reference remain auditable; the parent must independently check the
saved JSON and actual events before applying a winning preset. Judgment relay
calls are **not** part of editing-model usage statistics.

One async native subagent workflow owns all children. Workflow/launch/provider
infrastructure failures stop the study; no runner or model fallback is automatic.
Ordinary failed code tests or non-adoption can advance to the next preset, and
remain visible in reports. A null winner means no verified improvement, not
permission to declare success or keep sampling unchanged prompts until lucky.

## Environment and preparation

Round-2's virtualenv inherits installed Python packages and adds pytest-asyncio
1.4.0. Python is 3.14; pytest is 9.1.1. Blinker requires the asyncio pytest plugin;
without it its `asyncio_mode` setting fails validation. Baseline upstream suites:
cachetools 216 tests; blinker 25 tests. No global Python installation is changed.

On this machine, `/usr/bin/git` routes through an Xcode license gate. The
already-installed Command Line Tools Git is used instead. All arms receive the
same explicit virtualenv/toolchain PATH prefix. No license or global config is
changed. `prepare.mjs` contains local default paths; pass a fresh study root and
cachetools upstream path, and provision its `venv`/`blinker-upstream` first.

```sh
ROOT=/tmp/cx-adoption-round2-new
mkdir -p "$ROOT"
/Library/Developer/CommandLineTools/usr/bin/git clone --depth 1 --branch 1.9.0 \
  https://github.com/pallets-eco/blinker.git "$ROOT/blinker-upstream"
python3 -m venv --system-site-packages "$ROOT/venv"
"$ROOT/venv/bin/python3" -m pip install pytest-asyncio==1.4.0
node --import tsx bench/adoption/round2/prepare.mjs "$ROOT" /path/to/cachetools-upstream
```

Before running models, validate the evaluator itself:

```sh
PYTHONPATH=/path/to/cachetools-upstream/src:$ROOT/blinker-upstream/src \
  "$ROOT/venv/bin/python3" bench/adoption/round2/acceptance_selftest.py
node --test bench/adoption/round2/study.test.mjs
```

The self-test rejects upstream missing implementations, accepts reference
behaviors without altering source checkouts, and rejects deliberate value-read
and cache-clearing bugs. It caught and fixed an observer retaining weak receivers
**before model trials were frozen/launched**. No post-hoc acceptance amendment is
planned; if one is required, preserve old evidence and label it explicitly.

Preparation writes the manifest/hashes, 24 isolated clones, a trial observer,
project-local edit/judgment agents and `run.js`. Discover capabilities and models,
validate the script, then launch via `subagent` with `cwd: ROOT`, `async: true`,
`globalConcurrencyLimit: 3`, `maxSubagentSpawnsPerRun: 31`, and a bounded deadline.
Consume native notifications rather than polling. Preserve all observed results,
including failed, skipped and baseline trials. Never edit the frozen experiment
files while the workflow is active.
