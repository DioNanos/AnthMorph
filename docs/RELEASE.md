# Release Guide

This guide prepares AnthMorph for GitHub push/release and npm publish without embedding credentials in the repo.

## Preconditions

- release auth for GitHub and npm is configured externally
- git remote `origin` points to the release repository
- npm login is configured externally
- Docker is available for reproducible Linux checks when publishing

## Release Verification

Run local Rust coverage first:

```bash
cargo test
```

Run the Docker verification set before publishing:

```bash
./scripts/docker_release_checks.sh
```

Or step by step:

```bash
./scripts/docker_secret_scan.sh
./scripts/docker_rust_test.sh
./scripts/docker_build_linux.sh
./scripts/docker_npm_dry_run.sh
./scripts/docker_npm_dry_run.sh publish
```

## Checklist

- working tree reviewed and intentional
- `CHANGELOG.md` updated
- versions aligned in `Cargo.toml`, `Cargo.lock`, `package.json`, and docs
- Rust tests pass
- Docker secret scan passes
- macOS, Linux, and Termux npm installs build locally with Cargo
- npm pack dry-run passes
- npm publish dry-run passes
- runtime surface remains `POST /v1/responses`, `GET /v1/models`, and `GET /health`

## Git Push And Tag

```bash
git status
git add .
git commit -m "Release v0.2.1"
git tag -a v0.2.1 -m "Release v0.2.1"
git push origin develop
git push origin v0.2.1
```

## GitHub Release Notes

Use the `0.2.1` section from `CHANGELOG.md` as the release body.

If `gh` is installed:

```bash
awk '
  /^## 0.2.1$/ {capture=1; next}
  /^## / && capture {exit}
  capture {print}
' CHANGELOG.md > /tmp/anthmorph-v0.2.1-notes.md

gh release create v0.2.1 --title "v0.2.1" --notes-file /tmp/anthmorph-v0.2.1-notes.md
```

## npm Publish

Final publish:

```bash
npm publish --access public
```

## Notes

- Do not publish from a dirty repo.
- Do not store npm tokens, GitHub tokens, or API keys in tracked files.
- If the Codex Responses surface changes, update `README.md`, `CHANGELOG.md`, and release notes in the same commit.
