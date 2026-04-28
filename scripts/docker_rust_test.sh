#!/bin/sh
set -eu

SCRIPT_PATH=$(readlink -f -- "$0" 2>/dev/null || printf "%s" "$0")
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")/.." && pwd)
PAYLOAD_DIR_DEFAULT=

set -- docker run --rm -v "$ROOT_DIR:/work" -w /work

if [ -n "${ANTHMORPH_PAYLOAD_DIR:-}" ] && [ -d "$ANTHMORPH_PAYLOAD_DIR" ]; then
  PAYLOAD_DIR_HOST=$ANTHMORPH_PAYLOAD_DIR
  set -- "$@" -v "$PAYLOAD_DIR_HOST:$PAYLOAD_DIR_HOST:ro"
  ANTHMORPH_PAYLOAD_DIR=$PAYLOAD_DIR_HOST
fi

for env_name in \
  ANTHMORPH_PROVIDER_API_KEY \
  ANTHMORPH_PROVIDER_BASE_URL \
  ANTHMORPH_PROVIDER_MODEL \
  ANTHMORPH_PAYLOAD_DIR
do
  eval "env_value=\${$env_name-}"
  if [ -n "${env_value:-}" ]; then
    set -- "$@" -e "$env_name=$env_value"
  fi
done

set -- "$@" rust:1.89-bookworm sh -lc 'export PATH=/usr/local/cargo/bin:$PATH; cargo test -- --nocapture'
"$@"
