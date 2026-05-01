# AnthMorph

[![Release](https://img.shields.io/github/v/release/DioNanos/AnthMorph?style=flat-square&logo=github)](https://github.com/DioNanos/AnthMorph/releases)
[![npm](https://img.shields.io/npm/v/@mmmbuto/anthmorph?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/anthmorph)
[![License: MIT](https://img.shields.io/badge/license-MIT-yellow?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.94%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Termux-2ea44f?style=flat-square)](#install)
[![API](https://img.shields.io/badge/api-Responses%20%7C%20Messages%20%7C%20Chat-2563eb?style=flat-square)](#project-status)
[![Local First](https://img.shields.io/badge/local--first-no%20external%20deps-111827?style=flat-square)](#what-it-does)

AnthMorph is a high-performance Rust API bridge for modern AI coding clients.
Its main job is to let Codex and codex-vl use providers that do not expose the
latest OpenAI `/responses` API by translating, normalizing, and streaming across
Anthropic Messages, OpenAI legacy chat-completions, and Responses-style traffic.

Use it when a provider is powerful but speaks the wrong wire format for your
client. AnthMorph keeps the local client surface stable, adapts model names and
tool calls, applies local auth/rate-limit policy, and forwards to the selected
backend with minimal runtime overhead.

## Project Status

- Current line: `0.2.1`
- Primary Codex API: `POST /v1/responses`
- Anthropic ingress: `POST /v1/messages`, `POST /v1/messages/count_tokens`
- OpenAI legacy ingress: `POST /v1/chat/completions`, `POST /chat/completions`
- Operational APIs: `GET /v1/models`, `GET /health`
- Target clients: Codex and codex-vl latest, plus Anthropic/OpenAI-compatible clients
- Target platforms: macOS, Linux, and Termux

## What It Does

- Translates Codex/codex-vl Responses traffic to native Responses or legacy chat-completions backends.
- Accepts Anthropic Messages API traffic and adapts it to OpenAI-compatible or Anthropic-compatible backends.
- Accepts OpenAI legacy chat-completions traffic for clients and tools that still use `/chat/completions`.
- Streams Server-Sent Events for supported streaming paths.
- Normalizes configured/default models for clients that send `default` or Claude-style model names.
- Shortens long function/tool names for providers with stricter tool-name limits.
- Supports local ingress auth, backend API key forwarding, CORS allow-lists, and per-client rate limiting.
- Serves `/v1/models` from backend discovery plus a local fallback model cache.

## Install

Global npm install:

```bash
npm install -g @mmmbuto/anthmorph
```

On macOS, npm install builds the local binary with Cargo.

Local source build:

```bash
cargo build --release
```

Run against a legacy OpenAI-compatible chat backend:

```bash
ANTHMORPH_BACKEND_URL=https://api.example.com/v1 \
ANTHMORPH_BACKEND_PROFILE=openai-generic \
ANTHMORPH_UPSTREAM_API=chat-completions \
ANTHMORPH_PRIMARY_MODEL=example/model \
ANTHMORPH_API_KEY="$PROVIDER_API_KEY" \
PORT=9876 \
anthmorph
```

Run against a native Responses backend:

```bash
ANTHMORPH_BACKEND_URL=https://api.example.com/v1 \
ANTHMORPH_BACKEND_PROFILE=openai-generic \
ANTHMORPH_UPSTREAM_API=responses \
ANTHMORPH_PRIMARY_MODEL=example/model \
ANTHMORPH_API_KEY="$PROVIDER_API_KEY" \
PORT=9876 \
anthmorph
```

## Codex and codex-vl

Point Codex or codex-vl at AnthMorph with Responses wire format:

```toml
model = "example/model"
model_provider = "anthmorph"

[model_providers.anthmorph]
name = "Provider via AnthMorph"
base_url = "http://127.0.0.1:9876/v1"
env_key = "OPENAI_API_KEY"
wire_api = "responses"
requires_openai_auth = false
```

Codex sends `/v1/responses` to AnthMorph. AnthMorph then chooses the configured
upstream mode:

```text
ANTHMORPH_UPSTREAM_API=responses          -> {BACKEND_URL}/responses
ANTHMORPH_UPSTREAM_API=chat-completions   -> {BACKEND_URL}/chat/completions
```

## Runtime Surface

```text
POST http://127.0.0.1:9876/v1/responses
POST http://127.0.0.1:9876/v1/messages
POST http://127.0.0.1:9876/v1/messages/count_tokens
POST http://127.0.0.1:9876/v1/chat/completions
POST http://127.0.0.1:9876/chat/completions
GET  http://127.0.0.1:9876/v1/models
GET  http://127.0.0.1:9876/health
```

## Environment

Minimum direct environment:

```bash
PORT=9876
ANTHMORPH_BACKEND_URL=https://api.example.com/v1
ANTHMORPH_BACKEND_PROFILE=openai-generic
ANTHMORPH_UPSTREAM_API=chat-completions
ANTHMORPH_PRIMARY_MODEL=example/model
ANTHMORPH_API_KEY=...
```

Optional:

```bash
ANTHMORPH_REASONING_MODEL=...
ANTHMORPH_INGRESS_API_KEY=...
ANTHMORPH_ALLOWED_ORIGINS=https://example.test
ANTHMORPH_RATE_LIMIT_PER_MINUTE=60
ANTHMORPH_STRICT_MODEL=true
ANTHMORPH_STREAM_CHUNK_TIMEOUT_SECS=30
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

Copyright (c) 2026 DioNanos
