#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="aarch64-apple-darwin"
BINARY="${PI_CX_BINARY:-$ROOT/target/$TARGET/release/cx}"
CACHE="${CX_CACHE_DIR:-$HOME/Library/Caches/cx}"
GRAMMARS="$CACHE/grammars"
OUT="${PI_CX_OUT_DIR:-$ROOT/dist}"
VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")"
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/pi-cx-asset.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT

[[ "$(uname -s)-$(uname -m)" == "Darwin-arm64" ]] || { echo "pi-cx asset build requires darwin-arm64" >&2; exit 1; }
[[ -x "$BINARY" ]] || { echo "missing release binary: $BINARY" >&2; exit 1; }
[[ "$($BINARY --version)" == *"$VERSION"* ]] || { echo "binary/Cargo version mismatch" >&2; exit 1; }

missing=0
for name in rust typescript tsx python go c cpp; do [[ -f "$GRAMMARS/libtree_sitter_${name}.dylib" ]] || missing=1; done
if [[ "$missing" == 1 ]]; then "$BINARY" lang add rust typescript python go c cpp; fi
mkdir -p "$STAGE/bin" "$STAGE/grammars" "$OUT"
cp "$BINARY" "$STAGE/bin/cx"
chmod 755 "$STAGE/bin/cx"
for name in rust typescript tsx python go c cpp; do cp "$GRAMMARS/libtree_sitter_${name}.dylib" "$STAGE/grammars/"; done
node "$ROOT/scripts/make-pi-cx-manifest.mjs" "$STAGE" "$VERSION"
ARCHIVE="$OUT/pi-cx-aarch64-apple-darwin.tar.gz"
tar -C "$STAGE" -czf "$ARCHIVE" manifest.json bin grammars
shasum -a 256 "$ARCHIVE" | awk -v n="$(basename "$ARCHIVE")" '{print $1 "  " n}' > "$ARCHIVE.sha256"
echo "$ARCHIVE"
