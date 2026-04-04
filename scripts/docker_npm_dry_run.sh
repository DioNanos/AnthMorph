#!/bin/sh
set -eu

SCRIPT_PATH=$(readlink -f -- "$0" 2>/dev/null || printf "%s" "$0")
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")/.." && pwd)
MODE=${1:-pack}

case "$MODE" in
  pack)
    CMD='npm pack --dry-run'
    ;;
  publish)
    CMD='npm publish --dry-run'
    ;;
  *)
    echo "usage: $0 [pack|publish]" >&2
    exit 1
    ;;
esac

docker run --rm \
  -v "$ROOT_DIR:/work" \
  -w /work \
  node:22-bookworm \
  sh -lc "$CMD"
