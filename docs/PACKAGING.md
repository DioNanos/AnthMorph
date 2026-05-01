# Packaging

AnthMorph ships as a single npm package: `@mmmbuto/anthmorph`.

## Platform Model

- Linux x64 uses `prebuilt/linux-x64/anthmorph`
- macOS Apple Silicon uses `prebuilt/darwin-arm64/anthmorph`
- Termux on Android/aarch64 uses `prebuilt/android-arm64/anthmorph`
- unsupported platforms may build from source during install when Cargo is available

The published npm package carries the supported release binaries directly.

## Install Behavior

`postinstall` does this:

- selects the matching prebuilt for Linux x64, macOS arm64, or Termux Android arm64
- verifies the selected binary reports the package version
- bootstraps the default config when missing
- falls back to `cargo build --release` only when no supported prebuilt exists

## Runtime Shape

The package exposes three public compatibility surfaces:

- Codex/codex-vl Responses ingress: `POST /v1/responses`
- Anthropic Messages ingress: `POST /v1/messages`
- OpenAI legacy chat ingress: `POST /v1/chat/completions`

It also exposes:

- `POST /v1/messages/count_tokens`
- `GET /v1/models`
- `GET /health`

`ANTHMORPH_UPSTREAM_API=chat-completions` is the backend adapter for providers that do not expose `/responses`.

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
- Linux, macOS, and Termux prebuilts

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
