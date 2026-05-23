# Changelog

All notable changes to this project will be documented in this file.

## 0.2.3

- Streaming `response.completed` SSE event now carries the `usage` field (input/output tokens). Token tracking on consumer clients (codex, codex-vl, Claude Code, and any client consuming the Responses streaming wire format) is restored — previously all token counters reported zero in streaming mode because the final event payload omitted `usage`.
- Upstream `stream_options.include_usage=true` is enabled by default in the OpenAI-generic translation path so backends that gate the usage chunk on this flag (Z.AI, DeepSeek, OpenAI itself) emit it.
- Accumulator captures the latest `usage` chunk in the streaming loop and injects it into the `response.completed` payload before sending. Non-streaming path was already correct and is unchanged.

## 0.2.2

- Restored Linux and Termux npm prebuilts.
- Kept macOS npm installs on local Cargo builds.

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
