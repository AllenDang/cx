# Phase 5 — qualified symbol identity

> Roadmap reference: `docs/AGENT_NATIVE_DEVELOPMENT_ROADMAP.md` §4.3, §5.2, §5.3, §11 Phase 5.
> `INDEX_VERSION` 10 → 11. Acceptance: same-name symbols in different scopes are never silently merged.

## Problem

Twelve different `run` symbols across namespaces, modules, classes, vendored and
generated code were represented only by `name + file + range` (§4.3). An agent
could not tell `alpha::run` from `beta::Runner::run`, and `cx definition --name
run` returned an ordered list that looked authoritative. Worse, the Phase 3
ambiguity warning counted *rows*, so a C++ prototype plus its definition — one
symbol at two locations — was announced as "2 candidates".

## Change

### Data model

`Symbol` gained two fields:

- `scope_path: Vec<String>` — lexical containers, outermost first
  (`["ange", "EcsWorld"]`)
- `qualified_name: Option<String>` — `None` when cx does not model that
  language's scopes

`None` is the important part. Reporting the bare name as "qualified" would claim
a resolution that never happened, so an unmodelled language reports *unresolved*
and a modelled top-level symbol reports its own name. Those are different facts
and the output keeps them different.

`StableSymbolId` (§5.2) is **derived**, not stored: the index keeps language,
qualified name, kind and signature, and `Symbol::stable_id()` composes them on
demand. Changing the identity scheme therefore does not require an index rewrite.
`logical_key()` deliberately ignores the signature so a declaration and its
definition share one identity.

### Scope extraction

`scope_model(lang)` describes three genuinely different scope shapes, as §11
requires, rather than one guessed rule:

| Language | Scope nodes | Separator | Written qualifiers |
| --- | --- | --- | --- |
| Rust | `mod_item`, `impl_item` (via its `type` field), `trait_item` | `::` | `scoped_identifier` |
| C/C++ | `namespace_definition`, `class_specifier`, `struct_specifier`, `union_specifier`, `enum_specifier` | `::` | `qualified_identifier` |
| TypeScript | `class_declaration`, `abstract_class_declaration`, `interface_declaration`, `module`, `internal_module`, `enum_declaration` | `.` | — |

Two syntactic sources are combined:

1. enclosing scope nodes walked up the AST — `namespace ange { class W { … } }`
2. qualifiers written at the definition site — `void W::run() {}`

C++ needs both: `void EcsWorld::run()` inside `namespace ange` yields
`ange::EcsWorld::run` only if the namespace comes from the AST and `EcsWorld`
from the declarator. No import or type resolution happens — these are syntax
facts, which is all Phase 5 claims.

### Output and filters

- `qualified` appears in symbol rows and `definition` results, in both TOON and
  JSON. Empty means unresolved.
- `--scope <glob>` filters on the qualified name (`--scope 'alpha::*'`) for
  `symbols` and `definition`. It **never** matches a symbol whose scope is
  unresolved: silence beats a false positive.
- The ambiguity warning now counts distinct logical symbols and lists their
  qualified names (bounded to 5 plus an ellipsis, sorted for stable output).

## Result

```text
$ cx --json symbols --name run --all      # 12 locations, 10 distinct symbols
generated/gen_api.cpp        gen::run              definition
include/ange/ecs.hpp         ange::EcsWorld::run   declaration
src/app.ts                   Tickable.run          declaration
src/app.ts                   AlphaRunner.run       definition
src/app.ts                   run                   definition
src/ecs.cpp                  ange::EcsWorld::run   definition
src/lib.rs                   alpha::run            definition
src/lib.rs                   beta::run             definition
src/scope_a.cpp              alpha::run            definition
src/scope_b.cpp              beta::Runner::run     declaration
src/scope_b.cpp              beta::Runner::run     definition
vendor/thirdparty/blob.cpp   thirdparty::run       definition
```

Before, `cx definition --name validate_param` warned "2 candidates share the
name"; now it emits **no** warning, because both rows are one symbol
(`ange::validate_param`) seen as a declaration and a definition. `--name run`
warns about 10 distinct symbols and names them.

## A bug this phase found

`cx --json symbols` with zero matches printed nothing on stdout — the Phase 3
empty-envelope fix had silently failed to apply to `symbols()` (a partial edit),
leaving one command violating the contract every other command honored. Caught by
`scope_filter_never_matches_unresolved_scopes`, which is the first test to ask
`symbols` for a deliberately empty result set under `--json`. Fixed; the empty
case now flows through the standard emit path.

This is the concrete argument for §12.9's "pin exact result counts": a test that
asserted `>= 0` rows would have passed.

## Tests

`tests/qualified_identity.rs`, 11 tests:

- C++ namespaces, in-class declarations, and out-of-line member definitions
- Rust modules, including the `#[cfg(test)]` module qualifying its items
- TypeScript classes/interfaces, verifying the `.` separator rather than `::`
- unmodelled language (Markdown) reports unresolved, not top-level
- the full 12-location → qualified-name mapping, pinned exactly
- `--scope` narrowing for `symbols` and `definition`, including a nested scope
- `--scope '*'` matching nothing when scopes are unresolved
- `qualified` in JSON and plain-text output, absent when unresolved
- `--from` and `--scope` selecting the same symbol

Plus the flipped `ambiguous_definition_reports_distinct_qualified_symbols`, 2 new
index unit tests (`stable_id` separating identity from display name; scope fields
surviving a bincode roundtrip), and 8 integration assertions updated for the new
column.

Suite: 172 unit + 27 corpus + 13 freshness + 11 identity + 62 integration +
7 path identity = 292 passing, `clippy --all-targets -D warnings` clean.

## Cost

`./scripts/bench.sh . 9`, same host as earlier phases:

| Metric | Phase 4 | Phase 5 |
| --- | --- | --- |
| cold build | 0.09 s / 36.1 MiB | 0.10 s / 35.5 MiB |
| index size | 1,056,768 B | 1,056,768 B |
| root overview | 31 ms | 31 ms |
| definition | 31 ms / 7,588 B | 31 ms / 7,602 B |
| references | 73 ms | 74 ms |
| symbol search | 32 ms / 4,359 B | 31 ms / 6,274 B |

Latency is unchanged: scope extraction is a parent-chain walk during indexing,
not per query. Symbol-search output grew 44% because most rows now carry a
qualified name — the same trade as Phase 2's `role` column, and the reason a
`--fields` selector is still the right lever if output budget becomes binding.
