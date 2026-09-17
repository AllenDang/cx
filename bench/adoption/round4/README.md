# Round 4 confirmation — explicit source-entry policy

Status: running. This is an explicitly instructed navigation workflow, **not**
weight training or evidence of spontaneous tool preference.

## Why another round

Round 3's tail policy passed both development cases but failed its full gate:
strict automated source adoption 1/3, code/document quality 5/6. The Kimi failure
was the cache=None + lock boundary. The original failed verdict is retained.
Inspection also found that the shell heuristic incorrectly treated
`cat source.py 2>/dev/null` as a write; one DeepSeek trial had two successful
source lookups before its actual edits. That trial also downloaded another
release into scratch space. Neither issue is silently converted into a prior
round success. All shell/scope evidence requires review.

## Frozen confirmation design

- Three fresh DeepSeek baseline tasks and the same three candidate tasks:
  discard, reset-stats, receiver-count (cachetools + blinker).
- One DeepSeek documentation-only negative control.
- One GPT-6 and one Kimi compatibility task using the simple negative-size case.
  These are narrower smoke checks, not a claim that the prior Kimi reset-stats
  defect was fixed. No previous failed case is removed from its original report.
- Nine model editing/document trials plus two deterministic evaluation relays.
- Both arms use the same repaired runtime: unsupported dirty file types no longer
  poison later queries. That repair passed all 47 extension tests before launch.
- Candidate metadata remains the round-2 priority metadata package. The new
  English system-tail policy is in `policy.json`; user edit prompts do not name cx.
- Both reset-stats prompts add the identical clarification that a nonempty lock
  must be acquired even when cache=None. This spells out the existing test
  contract; no acceptance assertions changed. It means these are not identical
  prompts to the earlier rounds, which remain separately reported.
- Candidate success requires >=2/3 DeepSeek source tasks with successful nonempty
  cx source retrieval before actual editing, all six candidate code/document
  gates passing, no cx calls in the document-only control, and parent review of
  shell mutation/scope evidence. No ordinary tool is disabled.

## Measurement correction

The new inspector uses completion timestamps of actual nonempty lookup results
against direct in-workspace edit/write calls. It does not count output-artifact
writes as source edits. Potential shell writes are flagged for manual review,
rather than silently treating stderr redirection as a mutation. Preceding shell
commands and scope must still be reviewed; a direct-edit timestamp alone is not
proof that a shell command did not change source earlier. Earlier result JSON
files are not rewritten by this new inspector.

The actual provider payload still gets a bounded, sanitized audit: tool names,
description/guideline presence, task presence, and system-tail presence. No raw
request content or credentials are saved by that audit.

## Execution

`prepare.mjs` consumes the locally retained round-2/3 manifests and pinned clean
upstream checkouts, creates fresh clones, and writes the exact runtime and policy
hashes. It requires those prerequisites; it is not a standalone download script.
Use `node --import tsx bench/adoption/round4/prepare.mjs <new-root>`, then validate
and run the generated native Pi workflow with concurrency 3 and an 11-child cap.
The initial run is `/tmp/cx-adoption-round4`, workflow
`8d2e08e9-dfb9-4bc9-897b-8a783bdb6d7a`.

Offline tests: `node --test bench/adoption/round4/study.test.mjs`. The evaluator
reuses unchanged round-2 behavior checks and round-3 documentation checks.
