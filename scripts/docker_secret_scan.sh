#!/bin/sh
set -eu

SCRIPT_PATH=$(readlink -f -- "$0" 2>/dev/null || printf "%s" "$0")
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")/.." && pwd)

docker run --rm \
  -v "$ROOT_DIR:/repo" \
  -w /repo \
  zricethezav/gitleaks:latest \
  detect --source . --no-git --redact
