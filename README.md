# AnthMorph

[![Status](https://img.shields.io/badge/Status-0.1.6-blue.svg)](#project-status)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.94%2B-orange.svg)](https://www.rust-lang.org)
[![npm](https://img.shields.io/npm/v/@mmmbuto/anthmorph?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/anthmorph)

AnthMorph is a local agent router for Claude-style and Codex-style clients.
It exposes Anthropic `/v1/messages` and OpenAI `/v1/responses`, then adapts requests to provider APIs with tool-name normalization, model profiles, local secrets, and service management.

The current focus is **DeepSeek V4 support**, especially `deepseek-v4-pro[1m]` for agentic Claude/Codex workflows. AnthMorph treats that profile strictly: if the provider returns a different model, the proxy reports the mismatch instead of silently downgrading.

## Project Status

- Current line: `0.1.6`
- Primary target: DeepSeek V4 Pro / `deepseek-v4-pro[1m]`
- Secondary targets: `deepseek-v4-pro`, `deepseek-v4-flash`, `chutes.ai`, and generic OpenAI-compatible backends
- Public package: `@mmmbuto/anthmorph`
- Runtime model: local daemon or foreground proxy configured from `~/.config/anthmorph/config.toml`

## Highlights

- Anthropic `/v1/messages` ingress for Claude Code and Claude-style clients
- OpenAI `/v1/responses` ingress for Codex and codex-vl style clients
- DeepSeek profiles for `deepseek-v4-pro[1m]`, normal Pro, and Flash
- Strict model validation to catch provider-side fallback or remapping
- Long MCP tool-name normalization for DeepSeek's 64-character function-name limit
- Local secret handling through env vars, macOS Keychain, or fallback vault helpers
- `anthmorphctl` for config bootstrap, profile selection, service install, health, and model probes
- Chutes and generic OpenAI-compatible backends remain supported as explicit profiles

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

## Quickstart: DeepSeek V4

Create the canonical config and initialize DeepSeek:

```bash
anthmorphctl config bootstrap
anthmorphctl init deepseek4 --port 3108 --compat-mode compat
anthmorphctl key set deepseek4
anthmorphctl key test deepseek4
```

Start AnthMorph:

```bash
anthmorphctl service install
anthmorphctl service restart
anthmorphctl status
```

Probe the selected DeepSeek model:

```bash
anthmorphctl model probe deepseek4
```

The default DeepSeek profile requests `deepseek-v4-pro[1m]` in strict mode. If DeepSeek returns `deepseek-v4-flash` or any other model, AnthMorph reports the mismatch.

Canonical config:

```bash
~/.config/anthmorph/config.toml
```

Local dev fallback:

```bash
./.anthmorph/config.toml
```

## Client Bootstrap

Generate Claude Code settings:

```bash
anthmorphctl bootstrap claude-code --write
```

Generate a Codex provider snippet:

```bash
anthmorphctl bootstrap codex
```

AnthMorph exposes:

```text
http://127.0.0.1:3108/v1/messages
http://127.0.0.1:3108/v1/responses
http://127.0.0.1:3108/v1/models
http://127.0.0.1:3108/health
```

## DeepSeek Notes

DeepSeek currently exposes `deepseek-v4-pro` and `deepseek-v4-flash` through its OpenAI-compatible model list. Its Claude Code guide also documents `deepseek-v4-pro[1m]` for Anthropic-compatible usage.

AnthMorph handles this by making `deepseek-v4-pro[1m]` a strict profile:

- Claude-style requests can use the DeepSeek Anthropic lane.
- Codex-style `/v1/responses` requests do not silently fall back to `/chat/completions` when `[1m]` is selected.
- If the provider remaps `[1m]` to Flash, AnthMorph returns a clear model mismatch error.

## Validation

Local Rust tests:

```bash
cargo test -- --nocapture
```

Docker release checks:

```bash
./scripts/docker_release_checks.sh
```

Direct DeepSeek validation:

```bash
DEEPSEEK_API_KEY=... ./scripts/test_deepseek4_direct.sh
```

Real Claude Code payload replay:

```bash
./scripts/test_claude_code_patterns_real.sh chutes
```

## Docs

- Claude Code setup: [docs/CLAUDE_CODE_SETUP.md](docs/CLAUDE_CODE_SETUP.md)
- Packaging details: [docs/PACKAGING.md](docs/PACKAGING.md)
- Release guide: [docs/RELEASE.md](docs/RELEASE.md)
- Changelog: [CHANGELOG.md](CHANGELOG.md)

## License

MIT License

Copyright (c) 2026 Davide A. Guglielmi
