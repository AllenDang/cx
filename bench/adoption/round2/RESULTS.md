# Round 2 — interim findings

Historical result: the three metadata candidates did not meet the preregistered
adoption criterion. The subsequent read-only probe proved the tools were callable
and their metadata reached the provider. A later package, not these candidates
alone, was promoted after [round 4](../round4/RESULTS.md).

Native workflow: `dcd5d0b0-4a40-4923-9a81-88344c6aef22`.
It completed 9 editing trials and 4 deterministic evaluation relays (13 children).
The runtime cap was 30, actual use 13; future reproductions should allow 31 for
the theoretical longest path described in README.

| Condition | Editing trials | Any cx calls | Meaningful source adoption | Host code gates |
|---|---:|---:|---:|---:|
| New-task baseline | 3 | 0 | 0/3 | 2/3 |
| Compact English recommendation | 2 | 0 | 0/2 | 2/2 |
| Bilingual recommendation | 2 | 0 | 0/2 | 2/2 |
| Explicit source-navigation priority | 2 | 0 | 0/2 | 2/2 |

All three candidate development stages failed on adoption, not infrastructure or
code correctness. Consequently the candidate validation/compatibility stages
were correctly skipped; their prepared clones are **not executed samples**.

The new-task baseline failure is real: `cache_reset_stats` with `cache=None` and
an explicit lock resets counters outside the lock. The host check catches this;
no acceptance amendment was made. The original and sample pytest suites alone
passed, illustrating why independent behavior checks remain necessary.

Registered-tool/exposure checks passed, but those checks do not prove the
provider received the same tools and system instructions. Because even an
explicit navigation policy had no observable effect, the next step is a
separate **read-only, explicitly named tool capability probe** with sanitized
`before_provider_request` telemetry. It is not an editing request and is not
included in adoption statistics. The telemetry records tool names and exact
metadata-presence booleans, not credentials, headers, or raw message content.

The first probe launch was rejected before any child/model invocation because
the existing mission belongs to `/tmp/cx-adoption-round2`, not the new probe
root. The isolated probe checkout was verified clean at upstream HEAD; the same
native workflow was launched from the original mission root with an explicit
probe cwd. No CLI/model fallback was used.
