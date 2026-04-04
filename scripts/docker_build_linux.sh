#!/bin/sh
set -eu

SCRIPT_PATH=$(readlink -f -- "$0" 2>/dev/null || printf "%s" "$0")
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")/.." && pwd)

docker run --rm \
  -v "$ROOT_DIR:/work" \
  -w /work \
  rust:1.89-bookworm \
  sh -lc 'export PATH=/usr/local/cargo/bin:$PATH; cargo build --release'
