# Changelog

All notable changes to this project will be documented in this file.

## 0.2.0

- refocused AnthMorph as a Codex companion daemon with one public generation ingress: `POST /v1/responses`
- removed public `/v1/messages` and `/v1/messages/count_tokens` routes from the runtime router
- changed `/v1/responses` to prefer native Responses API upstreams, with an explicit `ANTHMORPH_UPSTREAM_API=chat-completions` backend adapter for legacy providers such as NVIDIA NIM
- preserved model listing, health checks, ingress auth, rate limiting, model remapping, and backend function-name normalization
- added regression coverage proving the Responses payload stays Responses-shaped and does not synthesize chat `messages`
- updated docs and package metadata for the 0.2.0 Responses-native line

## 0.1.5

- refocused the public README around AnthMorph as a local agent router with DeepSeek V4 support
- documented strict `deepseek-v4-pro[1m]` profile behavior and provider model-mismatch reporting
- refreshed Claude Code setup docs around the DeepSeek V4 path
- removed local-path references and tightened public release docs
- hardened `anthmorphctl start` to pass secrets via environment instead of process arguments
- narrowed npm package contents to the public docs surface and excluded internal release planning notes
- refreshed GitHub/npm release metadata for the 0.1.5 publish line

## 0.1.4

- hardened AnthMorph for public GitHub and npm release flow
- expanded public docs into linked guides for Claude Code setup, packaging, and release
- added Docker-based verification scripts for secret scan, Linux build, Rust tests, and npm dry-runs
- cleaned npm publish surface and clarified Termux prebuilt vs Linux source-build behavior
- kept Claude Code CLI compatibility improvements, real-backend corpus tests, and bootstrap flow from the current working line

## 0.1.3

- added `compat` mode and separated compatibility posture from backend profile
- added `/health`, `/v1/models`, and `/v1/messages/count_tokens`
- improved Claude Code request compatibility and SSE behavior
- added real-backend smoke coverage for Chutes, MiniMax, and Alibaba rejection flow

## 0.1.2

- fixed MCP `tool_use` streaming behavior for Claude Code compatibility

## 0.1.1

- added global CLI packaging
- added bundled Termux prebuilt binary
- improved npm install path for local usage

## 0.1.0

- initial public release
- Anthropic `/v1/messages` ingress with OpenAI-compatible upstream translation
- Chutes-first Rust proxy with streaming, tools, and local operator CLI
