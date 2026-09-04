# Phase 7 — direct callers and callees with evidence levels

> Roadmap reference: `docs/AGENT_NATIVE_DEVELOPMENT_ROADMAP.md` §5.4, §9, §11 Phase 7.
> One hop only. No index change.

## Problem

`cx references` found every syntactic occurrence of a name but said nothing about
what each occurrence *was*, and nothing connected a call to the definition it
refers to. The tempting shortcut — connect every call named `run` to every symbol
named `run` — is exactly what §9 forbids, because in the fixture corpus that
manufactures 50 edges out of 6 real call sites.

## Change

### The evidence ladder is explicit

```rust
enum EvidenceKind { Definition, Declaration, Call, TypeReference,
                    IdentifierReference, Import, Text }

enum ResolutionLevel { Text, Syntax, LexicalScope, ImportResolved, TypeResolved }
```

Every edge and every reference states the highest level actually reached:

| Level | What justifies it |
| --- | --- |
| `syntax` | the AST places this identifier in a callee position |
| `lexical_scope` | a qualifier written at the call site matched exactly one candidate, **or** exactly one candidate is visible by lexical nesting |
| `import_resolved` | the calling file imports the file defining exactly one candidate |
| `text` / `type_resolved` | **never produced**, and marked as such in the source |

`Text` and `TypeResolved` are kept in the enums with `#[allow(dead_code, reason =
…)]` rather than deleted: they are the vocabulary a consumer reads to understand
what the levels mean, and keeping them makes it explicit that cx never claims
them. A name-only match is `syntax`, never `lexical_scope`.

### Candidates are narrowed, never chosen

`resolve_call` tries, in order: same-language filtering, written qualifier,
import resolution (reusing Phase 6's `resolve_import`), then lexical nesting.
If none narrows the set to one, the edge keeps `to` empty and fills
`ambiguous_candidates`. A single project-wide candidate is reported as `to` with
`resolution: syntax` — uniqueness is a fact, but it is not scope reasoning, so it
does not get a scope-level label.

Same-language filtering matters more than it looks: without it, a C++
`runner.run()` would list `AlphaRunner.run` from TypeScript among its candidates.

### Two commands, one hop

- `cx callers --name X [--scope GLOB]` — call sites pointing at X
- `cx callees --name X [--scope GLOB]` — calls written inside X's body

`callees` on an ambiguous name returns **no rows** plus a warning naming the
candidates, because reading one arbitrary body would answer a different question
than the one asked. There is deliberately no `--depth`: §9 puts multi-hop last,
and a test asserts the flag does not exist.

### References gained evidence too

§14 requires references to carry evidence and resolution. Each occurrence is now
classified from the AST (`call`, `type_reference`, `import`,
`identifier_reference`) and upgraded to `definition`/`declaration` when the
tightest enclosing symbol *is* the symbol being referenced — that occurrence is
the symbol's own name, not a use of it. Resolution is `syntax`, which is the
honest ceiling for a reference query; edge resolution is what `cx callers` is for.

## Result — the acceptance case

```text
$ cx callers --name run
[6]{from,to,evidence,resolution,file,line,ambiguous_candidates}:
  run,run,call,lexical_scope,src/app.ts,12,""
  run_both,"alpha::run",call,lexical_scope,src/lib.rs,18,""
  run_both,"beta::run",call,lexical_scope,src/lib.rs,18,""
  run_all,"",call,syntax,src/scope_b.cpp,13,"alpha::run, ange::EcsWorld::run, beta::Runner::run, gen::run, thirdparty::run"
  test_world_runs,"",call,syntax,tests/ecs_test.cpp,5,"alpha::run, …"
  "thirdparty::helper","thirdparty::run",call,lexical_scope,vendor/thirdparty/blob.cpp,5,""
cx: 8 distinct symbols named "run": … Edges with an empty target could not be narrowed to one.
cx: 2 of 6 call sites are syntax evidence only; cx does not resolve types
```

Reading the rows against §11's condition:

- `runner.run()` in `run_all` and `world.run()` in the test are **not** bound to
  any `run`. No cross-scope false edge.
- `alpha::run()` and `beta::run()` on the *same line* stay two separate edges
  pointing at the correct scopes.
- The vendored `run()` call resolves to `thirdparty::run` by lexical nesting —
  same namespace, one visible candidate.
- Unresolved edges list every candidate, so nothing is discarded silently.

And for an unambiguous name, everything resolves:

```text
$ cx callers --name validate_param
  "ange::EcsWorld::run","ange::validate_param",call,lexical_scope,src/ecs.cpp,18,""
  "alpha::run","ange::validate_param",call,lexical_scope,src/scope_a.cpp,7,""
```

## Tests

`tests/relations.rs`, 14 tests:

- **ambiguous call produces no edge**, lists all same-language candidates, and
  excludes cross-language ones
- written qualifiers resolve to the named scope, with two calls on one line
  staying distinct
- lexical nesting resolves a same-namespace call
- every edge has file, line, `evidence: call`, and a resolution that is never
  `type_resolved` or `text`; an empty target always carries candidates
- warnings report distinct-symbol count and how many edges are syntax-only
- unambiguous target resolves for every caller, with no ambiguity warning
- `--scope` narrowing for both commands
- `callees` refuses an ambiguous name instead of guessing
- `callees` of a leaf symbol is an empty success, not an error
- both commands use the standard envelope with freshness
- pagination with runnable `next_queries`
- **no `--depth` flag exists** (§9 ordering)

Plus 3 unit tests in `src/relations.rs` (level ordering, snake_case
serialization, tightest-enclosing-symbol selection) and the flipped
`references_are_syntax_filtered_not_text_matched`, which now pins
`declaration`/`definition`/`call` evidence at exact lines in `scope_b.cpp`.

Suite: 191 unit, 27 corpus, 13 freshness, 14 relations, 12 map, 11 identity,
62 integration and 7 path identity tests — 337 passing, `clippy --all-targets
-D warnings` clean.

## Cost

Relation queries parse the files they inspect (like `references`), so they cost
roughly what a reference query costs:

| Query | Time | Output |
| --- | --- | --- |
| `cx callers --name print_toon` | 84 ms | 327 B |
| `cx callees --name relation_report` | 50 ms | 1,841 B |
| `cx references --name cx` (baseline) | 85 ms | 489 B |

`./scripts/bench.sh . 9` shows no change to indexing or the other queries; index
size is unchanged because no new data is persisted.

## What is still not claimed

- No multi-hop traversal (§9 step 7).
- No type resolution, so no overload resolution, no virtual dispatch, no template
  instantiation. Calls that need those stay `syntax` with candidates listed.
- Method calls through a variable (`runner.run()`) are unresolved by design: the
  receiver's type is unknown, and guessing it is what §12 forbids.
