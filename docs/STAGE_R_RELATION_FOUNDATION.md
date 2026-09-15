# Stage R — command-local relation foundation

Implements the first stage of [the task-analysis plan](AGENT_TASK_ANALYSIS_PLAN.md).
This is **not** an implementation of `impact`, `changes`, or `context`. No new
CLI/Pi tool, daemon, persisted graph, dependency, or release was introduced.

> Follow-up status: [the ANGE fixes](../bench/task_analysis/ANGE_FIX_REPORT.md)
> close the failures found by the real-task evaluation: 403 Rust tests, 18 mutation
> controls and 8/8 frozen tasks pass. Relation-first still costs more than the
> source-first baseline; no universal task-efficiency benefit is claimed.

## What changed

- `src/relation_index.rs` is shared by callers and callees. It builds a sorted
  candidate table, include lookup, and the requested call-site facts once per
  command. Resolution returns typed candidate references, not a display-name key.
- `DefinitionSiteId` contains root-relative file, language, and byte range. It is
  valid within a content snapshot, **not** a cross-version logical symbol ID.
  Embedded HTML units retain host offsets and are never bound as browser symbols.
- Overloads and same-name definitions retain their distinct sites. Colliding
  candidate labels include `name [file:start..end; language]`; offsets are bytes,
  zero-based, with an exclusive end. Resolved legacy `from`/`to` remain strings.
- C/C++ declarations associate with a definition only when language, qualified
  name, complete normalized signature, and a same-file/direct-include fact agree,
  and exactly one definition qualifies. Associated declaration sites are retained.
  Unmatched declarations are **not** globally suppressed by another definition.
  Declaration outline signatures can be truncated to one line: association uses
  the full declaration range from validated source instead of that display text.
- Written qualifiers match complete qualified names or enclosing namespace
  prefixes, never substrings. A failed qualifier cannot fall through to an
  unrelated unique name. Same-file lexical visibility and imported overloads are
  considered together. A project-wide name-only match remains `syntax`.
- Unknown receivers, Rust macro invocations, TypeScript `new` expressions, and
  detected parameter/local/import binding conflicts stay unresolved. This is
  conservative blocking, not a general binder. C/C++ include evidence is not
  transplanted to Rust/TS import aliases.
- Callee names follow grammar fields, not the last type/argument descendant.
  C++ builtin named casts and primitive scalar casts do not become calls; their operands remain traversed.
  C++ templates and Rust turbofish retain the named function and receiver form.
  Unsupported computed callees are disclosed rather than invented as key/value calls.
- C++ reference-return declarations/definitions, including members and templates,
  are extracted; indexes built with older extraction rules are rebuilt.
- The nearest AST execution container owns a call. A nested function's calls are
  not included in its outer function's callees. Anonymous closures have an explicit
  `(anonymous scope@BYTE)` owner instead of being attributed to their outer host.
- Repeated calls to the same target on one line remain separate sites. Source
  order is deterministic; file ordering and candidate ordering are also stable.

## Content consistency and coverage

Symbols/imports use the indexed content identity. Supported-language files are
read once and compared against that identity **before** extracting call sites or
full declaration signatures. A changed/unreadable file contributes no new call
sites; candidate narrowing is disabled rather than making a remaining target
spuriously unique. Changing a file after the read cannot mix in a third version:
the parser consumes the already-read bytes. Tests inject the reader barrier;
they do not rely on sleeping to provoke a race.

To preserve one-hop cost, callers parse files containing the requested lexical
name; callees parse files containing matching **definition** sites, not declaration-only headers. The candidate table
is global, but **parse coverage is only the requested call-site scope**, not a
complete repository graph. Unselected candidate symbols remain indexed syntax
facts. A future impact implementation must establish its own graph-wide coverage.

Error-recovery trees retain valid call nodes outside malformed syntax, with
`parse_error` disclosure. Malformed call nodes themselves are not emitted.
Unsupported call languages and index-level grammar skips are disclosed. Read,
content, grammar, and parse failures cannot silently become complete empty success.

The existing JSON-v1 `warnings: string[]` contains a structured record prefixed
`relation_coverage: `; the suffix is JSON:

- `model`: `direct_syntax_v2`;
- `scope`: indexed candidate facts plus the requested caller/callee parse scope;
- `generation`: the index generation (freshness mode is **not** upgraded);
- `snapshot_id`: a non-cryptographic content-manifest identity including the model
  version, extraction revision, sorted paths, languages, and indexed content hashes;
- `files_checked`, `files_analyzed`, `files_skipped_missing_grammar`;
- `complete_within_model`: false on skipped/degraded coverage, never a safety claim;
- `issues`: at most 16 `{file, reason}` samples, prioritized by failure kind then path
  so unsupported-language inventory cannot hide critical read/parse failures;
- `issues_total`, `issues_omitted`, and unabridged per-reason `issue_counts`;
- `limitations`: the static model and scope boundaries.

Reasons are `content_changed`, `read_failed`, `missing_grammar`, `parse_error`,
`unsupported_call_form`, and `unsupported_language`. An unmodelled computed callee
is not a complete empty success. These are per-file primary-reason counts; a parse
error takes precedence over unsupported call forms in the same file. Index-level
grammar skips have an explicit count, but the
current index does not retain their per-file identities. Unsupported files remain
outside call extraction. The manifest identifies indexed facts, not an atomic
worktree snapshot: metadata can still miss new files/edits outside the read scope.
A failed snapshot check has `complete_within_model: false` even if its manifest ID
is the same as the earlier valid generation. Do not use the ID alone as validity.

## Output and compatibility

- `INDEX_VERSION = 16` rebuilds old symbol facts and supports lazy content/generation-bound compact task facts.
  `SCHEMA_VERSION = 1` is unchanged. The later task commands move the development package to 0.8.0; no release was made.
- The eight envelope keys, edge fields, exit-code behavior, and one-hop commands
  are retained. Partial analyses continue to return evidence plus warnings, not
  a new CI safety exit code. `callees` distinguishes no subject and declaration-only
  subjects in warning strings; ambiguous subjects remain an empty warned success.
- A body subject is selected by **definition site**, not deduplicated display labels.
  Multiple definitions in different files/languages or overloads still require
  disambiguation. One definition can be read without inventing equivalence to its
  unmatched declarations; `relation_subject:` warns about those sites and the
  resolver retains them. With no definition, declaration-only subjects are disclosed.
  Exact selection among equal-qualified definitions remains outside this legacy interface.
- Scope filtering of callers includes unresolved sites when a typed candidate's
  qualified name matches. `relation_scope:` discloses the retained frontier; `to`
  remains empty and the full candidate set survives. This is potential, not resolved,
  scope membership. `page.total` counts the emitted resolved and uncertain sites.
- Finite relation pages target 16 KiB of serialized JSON, including envelope,
  warnings, and next-query strings. Pages may contain fewer than `--limit` rows.
  All candidate evidence stays intact; `page.total` is the exact matched-site
  count, and `next_queries` advances by the number actually returned. Byte-budget
  pagination is disclosed with `relation_output_budget:`. `--all` opts out.
- One indivisible row or metadata block can exceed the target. It is retained
  with an explicit irreducible-overflow warning, not erased or turned into an
  empty page that cannot advance. This is not an unconditional byte hard limit.
- No Pi adapter/schema was changed: it already accepts arbitrary result rows and
  warning strings. A packaged Pi binary was **not** substituted or released.

Two old test expectations were corrected, with their original corpus unchanged:
`run` has 10 callable entities rather than 8 deduplicated labels (Rust/C++
`alpha::run` differ, and the unrelated `Tickable.run` declaration survives).
`callees --scope 'alpha::*'` must reject those two language-specific bodies.
A separate uniquely scoped C++ case still tests successful scope selection.

## Validation and boundaries

See [the initial report](../bench/task_analysis/REPORT.md) and
[the completed ANGE fixes](../bench/task_analysis/ANGE_FIX_REPORT.md) for preserved
red/green evidence, 18 killed mutations, all Cargo gates, exact counts and paired cost.
The reference corpus also exposed two worthwhile controls: multiline declarations
must not turn a non-leaf into an ambiguous empty response, and the two `key()`
calls on one source line must not collapse into one site.

This remains a bounded syntactic foundation, not compiler semantics. General
binding/shadowing, overload/type resolution, template instantiation, dynamic
execution, transitive includes, source grammar changes during a query, and
runtime test coverage are not proved. No test recommendation or runner is
produced. There is no held-out task-benefit claim and no permission implied to
start Stage I, commit, or publish.
