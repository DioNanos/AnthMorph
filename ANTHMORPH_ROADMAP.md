# AnthMorph Roadmap — Bidirectional Anthropic↔OpenAI Proxy

**Branch**: `develop` (Forge)
**Release**: `main` (GitHub)
**Focus**: OpenRouter locale — traduce protocolli, gestisce chiavi, ottimizza per Claude Code & Codex

---

## Architettura

```
Client (Anthropic)              Client (OpenAI)
   ┌─ /v1/messages ─┐       ┌─ /v1/responses ─┐
   │ Claude Code     │       │ Codex/codex-vl   │
   │ OpenCode        │       │                   │
   └───────┬─────────┘       └───────┬───────────┘
           │                         │
           ▼                         ▼
    ┌─────────────────────────────────────┐
    │            ANTHMORPH                │
    │                                     │
    │  anthropic_to_openai()              │
    │     ┌→ strip billing headers        │
    │     └→ tool name normalization      │
    │                                     │
    │  openai_to_anthropic()              │
    │     ┌→ extract tool calls (parser)  │
    │     └→ map stop reasons             │
    │                                     │
    │  Streaming:                         │
    │     ┌→ filter tool markup mid-strm  │
    │     └→ SSE event translation        │
    └────────────────┬────────────────────┘
                     │
                     ▼
    ┌─────────────────────────────────────┐
    │         Backend Provider            │
    │  (DeepSeek / Chutes / OpenAI-gen)   │
    └─────────────────────────────────────┘
```

## Fasi

### Fase 0 — Foundation (in corso)
- [x] `anthropic_to_openai()` — Anthropic request → OpenAI request
- [x] `openai_to_anthropic()` — OpenAI response → Anthropic response
- [x] `responses.rs` — OpenAI Responses API request models
- [x] Streaming proxy pass-through
- [x] `tool_names.rs` — Tool name shortening (DeepSeek 64-char limit)
- [x] `model_cache.rs` — Model validation and cache
- [x] Key management (ingress/backend separation)
- [x] Profiles: DeepSeek, Chutes, OpenAI-generic
- [x] `anthmorphctl` CLI

### Fase 1 — DeepSeek Tool Parser (Rust)
**Obiettivo**: Parsare tool calls di DeepSeek dal testo generato (formato unicode)

Da vllm-mlx `deepseek_tool_parser.py`:
```
<｜tool▁calls▁begin｜>
<｜tool▁call▁begin｜>function<｜tool▁sep｜>get_weather
```json
{"city": "Paris"}
```<｜tool▁call▁end｜>
<｜tool▁calls▁end｜>
```

Nuovo modulo: `src/tool_parsers.rs` con:
- [x] `ToolParser` trait (extract_tool_calls, extract_tool_calls_streaming)
- [x] `DeepSeekToolParser` struct
- [x] Regex per token unicode DeepSeek
- [x] Integrazione in `proxy.rs` post-risposta backend

### Fase 2 — Streaming Tool Call Filter
**Obiettivo**: Sopprimere markup tool dal testo streaming

- `StreamBuffer` che accumula token durante generazione markup
- Rileva `<tool_call>`, `<｜tool▁calls▁begin｜>`, ecc. in streaming
- Emette solo quando tool call è completo
- Non blocca testo normale

### Fase 3 — x-anthropic-billing-header Strip
**Obiettivo**: Rimuovere header per-turn di Claude Code che bloccano prefix caching

- [x] Una regex in `transform.rs` `anthropic_to_openai()`
- [x] Pattern: `x-anthropic-billing-header:[^\n]*\n?`
- [x] Effetto: permette prefix-cache reuse tra turni

### Fase 4 — Responses API v2 SSE Events
**Obiettivo**: Streaming eventi strutturati per `/v1/responses`

- `responses_events.rs` con modelli strict
- Eventi: `response.created`, `response.output_item.added`, `response.output_text.delta`, ecc.
- 8 eventi totali (da vllm-mlx `responses_models.py`)

### Fase 5 — Additional Tool Parsers (futuro)
- Qwen parser (`<tool_call>`, `[Calling tool:]`)
- Mistral parser (`[TOOL_CALLS]`)
- Kimi parser (`<|tool_calls_section_begin|>`)
- GLM-4.7 parser (`<tool_call>func_name<arg_key>`)

## Regole di sviluppo

- `origin` → Forge (`develop` = lavoro, `main` = stabile)
- `github` → GitHub (`main` = solo release pubbliche)
- Commit atomici per fase
- Build su Forge runner (`mac-arm64:host`)
- Test: `cargo test -- --nocapture`
- Ogni fase: test Rust + smoke test con `curl`

## Completato

- (vuoto — inizio lavori)
