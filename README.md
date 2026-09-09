# cx

Semantic code navigation for AI agents — file overviews, symbol search, definitions, and references — without running a language server.

> Disclaimer: Built with AI.

## Install

```bash
brew tap ind-igo/cx && brew install cx
```

Or with Cargo:

```bash
cargo install cx-cli
```

Or via the install script:

```bash
curl -sL https://raw.githubusercontent.com/ind-igo/cx/master/install.sh | sh
```

On Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/ind-igo/cx/master/install.ps1 | iex
```

## Pi extension

This repository is also a Git Pi package named `pi-cx` for macOS, Linux, and Windows on arm64 and x86_64:

```bash
pi install git:github.com/AllenDang/cx@v0.7.9
```

It registers eight strongly typed `cx_*` tools and the read-only `/cx-status` diagnostic. The package downloads a version-pinned release asset during installation, verifies the bundled cx binary and Tree-sitter grammars, and uses cx's standard shared cache. It never falls back to a host `cx` on `PATH`. See [`extensions/pi-cx/README.md`](extensions/pi-cx/README.md) for security, cache, grammar, and troubleshooting details.

## Agent integration

`cx skill` prints a prompt that teaches any coding agent to prefer cx over raw file reads. Pipe it into whichever instructions file your agent reads:

```bash
# Claude Code (CLAUDE.md)
cx skill > ~/.claude/CX.md
# then add @CX.md to ~/.claude/CLAUDE.md

# Codex, Copilot, Zed, and other AGENTS.md-compatible tools
cx skill >> AGENTS.md
```

That's it. The prompt includes the command reference and the escalation hierarchy (overview → symbols → definition / references → read).

## Why

Agents burn most of their context reading files. We analyzed 105 of our own Claude Code sessions (73 pre-cx, 32 post-cx) and found:

- **66% of reads are chains** -- reading A to find B to find C, exploring before acting
- **37% are re-reads** -- same file read multiple times per session
- **Avg Read costs ~1,200 tokens** (median 594), and sessions average 21 reads

cx gives agents a cost ladder. Start cheap, escalate only when needed:

```
cx overview src/              ~20 tokens    "what's in this folder?"
cx overview src/fees.rs       ~200 tokens   "what's in this file?"
cx definition --name calc     ~200 tokens   "show me this function"
cx symbols --kind fn          ~70 tokens    "what functions exist in the codebase?"
cx references --name calc     ~1 query      "where is this used?"
```

In sessions with cx enabled, we measured **58% fewer Read calls** and **40-55% fewer tokens** spent on code navigation. The biggest wins are on chain reads and targeted lookups where `cx overview` or `cx definition` replaces a full file read.

**Why not an LSP?** Language servers are built for editors — persistent processes, 1-2GB RAM, per-language setup, and used by humans. Agents only need the ability to query the structure of their codebase. cx optimizes for that access pattern.

## How cx compares

| Tool | Overlap | cx difference |
| ------ | --------- | --------------- |
| **ctags** | Symbol indexing | Tree-sitter instead of regex, persistent db, built-in query CLI |
| **LSP** | Go-to-definition, find references, symbol search | No daemon, no compilation, no project setup — just parse and query |
| **ripgrep** | Finding code by name | Semantic — `cx definition --name X` vs grep-then-read-5-files |
| **Reading files** | Understanding code | `cx overview` ~200 tokens vs full file read ~thousands |

## Usage

### Overview -- file and directory table of contents

Directories show one level: direct files with symbol names, subdirectories with counts. Test files and test symbols are included by default; use `--no-tests` to exclude them.

```
$ cx overview .

[7]{file,symbols}:
  container/,"(3 files, 28 symbols)"
  scripts/,"(6 files, 16 symbols)"
  src/,"(19 files, 147 symbols)"
  setup.sh,"check_build_tools, check_node, detect_platform, ..."
```

Drill into a subdirectory:

```
$ cx overview src/

[7]{file,symbols}:
  language/,"(1 files, 19 symbols)"
  util/,"(3 files, 4 symbols)"
  index.rs,"Index, Symbol, SymbolKind, load_or_build, ..."
  main.rs,"Cli, Commands, main, resolve_root, ..."
```

Single file -- full symbol table with kinds, roles, line ranges, and signatures:

```
$ cx overview src/main.rs

[12]{name,qualified,kind,role,range,signature}:
  Cli,Cli,struct,definition,"16-48",struct Cli
  Commands,Commands,enum,definition,"51-135",enum Commands
  main,main,fn,definition,"180-275",fn main()
  resolve_root,resolve_root,fn,definition,"169-178","fn resolve_root(explicit: &Option<PathBuf>, path_hint: Option<&Path>) -> PathBuf"
  ...
```

Use `--full` on directories for the detailed per-file view with ranges and signatures.

Markdown files are indexed by headings. A Markdown definition returns the full section for that heading, including nested subheadings, and stops at the next sibling or parent heading.

```
$ cx overview README.md

[3]{name,qualified,kind,role,range,signature}:
  cx,"",heading,heading,"1-278",# cx
  Install,"",heading,heading,"7-30",## Install
  Usage,"",heading,heading,"77-199",## Usage
```

### Declaration vs definition

Every symbol carries a `role` that is independent of its `kind`:

| Role | Meaning |
| --- | --- |
| `definition` | the implementation: a body, a type with members, a namespace block |
| `declaration` | signature only: C/C++ prototype or in-class member, forward type declaration, Rust trait requirement or `extern` item, TypeScript interface/abstract/ambient member |
| `heading` | a Markdown document section |
| `unknown` | the grammar cannot tell the forms apart for this construct |

Roles come from explicit grammar captures, never from guessing whether a `{` follows. So a C++ header prototype and its implementation are distinguishable without string-matching a trailing `;`:

```
$ cx symbols --file include/ange/ecs.hpp

[6]{name,qualified,kind,role,signature}:
  EcsWorld,"ange::EcsWorld",class,definition,class EcsWorld
  EcsWorld,"ange::EcsWorld::EcsWorld",fn,declaration,EcsWorld();
  ange,ange,module,definition,namespace ange
  entity_count,"ange::EcsWorld::entity_count",fn,declaration,int entity_count() const;
  run,"ange::EcsWorld::run",fn,declaration,void run();
  validate_param,"ange::validate_param",fn,declaration,"void validate_param(const std::string& name, int value);"
```

`cx definition` sorts implementations ahead of signature-only sites, so the first result is the body. Use `--role declaration` when you specifically want the prototype, or `--role definition` to exclude prototypes entirely.

### Symbols -- search across the project

```
$ cx symbols --kind fn

[15]{file,name,qualified,kind,role,signature}:
  src/output.rs,print_toon,print_toon,fn,definition,"pub fn print_toon<T: Serialize>(value: &T)"
  src/query.rs,symbols,symbols,fn,definition,"pub fn symbols(...) -> i32"
  src/query.rs,definition,definition,fn,definition,"pub fn definition(...) -> i32"
  ...
```

Filters: `--kind`, `--role`, `--scope` (glob on the qualified name), `--name` (glob), `--file`

### Qualified names -- telling same-named symbols apart

Every symbol carries a `qualified` name built from its lexical scope, so twelve `run` symbols stay twelve distinct things:

```
$ cx symbols --name run

[12]{file,name,qualified,kind,role,signature}:
  generated/gen_api.cpp,run,gen::run,fn,definition,void run()
  include/ange/ecs.hpp,run,ange::EcsWorld::run,fn,declaration,void run();
  src/app.ts,run,Tickable.run,fn,declaration,"run(): number"
  src/lib.rs,run,alpha::run,fn,definition,pub fn run() -> u32
  src/lib.rs,run,beta::run,fn,definition,pub fn run() -> u32
  ...
```

Narrow to one scope with `--scope`:

```bash
cx definition --name run --scope 'beta::Runner::*'
cx symbols --name run --scope 'alpha::*'
```

Scopes are modelled for Rust, C/C++ and TypeScript (`::` for the first two, `.` for TypeScript). For other languages the `qualified` column is **empty**, which means *unresolved* -- not *top level*. `--scope` never matches an unresolved symbol, so it cannot produce a false positive.

Qualified names come from two syntactic facts: enclosing scope nodes, and qualifiers written at the definition site (`void EcsWorld::run()`). No import or type resolution is involved, so `cx` does not claim to know which `run` a call refers to.

Public/exported symbols are identifiable from their signatures (e.g. `pub fn` in Rust, `export function` in TypeScript).

### Definition -- get a function body without reading the file

```
$ cx definition --name resolve_root

file: src/main.rs
line: 149
role: definition
---
fn resolve_root(explicit: &Option<PathBuf>, path_hint: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return util::path::canonical(p);
    }
    if let Some(hint) = path_hint {
        return util::path::canonical(&util::git::find_project_root(&util::path::canonical(hint)));
    }
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    util::path::canonical(&util::git::find_project_root(&util::path::canonical(&cwd)))
}
```

Use `--from src/foo.rs` to disambiguate when multiple files define the same name. `--kind fn` filters by symbol kind and `--role definition` skips signature-only sites. `--max-lines` (default 200) truncates large bodies.

### References -- find all usages of a symbol

```
$ cx references --name Symbol

[5]{file,lines,refs,callers}:
  src/index.rs,"23, 69, 175, 628, 635",5,"FileData, Symbol, load_entries"
  src/language/extract.rs,"1, 83, 130, 231",4,"deduplicate, extract_symbols"
  src/language/mod.rs,"4, 325",2,parse_and_extract
  src/query.rs,"38, 125, 480, 676",4,"SymbolRow, definition, dir_overview"
```

References are grouped by file by default for change planning. Use `--context` when you need the exact source line for each hit:

```
$ cx references --name Symbol --context

[17]{file,line,caller,context}:
  src/index.rs,23,,FileData,"pub symbols: Vec<Symbol>,"
  src/index.rs,69,Symbol,"pub struct Symbol {"
  src/language/mod.rs,4,,"use crate::index::{Symbol, SymbolKind};"
  src/query.rs,38,SymbolRow,"symbol: &'a Symbol,"
  ...
```

The `caller` column shows which function or type encloses the reference.

Use `--file src/index.rs` to scope the search to a single file. Includes both definition and usage sites. Duplicate references on the same line are collapsed.

References are computed on-the-fly via AST walking (not indexed), so results are always fresh.

### Map -- bounded repository orientation

`cx overview` shows one directory. `cx map` shows the whole repository as subsystems, with resolved import edges and a stated ranking:

```
$ cx map --depth 2

[8]{subsystem,class,files,symbols,tests,depends_on,dependents,external_imports,api}:
  src/language/,production,21,209,0,src/,1,26,"FileParse, Heading, LANGUAGES, LangError, ..."
  src/,mixed,7,196,0,"src/language/, src/util/",1,32,"CacheAction, Cli, Commands, ..."
  src/util/,production,4,33,0,"",1,8,"absolute_normalize, canonical, find_project_root, ..."
  docs/,docs,7,93,0,"",0,0,""
cx: 6 files excluded as test (use --tests)
cx: ranked by dependents desc, then symbols desc, then name
```

- **Ranked by fan-in** (`dependents`), then symbol count. The basis is always printed, so the ordering is never something you have to infer.
- **Vendor, generated and test paths are excluded by default**, and every exclusion is reported with the flag that restores it (`--include-vendor`, `--include-generated`, `--tests`). `--exclude <glob>` is repeatable.
- **Edges are only claimed when an import resolves to an indexed file.** Anything else is counted as `external_imports`, and an import matching several files is reported as unresolved rather than pointed at an arbitrary one. Import edges are modelled for C/C++ `#include`, Rust `use crate::…`, and TypeScript relative imports.
- **`api` samples suppress low-information names** (`run`, `get`, `new`, `name`, …) so the column says something about the subsystem.
- `--depth N` controls how deep the subsystem grouping goes; results paginate like every other command.

`overview` is unchanged and remains the cheapest entry point.

### Callers and callees -- direct edges with stated evidence

```
$ cx callers --name run

[6]{from,to,evidence,resolution,file,line,ambiguous_candidates}:
  run_both,"alpha::run",call,lexical_scope,src/lib.rs,18,""
  run_both,"beta::run",call,lexical_scope,src/lib.rs,18,""
  run_all,"",call,syntax,src/scope_b.cpp,13,"alpha::run, ange::EcsWorld::run, beta::Runner::run, ..."
  "thirdparty::helper","thirdparty::run",call,lexical_scope,vendor/thirdparty/blob.cpp,5,""
cx: 2 of 6 call sites are syntax evidence only; cx does not resolve types
```

Every edge says how it was established:

| `resolution` | What justifies it |
| --- | --- |
| `syntax` | the AST puts this identifier in a callee position -- the target is not resolved |
| `lexical_scope` | a qualifier written at the call site matched one candidate, or exactly one candidate is visible by lexical nesting |
| `import_resolved` | the calling file imports the file defining exactly one candidate |

**cx never guesses a target.** When candidates cannot be narrowed to one, `to` is empty and `ambiguous_candidates` lists them all -- so `runner.run()` is reported as an unresolved call rather than being bound to whichever `run` happened to sort first. Candidates are also restricted to the caller's language, so a C++ call never points at a TypeScript method.

`cx callees --name X` lists calls written inside X's body. If several symbols share the name X it returns no rows and names the candidates, because reading one arbitrary body would answer a different question -- narrow it with `--scope`.

There is no `--depth`: cx does not do multi-hop traversal, and does no type resolution, so overload resolution, virtual dispatch and template instantiation are out of scope by design.

`cx references` rows also carry `evidence` (`definition`, `declaration`, `call`, `type_reference`, `import`, `identifier_reference`) and `resolution: syntax`.

### Freshness -- proving the index matches your edits

Every result reports which index generation answered it and how that was checked:

```json
"freshness": {
  "generation": 8,
  "mode": "metadata",
  "files_checked": 48,
  "files_updated": 0,
  "files_removed": 0,
  "files_skipped_missing_grammar": 0
}
```

Three modes, in increasing strength:

| Mode | How | Cost |
| --- | --- | --- |
| `metadata` (default) | size + high-resolution mtime, no file reads | baseline |
| `verified` (`--fresh verified`) | content hash of every indexable file | ~+25% on a 31 MB / 2,000-file tree |
| `paths` (`cx refresh <paths>`) | content hash of just the named files | one file: ~40 ms |

`metadata` has one real blind spot: an edit that preserves **both** size and mtime. cx does not pretend otherwise -- such a query still reports `mode: metadata` and `files_updated: 0`. Use `--fresh verified` or name the files:

```bash
# after editing, name what changed — hashed immediately, regardless of clock granularity
cx refresh src/a.cpp src/b.h

[2]{file,status}:
  src/a.cpp,updated
  src/b.h,unchanged
cx: generation 9 | mode paths | checked 2 | updated 1 | removed 0
```

The recommended agent loop is **edit -> `cx refresh <changed paths>` -> query**. Because `refresh` reports the generation it established, and later queries report the generation that answered them, a matching pair is mechanical proof your edit is included. `cx refresh` with no arguments verifies the whole project by content hash.

### Pagination

Commands have default result limits to keep output bounded: definition shows 3, symbols 100, references 50. When results are truncated, cx prints a hint:

```
cx: 3/32 definitions for "OnTypeModel" | --from PATH to narrow | --offset 3 for more | --all
```

Use `--offset N` to page forward, `--all` to bypass the limit, or `--limit N` to override the default. Narrowing with `--from` / `--file` / `--kind` / `--role` is usually better than paging.

With `--json`, pagination facts live in `page` and the exact follow-up commands live in `next_queries` (see below). The stderr hint is only printed for TOON output.

## JSON output contract

`--json` always returns one object with the same keys, for every command and every result count -- empty, complete, truncated, or failed:

```json
{
  "schema_version": 1,
  "query": { "kind": "symbols", "subject": "run" },
  "freshness": { "generation": 8, "mode": "metadata", "files_checked": 48,
                 "files_updated": 0, "files_removed": 0,
                 "files_skipped_missing_grammar": 0 },
  "page": { "total": 12, "offset": 0, "limit": 4, "truncated": true },
  "results": [ ... ],
  "warnings": [],
  "next_queries": [
    "cx --json symbols --name run --limit 4 --offset 4",
    "cx --json symbols --name run --all"
  ],
  "error": null
}
```

- `results`, `warnings` and `next_queries` are always arrays; `error` is always present (`null` on success). Nothing appears or disappears based on result count, so a consumer never branches on key existence.
- `next_queries` entries are literally runnable -- they are the current invocation with pagination flags rewritten.
- `warnings` reports facts an agent should not ignore, such as several candidates sharing one symbol name.
- A successful query with **no** results is `results: []` with `error: null` and exit code 0. That is different from a failure, which sets `error` and exits 1:

```json
{
  "error": { "code": "file_not_indexed", "message": "file not in index: src/nope.cpp" }
}
```

Error codes: `file_not_indexed`, `unsupported_file_type`, `no_indexed_files`, `grammar_not_installed`.

Exit codes: `0` success (including zero results), `1` query failure, `2` usage error from argument parsing.

Under `--json` the payload is authoritative and cx does not duplicate messages on stderr. Without `--json`, the familiar `cx: ...` notes and pagination hints on stderr are unchanged.

## How it works

On first invocation, cx builds an index by parsing all source files with tree-sitter. The index stores symbols, signatures, and byte ranges for every file; overview derives line ranges from those byte ranges. Subsequent invocations incrementally update only changed files.

Language grammars are downloaded on demand as shared libraries via [tree-sitter-language-pack](https://github.com/kreuzberg-dev/tree-sitter-language-pack). Install the ones you need:

```bash
cx lang add rust typescript python
cx lang list        # see what's installed
cx lang remove lua  # remove one
```

If you run cx without installing grammars first, it will tell you which ones are needed:

```
cx: no language grammars installed

Detected languages in this project:
  rust (42 files)
  typescript (18 files)

Install with: cx lang add rust typescript
```

**Supported languages:** Run `cx lang list` to see all supported languages and their install status.

**Index location:** `~/.cache/cx/indexes/` (one db per project, keyed by path hash). Run `cx cache path` to see the exact location, `cx cache clean` to delete it. Override with `CX_CACHE_DIR`.

**Project root detection:** walks up from cwd looking for `.git`. Override with `--root /path/to/project`.

**File filtering:** cx respects your `.gitignore`. To exclude additional directories from indexing, drop an empty `.cx-ignore` file inside them.

**Sandboxed environments (Codex, Claude Code, etc.):** cx writes to `~/.cache/cx` by default. If your sandbox restricts writes outside the workspace, either add `~/.cache/cx` to the sandbox's writable paths, or set `CX_CACHE_DIR` to a writable location (e.g. `CX_CACHE_DIR=/tmp/cx-cache`).

## Output format

Overview, symbols, and references use [TOON](https://toonformat.dev) -- a token-efficient structured format. Definition uses a plain-text format (metadata header + raw code body) for readability. Use `--json` for the versioned JSON envelope on any command (see [JSON output contract](#json-output-contract)).

## Adding a language

cx uses tree-sitter grammars loaded dynamically via `tree-sitter-language-pack`. To add support for a new language:

1. In `src/language/mod.rs`, add:
   - A query constant with tree-sitter patterns for the language's symbols
   - A `LanguageConfig` entry in the `LANGUAGES` array
2. Add tests

The grammar itself is downloaded at runtime — no build dependency needed. Here's a minimal example — adding Swift support:

```rust
const SWIFT_QUERY: &str = r#"
(function_declaration
  name: (simple_identifier) @name) @definition.function

(class_declaration
  name: (type_identifier) @name) @definition.class

(protocol_declaration
  name: (type_identifier) @name) @definition.interface
"#;

LanguageConfig {
    name: "swift",
    extensions: &["swift"],
    grammar_override: &[],
    download_names: &[],  // empty = download name matches config name
    query: SWIFT_QUERY,
    sig_body_child: None,
    sig_delimiter: Some(b'{'),
    kind_overrides: &[],
    ref_node_types: &["simple_identifier", "type_identifier"],
},
```

**Writing queries:** Use `tree-sitter parse` or inspect `node-types.json` in the grammar to discover the AST structure. Capture `@name` for the symbol name and `@definition.<kind>` for the enclosing node. Supported kinds: `function`, `method`, `class`, `interface`, `type`, `enum`, `module`, `constant`, `event`.

**Kind overrides:** When a language maps generic capture names to specific concepts (e.g., Rust's `definition.class` → `SymbolKind::Struct`), add entries to `kind_overrides`. These are checked before the default mapping.

**Grammar names:** The `name` field must match the name used by `tree-sitter-language-pack` (check their [language list](https://github.com/kreuzberg-dev/tree-sitter-language-pack)). If the download name differs from the config name, use `download_names` (e.g., `typescript` also downloads `tsx`).
