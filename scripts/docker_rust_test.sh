#!/bin/sh
set -eu

SCRIPT_PATH=$(readlink -f -- "$0" 2>/dev/null || printf "%s" "$0")
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")/.." && pwd)
PAYLOAD_DIR_DEFAULT=/opt/claude-proxy/tests/payloads

set -- docker run --rm -v "$ROOT_DIR:/work" -w /work

if [ -d "${ANTHMORPH_CLAUDE_PAYLOAD_DIR:-$PAYLOAD_DIR_DEFAULT}" ]; then
  PAYLOAD_DIR_HOST=${ANTHMORPH_CLAUDE_PAYLOAD_DIR:-$PAYLOAD_DIR_DEFAULT}
  set -- "$@" -v "$PAYLOAD_DIR_HOST:$PAYLOAD_DIR_HOST:ro"
  ANTHMORPH_CLAUDE_PAYLOAD_DIR=$PAYLOAD_DIR_HOST
fi

for env_name in \
  CHUTES_API_KEY \
  CHUTES_BASE_URL \
  CHUTES_MODEL \
  MINIMAX_API_KEY \
  MINIMAX_BASE_URL \
  MINIMAX_MODEL \
  ALIBABA_CODE_API_KEY \
  ALIBABA_BASE_URL \
  ALIBABA_MODEL \
  ANTHMORPH_CLAUDE_PAYLOAD_DIR
do
  eval "env_value=\${$env_name-}"
  if [ -n "${env_value:-}" ]; then
    set -- "$@" -e "$env_name=$env_value"
  fi
done

set -- "$@" rust:1.89-bookworm sh -lc 'export PATH=/usr/local/cargo/bin:$PATH; cargo test -- --nocapture'
"$@"
