#!/bin/sh
set -eu
ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
mkdir -p "$ROOT/logs" "$ROOT/.cache/tmp"
LOG="$ROOT/logs/run_$(date -u '+%Y-%m-%d_%H-%M-%S')_UTC_$$.log"
log() { printf '{"timestamp":"%s","level":"%s","category":"LAUNCHER","message":"%s"}\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$1" "$2" >> "$LOG"; }
log INFO 'Native launcher started'
BINARY="$ROOT/bin/fiverr-agent-mcp"
if [ ! -x "$BINARY" ]; then log ERROR 'Native executable missing'; printf '%s\n' 'Native executable missing; download a verified package. No local compilation performed.' >&2; exit 1; fi
export TEMP="$ROOT/.cache/tmp" TMP="$ROOT/.cache/tmp" TMPDIR="$ROOT/.cache/tmp" XDG_CACHE_HOME="$ROOT/.cache"
log DEBUG 'Rooted child temporary paths configured'
code=0
"$BINARY" "$@" || code=$?
if [ "$code" -eq 0 ]; then log INFO "Native process exited with code $code"; else log ERROR "Native process exited with code $code"; fi
exit "$code"
