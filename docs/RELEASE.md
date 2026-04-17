# Release Guide

This guide prepares AnthMorph for GitHub push/release and npm publish without embedding credentials in the repo.

## Preconditions

- release auth for GitHub and npm is already configured on the release machine
- git remotes are configured with `origin` = GitHub and `develop` = VPS3 bare repo
- npm login is already configured externally
- Docker is available on the release machine

## Release verification

Run the full Docker verification set:

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
- versions aligned in `Cargo.toml`, `package.json`, and docs
- Docker secret scan passes
- Rust tests pass
- Docker Linux build passes
- npm pack dry-run passes
- npm publish dry-run passes

## GitHub push and tag

```bash
git status
git add .
git commit -m "Release v0.1.5"
git tag -a v0.1.5 -m "Release v0.1.5"
git push origin main
git push origin v0.1.5
git push develop main
git push develop v0.1.5
```

## GitHub release notes

Use the `0.1.5` section from `CHANGELOG.md` as the release body.

If `gh` is installed:

```bash
awk '
  /^## 0.1.5$/ {capture=1; next}
  /^## / && capture {exit}
  capture {print}
' CHANGELOG.md > /tmp/anthmorph-v0.1.5-notes.md

gh release create v0.1.5 --title "v0.1.5" --notes-file /tmp/anthmorph-v0.1.5-notes.md
```

If `gh` is not installed, create the release in the GitHub web UI from tag `v0.1.5`.

## npm publish

Final publish:

```bash
npm publish --access public
```

## Notes

- Do not publish from a dirty repo by accident.
- Do not store npm tokens, GitHub tokens, or API keys in tracked files.
- If Linux install UX changes materially, update `README.md` and `docs/PACKAGING.md` in the same release.
