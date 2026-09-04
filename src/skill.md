---
name: cx
description: Semantic code navigation via the cx CLI — symbol search, definitions, references, file/dir overviews. Use when you have a symbol name or want to orient in unfamiliar code; prefer over reading whole files.
---

# cx — Semantic Code Navigation

```
Usage: cx [OPTIONS] <COMMAND>

Commands:
  overview [OPTIONS] <PATH>             Table of contents — symbols + ranges + signatures for a file, or symbol names for a directory
  map [OPTIONS]                         Bounded repository map — subsystems, sizes, resolved import edges
  symbols [OPTIONS]                     Search symbols across project
  definition [OPTIONS] --name <NAME>    Get a function/type/... body without reading the whole file (default limit: 3)
  references [OPTIONS] --name <NAME>    Find all usages of a symbol across the project
  callers [OPTIONS] --name <NAME>       Direct call sites pointing at a symbol, with resolution level
  callees [OPTIONS] --name <NAME>       Direct calls written inside a symbol's body
  refresh [PATHS]...                    Re-index the named files now, by content hash
  lang [OPTIONS] <SUBCOMMAND>           Manage language grammars (sub-commands: add, remove, list, help)
  help [COMMAND]                        Full command/option list or help on the given subcommand(s)

Options:
      --root <ROOT>      Project root (defaults to git root)
      --json             Emit JSON instead of TOON
      --no-tests         Exclude test files and test symbols from results (`*/tests/* and `*.test.ts`, test_*.py`, ...)
      --fresh <MODE>     Index verification before answering: metadata (default) or verified
Pagination options:
      --limit <LIMIT>    Max number of results to return (overrides per-command default)
      --offset <N>       Skip the first N results (not counted against limit)
      --all              Return all results (bypass default limit)
```

Prefer cx over reading files. Zoom in until you have what you need, then stop:
`map` (whole repo) > `overview` > `symbols` > `definition` or `references`.

Fall back to the Read tool when `cx` can't represent the target (anonymous functions, JSX-inline components, dynamic
dispatch, string-keyed lookups, non-symbol regions).

## First-run checks (once per session)

1. **Is cx installed?** Run `command -v cx`. If absent, stop and ask the user to install it — do not attempt the install
   yourself. Canonical commands:
    - Homebrew: `brew tap ind-igo/cx && brew install cx`
    - Cargo: `cargo install cx-cli`
    - Shell (Linux/macOS): `curl -sL https://raw.githubusercontent.com/ind-igo/cx/master/install.sh | sh`

2. **Are this project's grammars installed?** Just run `cx overview .` as your first probe. If grammars are missing the
   output is self-diagnosing — it prints the detected project languages and the exact install command, e.g.:

   ```
   cx: no language grammars installed
   Detected languages in this project:
     typescript (37 files)
     markdown (7 files)
   Install with: cx lang add typescript markdown
   ```

   If your harness sandboxes network egress, the install will fail with "Connection refused" when `cx` fetches the
   grammars
   from GitHub. Run the install outside the sandbox, or grant network access for the install command. Once grammars are
   cached, normal cx queries don't need network. Re-run `cx overview .` to confirm.

## Common Recipes & Extra Info

```
cx map [--depth N] [--exclude GLOB]                  whole-repo orientation: subsystems, sizes, import edges
cx overview DIR --full                               `--full` also includes kind/range/signature for direct files
cx symbols [--kind K] [--name GLOB] [--file PATH]    search symbols project-wide
cx symbols --role declaration                        signature-only sites (C/C++ prototypes, trait/interface members)
cx symbols --scope 'alpha::*'                        filter by qualified name (glob)
cx symbols --kinds [--file PATH]                     list distinct kinds with counts
cx definition --name NAME [--from PATH] [--kind K]   get a definition (optionally filtered to specific kind, e.g. function body)
cx definition --name NAME --scope 'Class::*'         pick one scope when a name exists in several
cx references --name NAME [--file PATH] [--context]  find usages, `--context` will show the line where the use appears
cx callers --name NAME [--scope GLOB]                who calls it, with `resolution` per edge
cx callees --name NAME [--scope GLOB]                what it calls (needs --scope if the name is ambiguous)
cx refresh PATH...                                   re-index the named files now, by content hash
```

- `--file` and `--from` are identical and restrict the symbol to a precise file, but only if there is an exact match for
  the path resolved from cwd.
- Kinds: fn, struct, enum, trait, type, const, class, interface, module, event, field, heading
- Roles: definition (has a body/members), declaration (signature only), heading (Markdown section), unknown (grammar
  cannot tell the forms apart). Roles come from grammar captures, not from guessing at braces.
- `qualified` is the lexically qualified name (`ange::EcsWorld::run`, `Tickable.run`). Modelled for Rust, C/C++ and
  TypeScript; **empty means unresolved, not top-level**. `--scope` never matches an unresolved symbol.
- `.gitignore` is honored. Untracked-but-not-ignored files are still indexed.

## Key patterns

- New to a repository? Start with `cx map` (subsystems, ranked by how many other subsystems depend on them), then
  `cx overview <subsystem>` to drill in. `map` excludes vendor/generated/test paths by default and prints what it
  excluded; its `depends_on` edges only appear when an import resolves to an indexed file.
- Start with `cx overview .`, drill into subdirectories — cheaper than ls + reading files.
- **After you edit files, run `cx refresh <the paths you changed>` before querying them again.** Ordinary
  queries auto-detect changes by size+mtime, which misses an edit that preserves both; `cx refresh` hashes
  the named files so the update is guaranteed. Every result reports `freshness.generation`, and a query
  showing the same generation `refresh` reported is proof your edit is in the index.
- Use `--fresh verified` when you need a whole-project content check instead of naming paths.
- Can't find a symbol that should exist? Make sure the language grammar is installed via `cx lang list`.
- `cx definition --name X` gives exact text for Edit tool's `old_string` without reading the whole file.
- In C/C++ (and Rust traits, TypeScript interfaces) a name can have both a declaration and a definition. `cx definition`
  lists the implementation first; add `--role definition` to drop prototypes, or `--role declaration` to see only the
  header signature.
- When one short name exists in several scopes, read `warnings`: it reports how many **distinct** symbols share the name
  and lists their qualified names. Narrow with `--scope` (or `--from`) instead of assuming the first row is the one you
  want. A declaration plus its definition is one symbol, so it produces no such warning.
- `cx references --name X` groups hits by file; add `--context` only when exact source lines are needed.
- For call relationships use `cx callers` / `cx callees`, and **read the `resolution` column**: `syntax` means cx only
  knows the identifier sits in a callee position, so an empty `to` with `ambiguous_candidates` means "could be any of
  these", not "no caller". `lexical_scope` and `import_resolved` are resolved edges. cx does no type resolution, so
  method calls through a variable (`obj.run()`) usually stay `syntax`. There is no multi-hop traversal.
- When re-entering an unfamiliar area or picking up a topic after a gap, use `cx overview` / `cx definition` to
  re-orient — don't re-read full files
- Check signatures for `pub`/`export` to identify public API without reading the file.
- If your harness restricts writes outside the project root, the default cache location won't be writable.
  Set `$CX_CACHE_DIR` to a project-local path, or grant write permission to the default cache dir.

## Pagination

Default limits: overview: unlimited, definition 3, symbols 100, references 50.

When truncated, stderr shows: `cx: 3/32 definitions for "X"`. Use `--file` (or `--from`), `--kind` and `--role` to narrow, or use
`--offset` to get further pages, or `--all` to get all results.

## JSON contract

`--json` always returns one object with a fixed key set, regardless of command or result count:

```json
{
  "schema_version": 1,
  "query": { "kind": "symbols", "subject": "run" },
  "page": { "total": 12, "offset": 0, "limit": 4, "truncated": true },
  "results": [],
  "warnings": [],
  "next_queries": ["cx --json symbols --name run --limit 4 --offset 4"],
  "error": null
}
```

- Zero results is `results: []` with `error: null` and exit 0 — not a failure.
- A failure sets `error.code` (`file_not_indexed`, `unsupported_file_type`, `no_indexed_files`,
  `grammar_not_installed`) and exits 1. Exit 2 means bad arguments.
- `next_queries` entries are runnable as-is; prefer them over composing your own `--offset`.
- `warnings` flags things like several candidates sharing one name — read it before assuming the first
  result is the only one.
- Under `--json`, stderr stays quiet; the payload is authoritative.
- `freshness` reports the index `generation` that answered, the `mode` used (`metadata`, `verified`, or
  `paths`), and how many files were checked/updated/removed. A stale answer is labelled, never disguised.
