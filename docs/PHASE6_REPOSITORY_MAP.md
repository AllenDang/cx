# Phase 6 — bounded repository map

> Roadmap reference: `docs/AGENT_NATIVE_DEVELOPMENT_ROADMAP.md` §8, §11 Phase 6.
> `INDEX_VERSION` 11 → 12 (imports are now indexed). `cx overview` is unchanged.

## Problem

`cx overview` answers "what is in this directory". It does not answer "what is
this repository, and what is load-bearing in it". §8 asks for a `map` command
that does — while avoiding the failure mode it names explicitly: a ranking
dominated by third-party code, generated output, and common identifiers like
`std`, `string`, `name`, `run`.

## Change

### Imports are now facts in the index

`FileData` gained `imports: Vec<String>` — import/include targets exactly as
written, deduplicated. Extraction happens during indexing, where the file is
already parsed, so the cost is a query pass over an existing tree rather than a
second read.

Modelled languages (the same three as Phase 5, plus C):

| Language | Pattern |
| --- | --- |
| C / C++ | `(preproc_include path: (_) @import)` |
| Rust | `(use_declaration argument: (_) @import)` |
| TypeScript | `import`/`export … from` string sources |

Anything else reports **no** imports rather than a partial guess.

### Only resolvable edges become edges

`resolve_import` turns a written target into one of three outcomes:

- `File(path)` — exactly one indexed file matches
- `External` — nothing in this project matches (system header, package)
- `Ambiguous` — several indexed files match, so **no edge is claimed**

C/C++ resolution is a path-suffix match (include roots vary); TypeScript relative
specifiers are joined against the importing file's directory and tried with the
usual extensions plus `/index.*`; Rust `crate::a::b` is matched against the
conventional `src/a/b.rs`, `src/a/b/mod.rs`, `src/a.rs` layout. `std::` and
external crates are `External`. Ambiguous and external counts are reported, so a
sparse edge set is visibly sparse rather than quietly wrong.

### Filters run before ranking

Paths are classified as `production`, `test`, `docs`, `vendor`, or `generated`
from path conventions, with vendor and generated deliberately winning over test
and docs (a vendored test is still vendored). By default `map` excludes vendor,
generated **and** tests, and reports each exclusion count with the flag that
brings it back:

```text
cx: 1 files excluded as vendor (use --include-vendor)
cx: 1 files excluded as generated (use --include-generated)
cx: 1 files excluded as test (use --tests)
```

`--exclude <glob>` is repeatable for project-specific noise.

### Low-information symbols are suppressed

A 70-name stoplist (`run`, `get`, `new`, `name`, `data`, …) plus a
two-character minimum keeps API samples informative. This is the direct answer to
§8's warning about rankings dominated by `std`, `string`, `name`, `run`.

### Ranking is explained, not assumed

Rows are ranked by fan-in (`dependents`), then symbol count, then name. The basis
travels with the output — in `warnings` under `--json`, on stderr otherwise:

```text
cx: ranked by dependents desc, then symbols desc, then name
```

## Result

On the fixture corpus, the load-bearing header ranks first and the vendored blob
does not:

```text
$ cx map --include-vendor --include-generated --tests
[7]{subsystem,class,files,symbols,tests,depends_on,dependents,external_imports,api}:
  include/,production,1,6,0,"",2,2,"EcsWorld, ange"
  src/,production,6,25,0,include/,0,2,"AlphaRunner, EcsWorld, Runner, Tickable, alpha, ... (+7 more)"
  docs/,docs,1,3,0,"",0,0,""
  vendor/,vendor,1,3,0,"",0,0,"helper, thirdparty"
  generated/,generated,1,2,0,"",0,0,gen
  (root),docs,1,1,0,"",0,0,""
  tests/,test,1,1,1,include/,0,0,test_world_runs
```

`run` appears in ten of twelve fixture files and in **no** API sample.

On cx itself, `--depth 2` recovers the real module structure:

```text
src/language/,production,21,209,0,src/,1,26,…
src/,mixed,7,196,0,"src/language/, src/util/",1,32,…
src/util/,production,4,33,0,"",1,8,…
```

## Deliberately deferred: public/exported counts

§8 lists "public/exported definitions" as a v1 fact. It is **not** in this
version. The only evidence currently available is an export marker in the
signature text (`pub`, `export`), which exists in Rust and TypeScript but not
in C/C++, where visibility comes from access specifiers cx does not extract. A
column that silently means "0" for C++ would repeat exactly the mistake Phase 5
removed. Real visibility extraction is its own change; until then `map` reports
sizes, classes, edges and API samples, all of which are checkable.

## Tests

`tests/map.rs`, 12 tests:

- vendor/generated/test excluded by default, each with its reported count and
  opt-in flag
- `--exclude` glob filtering with a reported count
- ranking puts the most-depended-upon subsystem first, with vendor below it and
  at fan-in 0
- ranking basis present in both JSON warnings and stderr
- `run`/`get`/`new` absent from every API sample while distinctive names remain
- resolved include → edge, system headers → external count
- **ambiguous include produces no edge** and is reported
- `--depth` changes granularity
- pagination with runnable `next_queries` and non-overlapping pages
- list columns capped with explicit elision
- `overview` output unchanged

Plus 10 unit tests in `src/map.rs` (path classification, vendor-over-test
precedence, subsystem depth clamping, bounded lists, and all four resolution
outcomes), 6 in `src/language/tests.rs` (import extraction per language,
deduplication, unmodelled languages), and 1 in `src/util/path.rs`
(`lexical_join`).

Suite: 188 unit + 27 corpus + 13 freshness + 12 map + 11 identity + 62
integration + 7 path identity = 320 passing, `clippy --all-targets -D warnings`
clean.

## Cost

`./scripts/bench.sh . 9` plus direct `map` timing, same host as earlier phases:

| Metric | Phase 5 | Phase 6 |
| --- | --- | --- |
| cold build | 0.10 s / 35.5 MiB | 0.08 s / 38.8 MiB |
| index size | 1,056,768 B | 1,056,768 B |
| root overview | 31 ms / 796 B | 31 ms / 796 B |
| definition | 31 ms | 31 ms |
| symbol search | 31 ms | 32 ms |
| `cx map --depth 2` | — | 29 ms / 737 B |

`overview` is byte-for-byte unchanged, which §8 requires. `map` costs about the
same as any other warm query because imports are read from the index rather than
re-parsed, and its output is *smaller* than root `overview` on this repository.
