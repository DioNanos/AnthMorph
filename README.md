# AnthMorph

[![Status](https://img.shields.io/badge/Status-0.1.4-blue.svg)](#project-status)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.94%2B-orange.svg)](https://www.rust-lang.org)
[![Target](https://img.shields.io/badge/Target-Termux%20%2F%20Linux-green.svg)](https://termux.dev)
[![npm](https://img.shields.io/npm/v/@mmmbuto/anthmorph?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/anthmorph)

AnthMorph is a Chutes-first Anthropic `/v1/messages` proxy written in Rust.
It lets Claude-style clients talk to Chutes or other OpenAI-compatible backends through a profile-aware translation layer optimized for Claude Code CLI compatibility.

## Project Status

- Current line: `0.1.4`
- Primary target: `chutes.ai`
- Secondary target: generic OpenAI-compatible endpoints
- Release model: MIT-licensed GitHub repo plus public npm package
- Packaging model: one npm package with Termux prebuilt and Linux source-build path

## Highlights

- Anthropic `/v1/messages` ingress with OpenAI-compatible upstream translation
- `chutes` and `openai_generic` backend profiles
- `strict` and `compat` runtime modes
- Claude Code bootstrap via `anthmorphctl bootstrap claude-code`
- real-backend validation for Chutes, MiniMax, and Alibaba rejection flow
- Docker release checks for secret scan, Rust tests, Linux build, and npm dry-runs

## Install

Global npm install:

```bash
npm install -g @mmmbuto/anthmorph
```

Local source build:

```bash
cargo build --release
```

Linux Docker build:

```bash
./scripts/docker_build_linux.sh
```

## Quickstart

Initialize and run against Chutes:

```bash
export CHUTES_API_KEY=your_key_here
anthmorphctl init chutes --port 3107 --compat-mode compat
anthmorphctl start
anthmorphctl status
```

Point Claude Code at AnthMorph:

```bash
anthmorphctl bootstrap claude-code --write
```

Stop the proxy:

```bash
anthmorphctl stop
```

## Docs

- Claude Code setup: [docs/CLAUDE_CODE_SETUP.md](/home/dag/Dev/AnthMorph/docs/CLAUDE_CODE_SETUP.md)
- Packaging details: [docs/PACKAGING.md](/home/dag/Dev/AnthMorph/docs/PACKAGING.md)
- Release guide: [docs/RELEASE.md](/home/dag/Dev/AnthMorph/docs/RELEASE.md)
- Changelog: [CHANGELOG.md](/home/dag/Dev/AnthMorph/CHANGELOG.md)

## Packaging Notes

- Termux on Android/aarch64 uses the bundled prebuilt in `prebuilt/anthmorph`
- Linux and macOS build from source during install
- Docker is the supported reproducible release path on VPS3 and similar hosts
- If Cargo is unavailable on Linux/macOS, use the Docker build path documented in `docs/PACKAGING.md`

## Validation

Local Rust tests:

```bash
cargo test -- --nocapture
```

Docker release checks:

```bash
./scripts/docker_release_checks.sh
```

Real payload replay:

```bash
./scripts/test_claude_code_patterns_real.sh chutes
./scripts/test_claude_code_patterns_real.sh minimax
```

## License

MIT License

Copyright (c) 2026 Davide A. Guglielmi
