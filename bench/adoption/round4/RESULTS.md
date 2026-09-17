# Final confirmation: DeepSeek uses cx under explicit tool-side guidance

**Result: the required-entry package passed the confirmation gate and was ported
to the Pi extension source.** This is explicit system-guided tool routing, not
training model weights, spontaneous preference, or a guarantee for every future
request. The user edit prompts never named cx.

## What shipped in source

- Four entry-point descriptions/snippets/guidelines in `extensions/pi-cx/tools.ts`
  match the measured metadata exactly.
- `extensions/pi-cx/navigation.ts` appends the measured English policy to the
  system prompt while cx_symbols/cx_definition/cx_context are all active. It
  preserves existing instructions, avoids duplicate tail insertion, leaves user
  requests and available tools unchanged, and exempts document/configuration/
  command/test-only work. Empty/error/insufficient lookup falls back to ordinary
  tools.
- The independent dirty-refresh bug is fixed: `unsupported_file_type` is consumed
  as non-indexable, excluded from refreshed counts, and reported through
  `unsupportedPaths`. Security/read/unverified/unknown failures remain fatal.

The globally installed package is still pinned to `git:github.com/AllenDang/cx@v0.8.0`.
No user installation settings were changed and no release was published. A local
checkout must be explicitly loaded, or a later release installed, before these
source changes affect that installation. Reloading the old tag alone is not an
upgrade.

## Confirmation evidence

Native workflow: `8d2e08e9-dfb9-4bc9-897b-8a783bdb6d7a`.
Nine editing/document trials and two deterministic evaluation relays completed.
Parent independently reran both host evaluation commands after settlement.

| Model / condition | Source tasks | Effective pre-edit cx source retrieval | cx calls | Code gates |
|---|---:|---:|---:|---:|
| DeepSeek baseline | 3 | 0/3 | 1 (overview only) | 3/3 |
| DeepSeek required-entry | 3 | **3/3** | **10** | **3/3** |
| GPT-6 compatibility smoke | 1 | 1/1 | 2 | 1/1 |
| Kimi K3 compatibility smoke | 1 | 1/1 | 3 | 1/1 |
| DeepSeek documentation-only control | — | not applicable | **0** | 1/1 |

Exact models: `aliyun/deepseek-v4.1-flash`,
`amazon-bedrock/global.openai.gpt-6-astra`, `aliyun/kimi-k3`.
All requested medium reasoning; actual request metadata was inspected.

DeepSeek tasks and useful calls:

- `s04`, cachetools discard: nonempty `cx_definition(Cache, from=...)` before edits.
- `s05`, cachetools reset-stats: nonempty `cx_symbols(cached)` and
  `cx_definition(cached)` before edits.
- `s06`, blinker receiver-count: nonempty `cx_symbols(*receiver*)` before edits.

Overview calls alone did not qualify. All qualifying results completed before
in-workspace source editing began. The parent reviewed the preceding shell calls:
no candidate had an earlier shell mutation of project source. The rule did not
produce perfect adherence to every recommended step/order; the measured outcome
is effective pre-edit retrieval, not universal first-tool compliance.

Independent code gates included the frozen upstream suite, the modified
workspace suite including added tests, and host behavior checks. All nine final
trials passed. The documentation control changed only the requested README
paragraph and made no cx calls.

## Cost and limits

For the three DeepSeek source tasks:

| Metric | Baseline | Candidate |
|---|---:|---:|
| All tool calls | 69 | 84 |
| cx share of navigation calls | 1/47 = 2.1% | 10/61 = 16.4% |
| Mean elapsed time | 164.2s | 176.6s |
| Cumulative reported tokens | 1,160,508 | 1,472,889 |
| Provider-reported estimated cost | $0.0996 | $0.1103 |

Elapsed time increased about 7.5%, cumulative reported tokens about 26.9%.
Tokens include repeated input/cache accounting; costs are provider estimates,
not invoices. Model-generated tests and mutation checks also differ. This is an
adoption improvement in a small exploratory study, **not an efficiency win** or
proof that prompt text alone caused every difference.

The test set is only two Python libraries, with three paired source tasks and
one repetition per cell. Several candidate packages were tried sequentially,
and the tasks were reused. No statistical significance or broad-language
universality is claimed. GPT/Kimi compatibility checks here are deliberately
narrow negative-size smoke tasks, not proof that all their earlier failures were
fixed.

## Scope audit and retained negative evidence

Workspace isolation was implemented with independent clones and prompt
boundaries, not an OS filesystem sandbox. Manual review found:

- Baseline `s01` downloaded another cachetools release to `/tmp/ctdl` and read it.
  This violated the intended in-checkout research boundary.
- Candidate `s04` made mutation-test backups in `/tmp/init_backup.py` **after**
  its initial cx lookup and source edit. This violated the strict file-write
  boundary but did not precede or contaminate the measured initial retrieval.

These deviations are not hidden or counted as perfect task-scope compliance.
Excluding the discard pair for a strict-scope sensitivity comparison leaves two
paired tasks across cachetools and blinker: **0/2 baseline versus 2/2 candidate**
effective source retrieval, all code gates passing. The parent accepts the
navigation package on that clean-pair evidence plus the separately qualified
full observation, not on a claim of flawless agent filesystem behavior.

Earlier rounds remain failures under their own recorded gates:

- Round 1: longer descriptions only helped GPT frequency, not cross-model
  adoption; the long candidate was not adopted.
- Round 2: compact/bilingual/priority metadata yielded zero DeepSeek cx calls.
- Read-only explicit capability probe: two successful cx calls; excluded from
  adoption statistics.
- Round 3: examples 1/2 development adoption; tail-priority 2/2 development but
  failed validation. One shell stderr-redirection false positive and scratch
  lookup were found on review; the original failed verdict was not rewritten.
- Round 4: corrected timestamp-based observation, manual shell review, identical
  lock-boundary clarification in both arms, and the repaired runtime in both
  arms. The underlying behavior assertions were unchanged.

## Validation and artifacts

- Pi extension tests: **52/52** after source integration.
- Adoption/flow tests: **18/18** (16 JavaScript + 2 TypeScript).
- TypeScript checks: passed.
- Behavior evaluator self-tests: **4/4**, including known-good and deliberate
  broken implementations.
- The integrated policy and all registered metadata are asserted equal to the
  winning frozen snapshots in `tests/pi-extension/navigation.test.ts`.
- No model was switched after a provider failure; no basic tools were blocked to
  manufacture adoption; no tests were relaxed to erase prior failures.
- No commit or push was performed.

Sanitized per-trial metrics: [results.json](results.json).
Full local evidence (manifests, raw tool events, request-presence audits, native
receipts, task outputs, code diffs, logs and retained checkouts):
`~/.pi/agent/benchmarks/cx-adoption/continued-2026-09-17/`.
Those manifests retain the original `/tmp/...` execution paths and are historical
records, not a relocated runnable environment.
