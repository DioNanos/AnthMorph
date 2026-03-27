# AnthMorph

[![Status](https://img.shields.io/badge/Status-0.1.2-blue.svg)](#project-status)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.94%2B-orange.svg)](https://www.rust-lang.org)
[![Target](https://img.shields.io/badge/Target-Termux%20%2F%20Linux-green.svg)](https://termux.dev)
[![npm](https://img.shields.io/npm/v/@mmmbuto/anthmorph?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/anthmorph)

AnthMorph is a Chutes-first Anthropic `/v1/messages` proxy written in Rust.
It lets Claude-style clients talk to Chutes or other OpenAI-compatible backends through a safer, profile-aware translation layer.

Core capabilities:
- Anthropic `/v1/messages` ingress with OpenAI-compatible upstream translation
- `chutes` profile optimized for Chutes-specific compatibility, including `top_k` and reasoning handling
- `openai_generic` profile for conservative compatibility with generic OpenAI-style providers
- Streaming SSE translation with fragmented tool-call handling
- Local control CLI for init, start, stop, restart, status, and logs
- Termux-first npm distribution with bundled prebuilt binary and local self-build on Linux/macOS

## Project Status

- Current line: `0.1.2`
- Primary target: `chutes.ai`
- Secondary target: generic OpenAI-compatible endpoints
- Tested locally against Chutes, MiniMax, and Alibaba Coding Plan rejection handling
- Distribution paths: Termux-first npm package with bundled prebuilt binary, plus source builds
- Repository metadata is aligned for GitHub and npm publication

## Quickstart

1. Install

Source build:

```bash
cargo build --release
```

Global npm install:

```bash
npm install -g @mmmbuto/anthmorph
```

2. Initialize Chutes profile

```bash
export CHUTES_API_KEY=your_key_here
anthmorphctl init chutes --port 3107
```

3. Start proxy

```bash
anthmorphctl start
anthmorphctl status
```

4. Stop proxy

```bash
anthmorphctl stop
```

## CLI Control

`anthmorphctl` is the operator entrypoint.
By default it stores runtime state under `.anthmorph/` inside the installed package root.
For shell wrappers and daily usage, prefer setting `ANTHMORPH_STATE_DIR` to a dedicated writable path.

Common commands:

```bash
anthmorphctl init chutes
anthmorphctl init minimax
anthmorphctl init openai --backend-url https://api.example.com/v1 --model my-model --key-env EXAMPLE_API_KEY
anthmorphctl start
anthmorphctl status
anthmorphctl logs
anthmorphctl stop
```

Direct binary usage is also available:

```bash
anthmorph --port 3107 --backend-profile chutes --backend-url https://llm.chutes.ai/v1 --model Qwen/Qwen3-Coder-Next-TEE --api-key "$CHUTES_API_KEY"
```

## Architecture

- `Ingress`: accepts Anthropic `/v1/messages` requests and validates profile-safe behavior.
- `Transform`: converts Anthropic messages, tools, and stop reasons into OpenAI-compatible payloads.
- `Streaming`: translates upstream SSE chunks back into Anthropic-style streaming events.
- `Profiles`: selects `chutes` or `openai_generic` behavior for request and response handling.
- `Control CLI`: manages local config, runtime state, start/stop/status, and operator logs.

## API Key Policy

Preferred mode:
- Store only the environment variable name with `anthmorphctl set key-env ENV_NAME`
- Keep the secret in your shell environment

Optional mode:
- Persist the key locally with `anthmorphctl set key VALUE --save`

Recommendation:
- Do not save API keys in the repo by default
- Use env vars for daily operation and CI

## Backend Profiles

- `chutes`: optimized path for Chutes, including `top_k` pass-through and reasoning support
- `openai_generic`: strips nonstandard fields and fails conservatively when the backend cannot represent Anthropic semantics safely

## Safety Rules

- Assistant thinking blocks in request history are rejected instead of being downgraded to plain text
- Generic mode rejects backend reasoning content that cannot be represented safely
- Streaming tool-call deltas support contiguous fragments and fail closed on unsafe interleaving
- Optional ingress auth supports `Authorization: Bearer ...` or `x-api-key`
- CORS is disabled unless explicitly configured

## Real Backend Coverage

Current integration coverage:
- `chutes.ai`: positive end-to-end smoke test
- `MiniMax`: positive end-to-end smoke test in generic mode
- `Alibaba Coding Plan`: negative expected test documenting upstream rejection for generic chat-completions flow

## npm Packaging

This repository already includes npm packaging files:
- `package.json`
- `bin/anthmorph`
- `scripts/postinstall.js`

Packaging behavior:
- `npm install -g @mmmbuto/anthmorph` exposes `anthmorph` and `anthmorphctl`
- Termux uses the bundled `prebuilt/anthmorph` from the npm tarball
- Linux and macOS use `postinstall` to build locally with Cargo
- if no binary is available later, the `anthmorph` shim still falls back to a local release build

## Build And Test

```bash
cargo test
npm pack --dry-run
```

Real backend smoke tests:

```bash
./scripts/smoke_test.sh chutes
./scripts/smoke_test.sh minimax
./scripts/smoke_test.sh alibaba
```

## Repository Layout

```text
src/                  Rust proxy implementation
bin/                  npm-exposed executable shims
scripts/              control CLI, smoke tests, npm postinstall
tests/                protocol and real-backend integration tests
```

## Documentation

- Repository: https://github.com/DioNanos/AnthMorph
- npm package: https://www.npmjs.com/package/@mmmbuto/anthmorph
- Issue tracker: https://github.com/DioNanos/AnthMorph/issues

## Roadmap

1. Broader compatibility validation across more OpenAI-compatible providers
2. End-to-end validation against real Claude-style clients
3. Public-deployment hardening with rate limits and clearer auth policy
4. Better streaming coverage for complex multi-tool interleaving

## License

MIT License
<p>
Copyright (c) 2026 Davide A. Guglielmi<br>
Made in Italy
</p>
