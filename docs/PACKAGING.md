# Packaging

AnthMorph ships as a single npm package: `@mmmbuto/anthmorph`.

## Platform Model

- Termux on Android/aarch64 uses the bundled prebuilt binary from `prebuilt/anthmorph`
- Linux and macOS build from source during install
- the supported reproducible build path is Docker

The bundled prebuilt is currently Termux-only. Linux and macOS are supported through source builds.

## Install Behavior

`postinstall` does this:

- on Termux: uses the packaged prebuilt when its version matches the package version
- on Linux/macOS: runs `cargo build --release`
- if Cargo is missing: exits with a clear error and points to Docker-based build instructions

## Runtime Shape

The 0.2.x package is a Codex companion daemon. The public runtime surface is:

- `POST /v1/responses`
- `GET /v1/models`
- `GET /health`

The package exposes three public compatibility surfaces: Codex/codex-vl `/v1/responses`, Anthropic Messages `/v1/messages`, and OpenAI legacy chat `/v1/chat/completions`. `ANTHMORPH_UPSTREAM_API=chat-completions` remains the backend adapter for providers that do not expose `/responses`.

## Docker Build

Build a Linux release binary without depending on host Rust:

```bash
./scripts/docker_build_linux.sh
```

## npm Package Contents

The published package should include only:

- runtime shims and CLI scripts
- Rust sources and manifests needed for local builds
- docs and changelog
- the Termux prebuilt

The published package should not include:

- `target/`
- local state like `.anthmorph/`
- test output, temp logs, or tarballs
- operator-only release scratch files

## Dry-Run Validation

Pack dry-run:

```bash
./scripts/docker_npm_dry_run.sh
```

Publish dry-run:

```bash
./scripts/docker_npm_dry_run.sh publish
```
