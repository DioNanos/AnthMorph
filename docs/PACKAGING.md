# Packaging

AnthMorph ships as a single npm package: `@mmmbuto/anthmorph`.

## Platform model

- Termux on Android/aarch64 uses the bundled prebuilt binary from `prebuilt/anthmorph`
- Linux and macOS build from source during install
- the supported reproducible build path is Docker

The bundled prebuilt is currently **Termux-only**. Linux is supported through source builds, not a bundled Linux prebuilt.

## Install behavior

`postinstall` does this:

- on Termux: uses the packaged prebuilt when its version matches the package version
- on Linux/macOS: runs `cargo build --release`
- if Cargo is missing: exits with a clear error and points to Docker-based build instructions

## Docker build

Build a Linux release binary without depending on host Rust:

```bash
./scripts/docker_build_linux.sh
```

This uses `rust:1.89-bookworm` and exports `/usr/local/cargo/bin` into `PATH`, which is required in this environment.

## npm package contents

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

## Dry-run validation

Pack dry-run:

```bash
./scripts/docker_npm_dry_run.sh
```

Publish dry-run:

```bash
./scripts/docker_npm_dry_run.sh publish
```
