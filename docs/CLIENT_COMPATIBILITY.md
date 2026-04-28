# Client Compatibility Notes

AnthMorph 0.2.x is primarily optimized for Codex and codex-vl through:

```text
POST /v1/responses
```

It also exposes compatibility ingress for Messages-style and legacy OpenAI-style clients:

```text
POST /v1/messages
POST /v1/messages/count_tokens
POST /v1/chat/completions
POST /chat/completions
```

New Codex and codex-vl setup should target AnthMorph with `wire_api = "responses"`.

For current setup, use the root [README](../README.md).
