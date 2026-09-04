# pi-cx

`pi-cx` is the Pi extension shipped by the cx repository. It starts the bundled, verified cx binary once per tool call; it does not run a daemon, emulate an LSP lifecycle, replace Pi's `read`/`grep` tools, or fall back to a `cx` on `PATH`.

## Install

```bash
pi install git:github.com/AllenDang/cx@v0.7.3
```

The package supports macOS, Linux, and Windows on arm64 and x86_64. Installation selects the asset matching the current OS and architecture, verifies the archive and every manifest file, then installs it under a platform-specific `vendor/pi-cx/<platform>-<arch>` directory. Installation runs native code verification and requires network access. Review package source before installation.

## Tools

| Tool | Purpose |
|---|---|
| `cx_overview` | One directory level or a file outline |
| `cx_symbols` | Typed symbol discovery |
| `cx_definition` | One symbol body |
| `cx_references` | Syntax-classified occurrences |
| `cx_callers` | One-hop caller evidence |
| `cx_callees` | One-hop callee evidence |
| `cx_map` | Bounded repository map |
| `cx_refresh` | Explicit verified refresh after edits |

Every query is rooted at the current Pi session's canonical `ctx.cwd`; tools do not accept a root argument. Path and symlink escapes are rejected. Results preserve cx's schema-v1 JSON envelope. cx evidence is syntax-oriented and does not claim compiler-level semantic resolution.

## Cache and grammars

pi-cx shares cx's standard cache (`~/Library/Caches/cx` on macOS), including indexes and grammars. Seven bundled Tree-sitter dylibs cover Rust, C, C++, JavaScript, JSX, TypeScript, TSX, Python, and Go. Before the first query, missing or mismatched bundled grammars are repaired offline under a cache lock using digest checks and atomic renames. Other grammars are downloaded only after confirmation in TUI/RPC mode; print/JSON mode returns a structured error.

## Diagnostics and troubleshooting

Run `/cx-status` for a read-only report. It does not seed grammars, download files, or index the current project.

- **unsupported platform**: use macOS, Linux, or Windows on arm64/x86_64; another architecture is not selected automatically.
- **checksum or binary version failure**: remove and reinstall the exact tagged package. PATH fallback is deliberately disabled.
- **schema mismatch**: install the package tag matching the bundled cx release.
- **grammar missing**: confirm `cx lang add <language>` in an interactive session, or run the reported bundled-binary command yourself.
- **cache permission**: make the standard cx cache writable; pi-cx does not switch to a private cache.

Tool stdout is capped at Pi's 50KB/2000-line boundary. Oversized cx JSON is saved to a mode-`0600` temporary file and replaced with valid truncation metadata plus a pagination suggestion.
