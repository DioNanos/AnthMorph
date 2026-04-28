# Changelog

All notable changes to this project will be documented in this file.

## 0.2.0

- Repositioned AnthMorph as a high-performance Rust bridge for Codex/codex-vl, Anthropic Messages, OpenAI legacy chat, and Responses traffic.
- Added public compatibility ingress for Anthropic-style clients:
  - `POST /v1/messages`
  - `POST /v1/messages/count_tokens`
- Added public OpenAI legacy chat ingress:
  - `POST /v1/chat/completions`
  - `POST /chat/completions`
- Kept Codex/codex-vl optimized Responses ingress:
  - `POST /v1/responses`
- Added model normalization, function/tool name adaptation, SSE streaming support, local auth, CORS allow-lists, and rate limiting.
- Updated public README, packaging docs, npm metadata, and release notes.

## 0.1.x

- Initial Anthropic/OpenAI-compatible proxy work.
- Added npm packaging, Termux-oriented install support, streaming, tool-use handling, model discovery, health checks, and release verification scripts.
