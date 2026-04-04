#!/bin/sh
set -eu

SCRIPT_PATH=$(readlink -f -- "$0" 2>/dev/null || printf "%s" "$0")
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")/.." && pwd)

echo "[1/4] Secret scan"
sh "$ROOT_DIR/scripts/docker_secret_scan.sh"

echo "[2/4] Rust tests"
sh "$ROOT_DIR/scripts/docker_rust_test.sh"

echo "[3/4] Linux release build"
sh "$ROOT_DIR/scripts/docker_build_linux.sh"

echo "[4/4] npm dry-runs"
sh "$ROOT_DIR/scripts/docker_npm_dry_run.sh"
sh "$ROOT_DIR/scripts/docker_npm_dry_run.sh" publish
