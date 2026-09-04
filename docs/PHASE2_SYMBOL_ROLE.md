# Phase 2 — SymbolRole and schema migration

> Roadmap reference: `docs/AGENT_NATIVE_DEVELOPMENT_ROADMAP.md` §4.2, §5.1, §11 Phase 2.
> `INDEX_VERSION` 8 → 9. Old indexes rebuild automatically.

## Problem

A C++ header prototype and its implementation were both indexed as plain
definitions. The only difference visible to an agent was a trailing `;` inside
the `signature` string — not a contract:

```text
$ cx --json definition --name validate_param --all      # before
[{"file":"include/ange/ecs.hpp", …}, {"file":"src/ecs.cpp", …}]
```

## Change

`SymbolRole` is a new field on `Symbol`, independent of `SymbolKind`:

| Role | Meaning |
| --- | --- |
| `definition` | implementation: body, type with members, namespace block |
| `declaration` | signature only |
| `heading` | Markdown document section |
| `unknown` | grammar cannot distinguish the forms for this construct |

Roles are **never** inferred from the presence of `{}`. They come from the
capture name in each language's tree-sitter query, so `extract.rs` now reads
`@definition.*`, `@declaration.*` and `@unknown.*` prefixes and normalizes the
kind-lookup key to `definition.<suffix>` — a language's existing
`kind_overrides` table keeps working for every role without duplicated rows.

Queries relabelled (no change in which symbols are captured — see *Scope* below):

- **C++**: function prototypes, in-class method/constructor declarations,
  `field_declaration` members, and bodyless `class Foo;` / `struct Foo;` forward
  declarations. The body-bearing `class`/`struct` patterns are listed *after* the
  bodyless ones so they win the same-byte-range dedup.
- **C**: prototypes plus bodyless `struct`/`union` forward declarations.
- **Rust**: `function_signature_item` (trait requirements, `extern` block items)
  and `associated_type`. A trait method *with* a default body stays a definition.
- **TypeScript**: `method_signature` (interface members),
  `abstract_method_signature`, and `function_signature` (`declare function`,
  overload signatures).
- **Markdown**: headings carry `role = heading`.

CLI/output:

- `role` appears in every symbol row (`symbols`, `overview`) and in
  `definition` output, in both TOON and JSON.
- `--role definition|declaration|heading|unknown` filters `symbols` and
  `definition`.
- `cx definition` now sorts implementations ahead of signature-only sites, so the
  first result is the body an agent actually wants. `role_priority` puts
  `unknown` last, so an unlabelled construct never outranks a proven body.

## Result

```text
$ cx symbols --file include/ange/ecs.hpp
[6]{name,kind,role,signature}:
  EcsWorld,class,definition,class EcsWorld
  EcsWorld,fn,declaration,EcsWorld();
  ange,module,definition,namespace ange
  entity_count,fn,declaration,int entity_count() const;
  run,fn,declaration,void run();
  validate_param,fn,declaration,"void validate_param(const std::string& name, int value);"

$ cx --json definition --name validate_param --all
  src/ecs.cpp            definition     # body first
  include/ange/ecs.hpp   declaration
```

## Scope discipline

Two capture additions were reverted mid-phase because they changed symbol
*coverage*, not roles:

- TypeScript `property_signature` (would have added interface fields as new
  symbols, +2 in a 1-symbol test)
- C++ destructor prototypes (`~EcsWorld();`, would have taken the fixture corpus
  from 41 to 42 symbols)

Both may be worth adding, but as their own change with their own justification.
Phase 2's diff relabels existing captures and adds nothing to the index: the
corpus still holds exactly 41 symbols, now 31 definition / 6 declaration /
4 heading / 0 unknown.

## Migration

`INDEX_VERSION` 9 forces a rebuild of any index written by an older cx.
`Symbol.role` also carries `#[serde(default)]` = `Definition` so a
version-compatible payload never fails to decode — but the version gate means a
pre-Phase-2 index is re-parsed rather than silently defaulted.
`test_pre_role_index_version_is_rebuilt_with_roles` stamps version 8 onto a real
index and asserts the rebuilt entries come back with correct roles, then reopens
to confirm the rebuild was persisted at the new version.

## Tests

- 10 new unit tests in `src/language/tests.rs`: C++ prototype/body, in-class vs
  out-of-class members, forward class declaration, C prototype + forward struct,
  Rust trait requirement vs impl vs default body, Rust `extern` block,
  TypeScript interface vs class vs abstract member, `declare function`, Markdown
  headings, and languages with no declaration form.
- 5 new corpus tests in `tests/fixture_corpus.rs`, including the flipped
  `cpp_declaration_and_definition_are_machine_distinguishable` (was
  `…_are_currently_indistinguishable`) and exact role histogram 31/6/4/0.
- 1 new index test for the v8 → v9 migration.
- 3 integration tests updated for the new TOON column.

Suite: 167 unit + 23 corpus + 62 integration + 7 path identity = 259 passing.

## Cost

`./scripts/bench.sh . 5`, same host as Phase 0/1. Query latency is unchanged;
output grows by roughly `,definition` per row:

| Metric | Phase 1 | Phase 2 |
| --- | --- | --- |
| cold build | 0.09 s / 33.7 MiB | 0.10 s / 33.6 MiB |
| index size | 1,056,768 B | 1,056,768 B |
| root overview | 36 ms / 761 B | 34 ms / 785 B |
| file overview | 31 ms / 183 B | 35 ms / 212 B |
| definition | 32 ms / 6,878 B | 33 ms / 7,588 B |
| references | 59 ms / 443 B | 64 ms / 443 B |
| symbol search | 31 ms / 2,750 B | 33 ms / 4,286 B |

The symbol-search growth (+56%) is the honest price of a per-row fact. It was
tempting to emit the column only when a result set contains a non-definition
role, but suppressing a fact to save budget is the same class of mistake as
dropping a truncation flag (roadmap §12), and it would make the column set
depend on data. If the cost needs to come down, the right lever is an explicit
field selector (`--fields`), not an implicit conditional — deferred to Phase 3,
where the JSON envelope is designed.
