#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${1:-${PI_CX_TARGET:-aarch64-apple-darwin}}"
BINARY="${2:-${PI_CX_BINARY:-$ROOT/target/$TARGET/release/cx}}"
if [[ "$TARGET" == *windows* && ! -f "$BINARY" && -f "$BINARY.exe" ]]; then BINARY="$BINARY.exe"; fi
exec node "$ROOT/scripts/build-pi-cx-asset.mjs" "$TARGET" "$BINARY" "${PI_CX_OUT_DIR:-$ROOT/dist}"
