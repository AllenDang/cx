# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.10] - 2026-09-11

### Added

- HTML and HTM inline JavaScript navigation with original source coordinates, independent script parsing, syntax-level call candidates, and relative module import map dependencies.
- HTML grammar bundled in all six pi-cx platform assets.

### Changed

- Index format version 13 rebuilds older indexes to include HTML files.

## [0.7.9] - 2026-09-09

### Added

- Cross-extension `cx:mark-dirty:v1` notifications let file-mutating Pi extensions request an automatic verified path refresh before the next CX query, without depending on Hashline Edit.
- Dirty refreshes are deduplicated and serialized per canonical project root, merge with explicit `cx_refresh`, preserve in-flight events, and expose bounded `dirtyRefresh` tool details.
- Named refresh reads are capability-scoped beneath the project root and verify stable file identity and contents before publishing freshness evidence.

- `pi-cx` Git Pi package with eight typed navigation tools, strict bundled-binary verification, offline grammar seeding, and per-platform release assets for macOS, Linux, and Windows on arm64/x86_64.
- Cold-cache sibling cx processes now wait with bounded exponential backoff for the index writer instead of failing after two seconds.
- pi-cx now renders tool failures as errors, validates argument bounds at runtime, documents qualified-name scope globs, and rejects qualified names passed to `cx_references`.

- `cx callers --name X` and `cx callees --name X`: direct (one-hop) call edges, each carrying `evidence`, `resolution` (`syntax`, `lexical_scope`, `import_resolved`), file, line, and `ambiguous_candidates`. cx never picks a target it cannot justify — an unresolvable call reports an empty target and lists every candidate. Candidates are restricted to the caller's language. `callees` on an ambiguous name returns no rows and names the candidates instead of reading an arbitrary body. No multi-hop traversal and no type resolution.
- `cx references` rows now carry `evidence` (`definition`, `declaration`, `call`, `type_reference`, `import`, `identifier_reference`) and `resolution: syntax`.
- `cx map`: bounded repository orientation. Groups files into subsystems (`--depth N`), reports sizes and test/vendor/generated/docs classification, and shows import edges that resolve to an indexed file. Ranked by fan-in with the ranking basis printed; vendor, generated and test paths are excluded by default and every exclusion is reported with the flag that restores it (`--include-vendor`, `--include-generated`, `--tests`, plus repeatable `--exclude <glob>`). Low-information symbol names are suppressed from API samples.
- Import/include targets are now indexed for C/C++ (`#include`), Rust (`use`), and TypeScript (`import`/`export … from`). Imports that match no indexed file are counted as external; imports matching several are reported unresolved rather than pointed at an arbitrary one.
- `qualified` name on every symbol, built from lexical scope: `ange::EcsWorld::run`, `alpha::run`, `Tickable.run`. Modelled for Rust, C/C++ and TypeScript; empty means *unresolved*, never *top-level*.
- `--scope <glob>` filter on `cx symbols` and `cx definition`, matched against the qualified name. It never matches a symbol whose scope is unresolved.
- Ambiguity warnings now count *distinct symbols* and list their qualified names, so a C++ prototype plus its definition (one symbol, two locations) no longer looks like a conflict.
- `freshness` in every JSON result: `{generation, mode, files_checked, files_updated, files_removed, files_skipped_missing_grammar}`, so an agent can prove which index state answered its query.
- `cx refresh <paths>` re-indexes named files immediately by content hash and reports per-path status (`updated`/`removed`/`unchanged`/`not_indexed`). With no arguments it verifies the whole project.
- `--fresh metadata|verified` selects how much verification a query performs. `metadata` (default) compares size + mtime; `verified` hashes contents and catches edits that preserve both.
- Versioned JSON envelope (`schema_version: 1`) for every `--json` command: `{schema_version, query, freshness, page, results, warnings, next_queries, error}` with a fixed key set that never varies by result count.
- `next_queries` supplies exact, runnable follow-up commands for truncated pages instead of a prose hint.
- Machine-readable error codes: `file_not_indexed`, `unsupported_file_type`, `no_indexed_files`, `grammar_not_installed`.
- `warnings` reports ambiguity, e.g. several candidates sharing one symbol name.
- `role` on every symbol, distinguishing `definition` from `declaration` (plus `heading` and `unknown`). C/C++ prototypes and forward type declarations, Rust trait requirements and `extern` items, and TypeScript interface/abstract/ambient members are now machine-distinguishable from implementations. Roles come from grammar captures, never from guessing at braces.
- `--role` filter on `cx symbols` and `cx definition`.
- `cx definition` sorts implementations ahead of signature-only sites, so the first result is the body.
- Fixture corpus (`tests/fixtures/agent_corpus`) and `scripts/bench.sh` for reproducible correctness and performance baselines.

### Changed

- Whole repository formatted with `cargo fmt`, and `cargo fmt --all -- --check` added as a CI job so it cannot regress. CI clippy widened to `--all-targets --all-features`.
- `INDEX_VERSION` 11 → 12; existing indexes rebuild automatically on first use. Index entries now store lexical scope, import targets, file size, and a content hash (all recorded from data already available during parsing, so indexing cost is unchanged).
- TOON and JSON symbol rows include a `qualified` column; `cx definition` plain-text output adds a `qualified:` line when the scope is resolved.
- Ordinary queries now compare file size in addition to mtime, catching same-mtime edits that change length.
- **Breaking (`--json` only):** JSON output is always an envelope object. Previously the root was a bare array unless results were truncated or offset. Read `results` for rows and `page` for `{total, offset, limit, truncated}`.
- A successful query with zero results now returns a parseable envelope with `results: []` and exit 0, instead of printing nothing.
- Under `--json`, cx no longer duplicates notes and pagination hints on stderr; the payload is authoritative. Without `--json`, stderr output is unchanged.
- One canonical path identity for the project root, index cache key, and every path argument: symlinked spellings such as `/tmp/p` and `/private/tmp/p` now share an index and accept arguments in either form. Case-only aliases remain separate (see KNOWN_ISSUES.md).
- `INDEX_VERSION` 8 → 9; existing indexes rebuild automatically on first use.
- TOON and JSON symbol rows include a `role` column; `cx definition` plain-text output includes a `role:` line.

### Fixed

- `cx map` no longer resolves includes by scanning every indexed path per import. A prebuilt component-boundary suffix lookup replaces the `O(imports × files)` scan, taking `map --depth 2` on a 3,905-file corpus from 3.4 s to 0.11 s median. File/External/Ambiguous outcomes are unchanged and were verified byte-identical on that corpus, including the 74 imports reported as ambiguous.
- `scripts/bench.sh` only benchmarked the current directory correctly: relative path arguments resolved against the caller's cwd, so pointing it at another project silently aborted the whole warm-query section under `set -e`. It now also measures `map`, `callers` and `callees`.
- `cx --json symbols` with zero matches printed nothing instead of the standard envelope, unlike every other command.
- `cx --root /tmp/p overview /private/tmp/p/src/a.rs` no longer fails with "file not in index".
- `cx refresh` no longer derives the project root from its path arguments, which could silently retarget cx at an unrelated directory and build a new index there.

## [0.7.2] - 2026-07-23

### Fixed

- Relative path arguments containing `.` or `..` now resolve consistently across overview, symbols, definition, and references.

## [0.7.1] - 2026-05-15

### Added

- Objective-C support for `.m` and `.mm` files, including classes, protocols, methods, and C functions.

## [0.7.0] - 2026-05-14

### Added

- Markdown heading navigation: `.md`, `.markdown`, and `.mdown` files are indexed by headings, and `definition` returns the selected heading section.
- Line ranges in `overview` output for more precise navigation.
- `cx symbols --kinds` to list available symbol kinds with counts.
- Directory paths in `--file` and `--from` filters.
- C++ header declaration indexing.
- Windows ARM64 release support.

### Changed

- References now default to the compact grouped summary; exact matching lines are available with `--context`.
- Overview includes test files and test symbols by default; use `--no-tests` to exclude them.
- Absolute path arguments now derive the project root from the provided path instead of only the current working directory.
- Full index crawling is parallelized.
- Query coverage expanded across TypeScript, Python, Go, Rust, Java, C++, C, Solidity, Ruby, Lua, Bash, and Zig.
- `SymbolKind::Method` was collapsed into `fn` for simpler output.

### Fixed

- Directory overview and symbol/definition filtering now handle current-working-directory and absolute-path resolution more consistently.
- C++ declaration-only headers at nested paths are indexed correctly.

## [0.6.3] - 2026-04-04

### Added

- `CX_CACHE_DIR` env var to override the cache location (#14) — enables cx in sandboxed agents (Codex, Claude Code) that restrict writes outside the workspace

## [0.6.2] - 2026-04-04

### Added

- **Pagination** (#15): Global `--limit`, `--offset`, `--all` flags across all query commands
  - Default limits: definition (3), symbols (100), references (50)
  - Compact stderr hint when truncated: `cx: 3/32 definitions for "X" | --from PATH to narrow | --offset 3 for more | --all`
  - JSON uses `{total, offset, limit, results}` envelope when paginated, bare array otherwise
  - `--all` and `--limit` are mutually exclusive (enforced by clap)
- Definition results sorted by symbol priority (types first) before pagination

### Changed

- Definition paginates before reading bodies from disk (avoids wasted I/O on large match sets)
- Skill prompt trimmed from ~1000 to ~350 tokens

## [0.6.1] - 2026-04-02

### Added

- **Dart language support** (#9, requested by @evanscai): classes (sealed/base/interface/mixin), mixins, extensions, extension types, enums, functions, methods, getters/setters, constructors (named/factory), operators, type aliases
- **Comprehensive Swift support** (based on #11 by @upupc): actors, extensions, properties, subscripts, enum bodies, init/deinit (#10, #12)
- **Elixir enhancements** (#6 by @RamXX): `@type`/`@typep`/`@opaque`, `@callback`, `defimpl`
- **Directory overview** (#8, reported by @it-ony): `cx overview dir/` — single-level table of contents with symbol names, `--full` for signatures
- Test symbol filtering in directory overviews — excludes test files by path pattern and Rust `#[test]`/`#[cfg(test)]` inline tests

### Changed

- Language module refactored into focused files (`queries/*.rs`, `extract.rs`, `tests.rs`)
- `RwLock` + thread-local `Parser` for better parallel indexing performance
- Symbol dedup now prefers later (more specific) query matches for same byte range
- Index version bumped to 6 (forces reindex)

### Fixed

- `--root` flag now correctly resolves relative paths against the project root instead of cwd

## [0.6.0] - 2026-03-30

### Changed

- **Breaking:** Index database moved from `.cx-index.db` in the repo root to `~/.cache/cx/indexes/`. No more repo pollution or `.gitignore` dance.

### Added

- `cx cache path` — print the index cache path for the current project
- `cx cache clean` — delete the cached index for the current project

### Removed

- `.cx-index.db` repo-local index file
- Gitignore warning on first run

### Fixed

- Flaky incremental update tests on filesystems with coarse (1-second) mtime granularity

## [0.5.0] - 2026-03-25

### Added

- `cx lang add <languages>` — download and install language grammars on demand
- `cx lang remove <languages>` — remove installed grammars
- `cx lang list` — show supported languages and install status
- Actionable warnings when grammars are missing during indexing
- First-run UX: shows detected languages with file counts and install command

### Changed

- Grammars are now dynamically loaded via `tree-sitter-language-pack` instead of
  statically linking 14 `tree-sitter-{lang}` crates
- `Language` enum replaced with string-based language identification
- `FileEntry` serialized with bincode (index version bumped to 4, forces reindex)
- tree-sitter upgraded from 0.25 to 0.26
- Zig and Python queries updated for newer grammar versions
- `find_references` now returns `Result` and propagates `NotInstalled` errors
- Release binary reduced from ~25MB to ~7MB

### Removed

- Static dependency on 14 individual `tree-sitter-{lang}` crates

## [0.4.5] - 2026-03-24

### Changed

- Updated Cargo.lock for redb 3 upgrade

## [0.4.4] - 2026-03-23

### Fixed

- x86_64 macOS build runner configuration

### Added

- Release workflow and install script
- Concurrent read access via redb 3 upgrade
