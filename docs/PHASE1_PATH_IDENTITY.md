# Phase 1 — one canonical path identity

> Roadmap reference: `docs/AGENT_NATIVE_DEVELOPMENT_ROADMAP.md` §4.1, §11 Phase 1, §13.
> Behavior change: none in output format; previously-failing path spellings now work.

## Problem

Four places derived a path identity independently:

| Site | Before |
| --- | --- |
| `main.rs::resolve_root` | `absolute_normalize` (lexical only) |
| `index.rs::cache_path_for` | `fs::canonicalize`, falling back to the raw path |
| `Index.root` | whatever string the caller passed |
| `query.rs::make_relative` | `absolute_normalize` on both sides |

So `--root /tmp/p` plus an argument spelled `/private/tmp/p/src/a.rs` hashed to
one index but failed to strip the root prefix:

```text
$ cx --root /tmp/cxalias overview /private/tmp/cxalias/src/a.rs
cx: file not in index: /private/tmp/cxalias/src/a.rs      # exit 1
```

The same failure appears for any symlinked checkout, and macOS temp dirs
(`/var` → `/private/var`) hit it constantly.

## Change

`util::path::canonical()` is now the single identity function: absolute,
lexically normalized, symlinks resolved, Windows `\\?\` verbatim prefix stripped
and drive letter upper-cased. Unlike `fs::canonicalize` it never fails — for a
path that does not exist yet it canonicalizes the deepest existing ancestor and
re-appends the remaining components, which keeps "file not in index" messages
root-relative instead of leaking absolute paths.

All four sites now route through it:

- `resolve_root` canonicalizes the explicit `--root`, the path-argument hint, and
  the cwd fallback (before *and* after the `.git` walk).
- `cache_path_for` hashes `canonical(root)`.
- `Index::load_or_build` stores the canonical root, so every relative key in the
  index is derived from the same base.
- `make_relative` canonicalizes both the argument and the root.

## Result

```text
$ cx --root /tmp/cxalias --json overview /private/tmp/cxalias/src/a.rs   # exit 0
$ cx --root /private/tmp/cxalias --json overview /tmp/cxalias/src/a.rs   # exit 0
$ cx --root /tmp/cxalias cache path
/Users/…/Library/Caches/cx/indexes/805a3db0f6f4b667.db
$ cx --root /private/tmp/cxalias cache path
/Users/…/Library/Caches/cx/indexes/805a3db0f6f4b667.db
```

## Tests

`tests/path_identity.rs` (7 tests, symlink-based so it is not macOS-specific):

- aliased roots hash to one cache file
- an index built under the alias is **reused** under the real root — asserted by
  the absence of `indexing`/`updating` on stderr, plus byte-identical stdout
- absolute path arguments resolve in either spelling, both directions
- `--file` and `--from` filters accept either spelling
- result rows always report root-relative paths (`src/a.rs`)
- a missing file reports a root-relative path, not an absolute one
- `.` and `src/..` root spellings share one index

`src/util/path.rs` unit tests cover idempotence, symlink resolution, missing tail
components, and `.`/`..` folding against the filesystem.

Two integration tests (`overview_absolute_path_resolves_foreign_project` and
friends) were red mid-change and pass again: canonicalizing the root without
canonicalizing the argument is exactly the bug this phase removes.

## Known limitation

Case-only aliases on case-insensitive filesystems (`/tmp/Project` vs
`/tmp/project` on macOS/Windows) are *not* unified: `realpath` does not correct
case, so the two spellings still hash differently. The Windows drive letter is
normalized; general case folding is deliberately not attempted, since folding on
a case-sensitive volume would merge genuinely different directories.

## Benchmark

`./scripts/bench.sh . 5`, same host as the Phase 0 baseline — no regression:

| Metric | Phase 0 | Phase 1 |
| --- | --- | --- |
| cold build | 0.08 s / 33.7 MiB | 0.09 s / 33.7 MiB |
| index size | 1,056,768 B | 1,056,768 B |
| root overview | 32 ms | 36 ms |
| file overview | 32 ms | 31 ms |
| definition | 31 ms | 32 ms |
| references | 57 ms | 59 ms |
| symbol search | 31 ms | 31 ms |
| one-file refresh | 0.04 s | 0.04 s |

Differences are within run-to-run noise; `canonical()` adds a handful of
`realpath` syscalls per command, not per file.
