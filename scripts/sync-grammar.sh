#!/usr/bin/env bash
# Unix からも Windows と同じ同期処理を使う。
set -euo pipefail
exec node "$(dirname "$0")/sync-grammar.mjs" "$@"
