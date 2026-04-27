# AnthMorph

[![Status](https://img.shields.io/badge/Status-0.2.0-blue.svg)](#project-status)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.94%2B-orange.svg)](https://www.rust-lang.org)
[![npm](https://img.shields.io/npm/v/@mmmbuto/anthmorph?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/anthmorph)

AnthMorph is a small Rust daemon for Codex and codex-vl. It accepts OpenAI Responses API traffic locally, applies lightweight governance, model normalization, auth, rate limiting, and provider quirks, then forwards to the selected backend.

The 0.2.x line is intentionally simple: one optimized Codex ingress and no public chat or Anthropic compatibility routes. Backends can be native Responses providers or legacy OpenAI-compatible chat providers selected by `ANTHMORPH_UPSTREAM_API`.

## Project Status

- Current line: `0.2.0`
- Public generation API: `POST /v1/responses`
- Operational APIs: `GET /v1/models`, `GET /health`
- Target clients: Codex and codex-vl with `wire_api = "responses"`
- Target platforms: Linux, macOS, and Termux
- Target backends: OpenAI-compatible Responses providers and legacy chat-completions providers behind the same Codex-facing `/v1/responses` ingress

## What It Does

- Exposes only the Codex-friendly Responses generation surface: `/v1/responses`
- Keeps public client traffic Responses-native; legacy chat-completions translation is only an internal backend adapter when explicitly selected
- Forwards upstream streaming as Server-Sent Events when `stream: true`
- Normalizes configured/default models for clients that send `default` or Claude-style model names
- Shortens long function tool names for providers with stricter tool-name limits
- Supports local ingress auth, backend API key forwarding, CORS allow-lists, and per-client rate limiting
- Serves `/v1/models` from backend discovery plus local fallback model cache

## Install

Global npm install:

```bash
npm install -g @mmmbuto/anthmorph
```

Local source build:

```bash
cargo build --release
```

Run directly:

```bash
ANTHMORPH_BACKEND_URL=https://integrate.api.nvidia.com/v1 \
ANTHMORPH_BACKEND_PROFILE=openai-generic \
ANTHMORPH_UPSTREAM_API=chat-completions \
ANTHMORPH_PRIMARY_MODEL=deepseek-ai/deepseek-v4-pro \
ANTHMORPH_API_KEY="$NVIDIA_API_KEY" \
PORT=9876 \
anthmorph
```

## Codex Provider

Point Codex or codex-vl at AnthMorph with Responses wire format:

```toml
model = "deepseek-ai/deepseek-v4-pro"
model_provider = "nvidia-anthmorph"

[model_providers.nvidia-anthmorph]
name = "NVIDIA via AnthMorph"
base_url = "http://127.0.0.1:9876/v1"
env_key = "OPENAI_API_KEY"
wire_api = "responses"
requires_openai_auth = false
```

For native Responses backends, AnthMorph forwards generation calls to:

```text
{ANTHMORPH_BACKEND_URL}/responses
```

For legacy providers such as NVIDIA NIM, set `ANTHMORPH_UPSTREAM_API=chat-completions`. AnthMorph still exposes only `/v1/responses` to Codex and performs the backend adaptation internally.

## Runtime Surface

```text
POST http://127.0.0.1:9876/v1/responses
GET  http://127.0.0.1:9876/v1/models
GET  http://127.0.0.1:9876/health
```

`/v1/messages`, `/v1/messages/count_tokens`, and public chat ingress are not part of the 0.2.x runtime surface.

## Environment

Minimum direct environment:

```bash
PORT=9876
ANTHMORPH_BACKEND_URL=https://integrate.api.nvidia.com/v1
ANTHMORPH_BACKEND_PROFILE=openai-generic
ANTHMORPH_UPSTREAM_API=chat-completions
ANTHMORPH_PRIMARY_MODEL=deepseek-ai/deepseek-v4-pro
ANTHMORPH_API_KEY=...
```

Optional:

```bash
ANTHMORPH_REASONING_MODEL=...
ANTHMORPH_INGRESS_API_KEY=...
ANTHMORPH_ALLOWED_ORIGINS=https://example.test
ANTHMORPH_RATE_LIMIT_PER_MINUTE=60
ANTHMORPH_STRICT_MODEL=true
```

## Validation

Local Rust tests:

```bash
cargo test
```

Docker release checks:

```bash
./scripts/docker_release_checks.sh
```

## Docs

- Packaging details: [docs/PACKAGING.md](docs/PACKAGING.md)
- Release guide: [docs/RELEASE.md](docs/RELEASE.md)
- Changelog: [CHANGELOG.md](CHANGELOG.md)

## License

MIT License

Copyright (c) 2026 Davide A. Guglielmi
