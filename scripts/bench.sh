#!/usr/bin/env bash
# Reproducible cx benchmark (roadmap §10.3).
#
# Records correctness-independent performance facts for one project:
#   cold index wall/user/sys, peak RSS, index bytes,
#   warm query median/p95, output bytes, incremental refresh time.
#
# Usage:
#   scripts/bench.sh [project-dir] [runs]
#
# Notes:
#   - Uses a throwaway CX_CACHE_DIR so grammar downloads are reused but the
#     index is always built cold when measuring cold time.
#   - Performance numbers are hardware-dependent and must NOT be used as CI
#     gates; only correctness fixtures are pinned in CI.
set -euo pipefail

PROJECT="${1:-$PWD}"
RUNS="${2:-9}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CX_BIN="${CX_BIN:-$REPO_ROOT/target/release/cx}"

if [[ ! -x "$CX_BIN" ]]; then
    echo "bench: building release binary" >&2
    (cd "$REPO_ROOT" && cargo build --release >/dev/null)
fi

PROJECT="$(cd "$PROJECT" && pwd)"
BENCH_CACHE="$(mktemp -d "${TMPDIR:-/tmp}/cx-bench.XXXXXX")"
# Reuse already-downloaded grammars so the run measures indexing, not network.
# Host cache location matches src/lang.rs::cx_cache_dir (dirs::cache_dir).
if [[ -n "${CX_CACHE_DIR:-}" ]]; then
    HOST_CACHE="$CX_CACHE_DIR"
elif [[ "$(uname -s)" == "Darwin" ]]; then
    HOST_CACHE="$HOME/Library/Caches/cx"
else
    HOST_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/cx"
fi
if [[ -d "$HOST_CACHE/grammars" ]]; then
    mkdir -p "$BENCH_CACHE"
    ln -s "$HOST_CACHE/grammars" "$BENCH_CACHE/grammars"
    [[ -f "$HOST_CACHE/manifest.json" ]] && cp "$HOST_CACHE/manifest.json" "$BENCH_CACHE/manifest.json"
else
    echo "bench: no grammars in $HOST_CACHE — cold build will include downloads" >&2
fi
trap 'rm -rf "$BENCH_CACHE"' EXIT
export CX_CACHE_DIR="$BENCH_CACHE"

# Path arguments are resolved relative to the process cwd, so every query runs
# with the project as cwd.  Without this, benchmarking a project other than the
# current directory makes `overview .` point outside the root, the query exits 1,
# and `set -e` aborts the whole warm section silently.
cx() { (cd "$PROJECT" && "$CX_BIN" --root "$PROJECT" "$@"); }

drop_index() { rm -rf "$BENCH_CACHE/indexes"; }

# Peak RSS in MiB plus wall/user/sys, portable across macOS and GNU time.
timed() {
    local label="$1"
    shift
    local err rc
    err="$(mktemp)"
    if /usr/bin/time -l "$@" >/dev/null 2>"$err"; then rc=0; else rc=$?; fi
    local wall user sys rss
    wall="$(awk '/real/ {print $1}' "$err" | tail -1)"
    user="$(awk '/real/ {print $3}' "$err" | tail -1)"
    sys="$(awk '/real/ {print $5}' "$err" | tail -1)"
    rss="$(awk '/maximum resident set size/ {print $1}' "$err" | tail -1)"
    if [[ -z "$rss" ]]; then
        rss="$(awk '/Maximum resident set size/ {print $NF * 1024}' "$err" | tail -1)"
    fi
    rm -f "$err"
    printf '%s\twall=%ss\tuser=%ss\tsys=%ss\tpeak_rss=%.1fMiB\n' \
        "$label" "${wall:-?}" "${user:-?}" "${sys:-?}" "$(awk -v r="${rss:-0}" 'BEGIN{print r/1048576}')"
    return $rc
}

# Median and p95 of N warm runs, in milliseconds, plus stdout bytes.
warm() {
    local label="$1"
    shift
    local times=() bytes=0 t0 t1
    for _ in $(seq "$RUNS"); do
        t0=$(python3 -c 'import time;print(time.perf_counter_ns())')
        bytes=$(cx "$@" 2>/dev/null | wc -c | tr -d ' ')
        t1=$(python3 -c 'import time;print(time.perf_counter_ns())')
        times+=("$(((t1 - t0) / 1000000))")
    done
    local sorted median p95
    sorted=$(printf '%s\n' "${times[@]}" | sort -n)
    median=$(printf '%s\n' "$sorted" | awk '{a[NR]=$1} END{print a[int((NR+1)/2)]}')
    p95=$(printf '%s\n' "$sorted" | awk '{a[NR]=$1} END{print a[int(NR*0.95+0.5)==0?1:int(NR*0.95+0.5)]}')
    printf '%s\tmedian=%sms\tp95=%sms\toutput_bytes=%s\n' "$label" "$median" "$p95" "$bytes"
}

echo "cx benchmark"
echo "  binary:  $CX_BIN ($("$CX_BIN" --version))"
echo "  project: $PROJECT"
echo "  runs:    $RUNS warm iterations per query"
echo

echo "== cold index =="
drop_index
timed "cold-build" "$CX_BIN" --root "$PROJECT" symbols --limit 1
index_bytes=$(find "$BENCH_CACHE/indexes" -name '*.db' -exec stat -f%z {} \; 2>/dev/null ||
    find "$BENCH_CACHE/indexes" -name '*.db' -exec stat -c%s {} \; 2>/dev/null)
printf 'index-bytes\t%s\n\n' "${index_bytes:-?}"

# cx --json returns a bare array when nothing is truncated and an envelope
# object otherwise (roadmap §3.4) — handle both until Phase 3 lands.
first_field() {
    python3 -c 'import json, sys
field, fallback = sys.argv[1], sys.argv[2]
try:
    doc = json.load(sys.stdin)
    rows = doc["results"] if isinstance(doc, dict) else doc
    print(rows[0][field])
except Exception:
    print(fallback)' "$1" "$2"
}

echo "== warm queries =="
warm "root-overview      " overview .
TARGET_FILE="$(cx --json symbols --limit 1 2>/dev/null | first_field file .)"
warm "file-overview      " overview "$TARGET_FILE"
SUBJECT="$(cx --json symbols --kind fn --limit 1 2>/dev/null | first_field name main)"
warm "definition ($SUBJECT)" definition --name "$SUBJECT"
warm "references ($SUBJECT)" references --name "$SUBJECT"
warm "symbol-search      " symbols --name '*init*'
# map and the relation queries are the corpus-scale commands: map resolves every
# import in the project and the relation queries parse every file that mentions
# the subject.  Benchmarking them only on a small repo hid a 3.4 s regression on
# a 3,905-file corpus, so they are measured here explicitly.
warm "map-depth-1        " map --depth 1
warm "map-depth-2        " map --depth 2
warm "callers ($SUBJECT)" callers --name "$SUBJECT"
warm "callees ($SUBJECT)" callees --name "$SUBJECT"
echo

echo "== incremental refresh =="
TARGET="$TARGET_FILE"
if [[ -n "$TARGET" && "$TARGET" != "." && -f "$PROJECT/$TARGET" ]]; then
    printf '\n' >>"$PROJECT/$TARGET"
    timed "one-file-refresh" /bin/sh -c 'cd "$1" && shift && exec "$@"' _ "$PROJECT" "$CX_BIN" --root "$PROJECT" overview "$TARGET"
    # Restore the file byte-for-byte.
    python3 - "$PROJECT/$TARGET" <<'PY'
import sys
p = sys.argv[1]
with open(p, 'rb') as f:
    data = f.read()
if data.endswith(b'\n\n'):
    with open(p, 'wb') as f:
        f.write(data[:-1])
PY
else
    echo "one-file-refresh	skipped (no indexed file)"
fi
