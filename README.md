# AnthMorph

[![Status](https://img.shields.io/badge/Status-0.1.5-blue.svg)](#project-status)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.94%2B-orange.svg)](https://www.rust-lang.org)
[![Target](https://img.shields.io/badge/Target-Termux%20%2F%20Linux-green.svg)](https://termux.dev)
[![npm](https://img.shields.io/npm/v/@mmmbuto/anthmorph?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/anthmorph)

AnthMorph is a universal Rust proxy for Anthropic `/v1/messages` and OpenAI `/v1/responses`.
It lets Claude-style clients and Codex-style clients talk to DeepSeek, Chutes, or other OpenAI-compatible backends through a profile-aware translation layer.

## Project Status

- Current line: `0.1.5`
- Primary target: `deepseek-v4-pro` through AnthMorph
- Secondary targets: `chutes.ai` and generic OpenAI-compatible endpoints
- Release model: MIT-licensed GitHub repo plus public npm package
- Packaging model: one npm package with Termux prebuilt and Linux source-build path

## Highlights

- Anthropic `/v1/messages` ingress for Claude-style clients
- OpenAI `/v1/responses` ingress for Codex/codex-vl
- `deepseek`, `chutes`, and `openai_generic` backend profiles
- `strict` and `compat` runtime modes
- long MCP tool-name normalization for DeepSeek's 64-char function-name limit
- Claude Code bootstrap via `anthmorphctl bootstrap claude-code`
- Codex bootstrap snippet via `anthmorphctl bootstrap codex`
- real-backend validation for Chutes, MiniMax, and Alibaba rejection flow
- direct DeepSeek validation script for `/models`, `/chat/completions`, and negative `/v1/responses`
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

Initialize the canonical config and pick DeepSeek:

```bash
anthmorphctl config bootstrap
anthmorphctl profile list
anthmorphctl init deepseek4 --port 3108 --compat-mode compat
anthmorphctl start
anthmorphctl status
```

Canonical config lives in:

```bash
~/.config/anthmorph/config.toml
```

Local dev fallback remains:

```bash
./.anthmorph/config.toml
```

Point Claude Code at AnthMorph:

```bash
anthmorphctl bootstrap claude-code --write
```

Generate a Codex provider snippet:

```bash
anthmorphctl bootstrap codex
```

Stop the proxy:

```bash
anthmorphctl stop
```

## Docs

- Claude Code setup: [docs/CLAUDE_CODE_SETUP.md](docs/CLAUDE_CODE_SETUP.md)
- Packaging details: [docs/PACKAGING.md](docs/PACKAGING.md)
- Release guide: [docs/RELEASE.md](docs/RELEASE.md)
- Changelog: [CHANGELOG.md](CHANGELOG.md)

## Packaging Notes

- Termux on Android/aarch64 uses the bundled prebuilt in `prebuilt/anthmorph`
- Linux and macOS build from source during install
- Docker is the supported reproducible release path on VPS3 and similar hosts
- If Cargo is unavailable on Linux/macOS, use the Docker build path documented in `docs/PACKAGING.md`

## Service Install

For local operator use, build and run AnthMorph through `anthmorphctl`:

```bash
cargo build --release
anthmorphctl init deepseek4 --port 3108 --compat-mode compat
anthmorphctl start
```

`anthmorphctl` now exports runtime configuration through environment variables before launch, so backend secrets do not appear in process arguments.

For a persistent user service:

```bash
anthmorphctl service install
anthmorphctl service status
```

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

Direct DeepSeek validation:

```bash
DEEPSEEK_API_KEY=... ./scripts/test_deepseek4_direct.sh
```

## License

MIT License

Copyright (c) 2026 Davide A. Guglielmi
