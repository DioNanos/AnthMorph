# Changelog

All notable changes to this project will be documented in this file.

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
