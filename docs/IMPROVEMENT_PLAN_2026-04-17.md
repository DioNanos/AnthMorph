# AnthMorph — Piano di Miglioramenti & Fix

**Data audit**: 2026-04-17
**Versione analizzata**: v0.1.4 (commit `69a7d03`) + modifiche uncommitted (`model_cache` + auth passthrough)
**Branch**: `main` (tracking `origin/main`)
**Esecutore previsto**: GLM 5.1
**Stato**: piano non ancora implementato — fermo prima di scrivere codice.

---

## 0. Contesto rapido

AnthMorph è un proxy Rust (axum + tokio) che traduce richieste Anthropic `/v1/messages` verso backend OpenAI-compatibili (Chutes, MiniMax, OpenAI generico). Supporta streaming SSE, tool use, thinking blocks, auth passthrough.

**Working tree sporco**:
- `src/main.rs` — integra model_cache + background refresh
- `src/proxy.rs` — refactor auth: `extract_client_key` + `resolve_backend_key`
- `src/error.rs` — `request_id` nel body, mapping status aggiuntivi (503, 413)
- `src/model_cache.rs` — nuovo file (cache `/v1/models` con `RwLock`)

**Prerequisito #0 per GLM**: prima di aprire qualsiasi altro intervento, committare/normalizzare il working tree corrente (non incluso in questo piano, va fatto dall'utente o dall'esecutore come primo step separato).

---

## 1. Principi di esecuzione (vincolanti per GLM 5.1)

Derivati da `~/CLAUDE.md` (Karpathy principles). Applicare rigorosamente:

1. **Surgical changes**: toccare solo ciò che il task richiede. Nessun refactor opportunistico.
2. **Simplicity first**: niente astrazioni premature, niente feature non richieste.
3. **Un task = un commit** (o max due se logicamente inseparabili). Messaggio in italiano, stile del repo.
4. **Verifica obbligatoria prima di marcare come done**:
   - `cargo build --release` passa
   - `cargo test` passa (incluso nuovi test del task)
   - `cargo clippy -- -D warnings` passa (se già configurato; altrimenti non introdurre warnings nuovi)
5. **Nessuna modifica a**: `Cargo.lock` (salvo update deps necessari), `prebuilt/`, `.git/`, file non tracciati dell'utente.
6. **Ordine**: procedere P0 → P1 → P2. Fermarsi tra fasi per review.
7. **Test**: ogni fix P0/P1 DEVE introdurre almeno un test (unit o integration) che fallisce senza il fix.

---

## 2. Riepilogo priorità

| # | Priorità | Titolo | File principali |
|---|----------|--------|-----------------|
| 1 | **P0** | Redazione secret in log di errore upstream | `src/proxy.rs:211,258` |
| 2 | **P0** | Rate limiting opzionale in ingress | `src/proxy.rs`, `src/main.rs` |
| 3 | **P1** | Handling JSON malformato in `tool_calls.arguments` | `src/transform.rs:465` |
| 4 | **P1** | Graceful shutdown del task di refresh model cache | `src/main.rs:110-115` |
| 5 | **P1** | Validazione CORS: rifiuto wildcard `*` | `src/proxy.rs:959-980` |
| 6 | **P1** | Test di timeout (client disconnect, upstream slow) | `tests/` |
| 7 | **P1** | Deduplicazione mapping `stop_reason` | `src/transform.rs:475-509` |
| 8 | **P1** | Estrazione `ToolCallAccumulator` da `create_sse_stream` | `src/proxy.rs:284-550` |
| 9 | **P2** | Backoff + error escalation in `model_cache::refresh` | `src/model_cache.rs` |
| 10 | **P2** | Rimozione o implementazione di `fallback_models` | `src/proxy.rs:604,627-639` |
| 11 | **P2** | Per-chunk timeout nello streaming SSE | `src/proxy.rs:241+` |
| 12 | **P2** | `docs/TROUBLESHOOTING.md` | nuovo |
| 13 | **P2** | Test su `model_cache` refresh | nuovo |
| 14 | **P2** | Bootstrap timeout configurabile in `anthmorphctl` | `scripts/anthmorphctl` |

---

## 3. Task dettagliati

### P0-1 — Redazione secret nei log di errore upstream

**File**:
- `src/proxy.rs:211` — `tracing::error!("Upstream error ({}): {}", status, error_text);`
- `src/proxy.rs:258` — analogo nel path streaming.

**Problema**: `error_text` è il body grezzo del backend. Backend come Chutes o OpenAI-compatibili possono riflettere in un errore l'header `Authorization` inviato o l'intero request dump → leak della API key nei log.

**Intervento**:
1. Introdurre in `src/error.rs` (o in nuovo `src/redact.rs` se si supera una funzione) una funzione pubblica `redact_secrets(input: &str) -> Cow<'_, str>` che:
   - Sostituisce `Bearer\s+[A-Za-z0-9._\-]+` con `Bearer ***`.
   - Sostituisce `x-api-key[":\s=]+[A-Za-z0-9._\-]+` con `x-api-key: ***` (case-insensitive).
   - Sostituisce sequenze che iniziano con `sk-`, `sk_`, `cpk_` (min 20 char) con `***`.
   - Tronca input a 2 KiB con suffisso `… [truncated]`.
2. Usare `redact_secrets` in `proxy.rs:211` e `proxy.rs:258`.
3. Test unitari in `redact.rs` (o inline) per ogni pattern + input pulito invariato.

**Criteri di successo**:
- `cargo test redact` passa.
- Test integration: forzare upstream a rispondere con body contenente `Bearer cpk_test123...`; verificare che log catturato non contenga il valore.

**No-go**: non loggare hash del secret, non loggarne la lunghezza.

---

### P0-2 — Rate limiting opzionale ingress

**Problema**: nessun throttling. Un client può saturare la quota backend condivisa o abusare del proxy.

**Intervento**:
1. Aggiungere dipendenza `tower_governor = "0.4"` (o equivalente minimale, valutare anche `governor` + middleware custom se più piccolo).
2. Nuova config in `Config` (`src/proxy.rs`):
   - `rate_limit_per_minute: Option<u32>` — env `ANTHMORPH_RATE_LIMIT_PER_MINUTE`.
   - Se `None` → nessun limiter (default attuale invariato).
3. In `main.rs`, quando `Some(n)`, aggiungere `GovernorLayer` con key extractor per `x-api-key`/`Authorization` (fallback IP se assente).
4. Documentare in `README.md` la nuova env var e il default OFF.
5. Test integration: inviare N+1 richieste in 60s → la N+1 ottiene HTTP 429 con body Anthropic `{ "type": "error", "error": { "type": "rate_limit_error", ... } }` (usare `ProxyError::Upstream` o aggiungere variante dedicata).

**Criteri di successo**:
- Rate limit disattivato per default, non regredisce comportamento attuale.
- Test verifica 429 + formato errore Anthropic-compliant.

**No-go**: non persistere state su disco; in-memory è sufficiente per il MVP.

---

### P1-3 — JSON malformato in `tool_calls.arguments`

**File**: `src/transform.rs:465` (e `:322` se presente pattern simile)

**Problema**: `serde_json::from_str(&tool_call.function.arguments).unwrap_or_else(|_| json!({}))` droppa silenziosamente gli argomenti → client riceve tool call vuoto senza diagnostica.

**Intervento**:
1. Sostituire il fallback silente con:
   ```
   match serde_json::from_str::<Value>(&tool_call.function.arguments) {
       Ok(v) => v,
       Err(err) => {
           tracing::warn!(
               tool_id = %tool_call.id,
               tool_name = %tool_call.function.name,
               error = %err,
               "tool_call.arguments is not valid JSON, forwarding as empty object"
           );
           json!({})
       }
   }
   ```
2. Valutare se in `CompatMode::Strict` questo debba invece produrre un `ProxyError::Transform`. Implementare SOLO se la semantica di `Strict` lo giustifica (verificare in `config.rs`). Se dubbio: lasciare warn in entrambe le modalità e aprire una nota in CHANGELOG.
3. Test unitario: input con arguments `"{ invalid json"` → ritorno `json!({})` + log catturato via `tracing-test`.

**Criteri di successo**: test passa, no regressione sui test esistenti di `transform`.

---

### P1-4 — Graceful shutdown del task refresh model cache

**File**: `src/main.rs:110-115`

**Problema**: `tokio::spawn` loop `sleep(60s) + refresh` non osserva alcun segnale di shutdown.

**Intervento**:
1. In `main.rs`, creare un `tokio::sync::watch::channel::<bool>(false)` (`shutdown_tx`, `shutdown_rx`).
2. Installare handler per `SIGINT`/`SIGTERM` con `tokio::signal::unix` (se linux); su altri OS usare `ctrl_c()`. In caso di segnale → `shutdown_tx.send(true)`.
3. Il loop di refresh diventa:
   ```
   loop {
       tokio::select! {
           _ = tokio::time::sleep(Duration::from_secs(60)) => { refresh(...).await; }
           _ = shutdown_rx.changed() => { break; }
       }
   }
   ```
4. `axum::serve(...).with_graceful_shutdown(async move { shutdown_rx.changed().await.ok(); })`.

**Criteri di successo**:
- Test: avviare server in test helper, inviare shutdown signal, verificare che il processo esca entro 2s.
- Nessuna regressione su `cargo test`.

---

### P1-5 — Validazione CORS: rifiuto wildcard

**File**: `src/proxy.rs:959-980` (`build_cors_layer`)

**Intervento**:
1. In parsing di `allow_origins`, se un entry è `*` o contiene `*` → ritornare `anyhow::Error` con messaggio chiaro: «wildcard origins non supportate; usare reverse proxy per `*`».
2. Aggiornare `main.rs` per propagare l'errore al boot (già fa `?`).
3. Test: `build_cors_layer` con config `allow_origins = ["*"]` → err. Con `["https://example.com"]` → ok.

---

### P1-6 — Test di timeout

**File**: nuovo `tests/timeouts.rs` (o sezione in `tests/real_backends.rs` se già gated per env).

**Intervento**:
1. Mock server HTTP (usare `wiremock` o `hyper` dummy) che:
   - Scenario A: risponde dopo 310s (oltre client timeout 300s) → proxy deve restituire `ProxyError::Http` mappato a 504 o 503.
   - Scenario B: durante streaming SSE, chiude la connessione a metà → proxy deve chiudere lo stream lato client senza panic.
2. Test gated con `#[ignore]` se troppo lenti; altrimenti accorciare timeout via env (richiede Config override).
3. Valutare feature flag `test-timeouts` se Cargo CI non deve eseguirli di default.

**Criteri di successo**: test eseguibili con `cargo test -- --ignored`, no panic.

---

### P1-7 — Deduplicazione mapping `stop_reason`

**File**: `src/transform.rs:475-484` vs `:501-509` (`map_stop_reason`).

**Intervento**:
1. Verificare chi chiama `map_stop_reason`. Se nessuno → rimuovere.
2. Se usato altrove → usarlo anche in `openai_to_anthropic` sostituendo il match inline.
3. Mantenere test esistenti; aggiungere test table-driven se assente.

---

### P1-8 — Estrazione `ToolCallAccumulator`

**File**: `src/proxy.rs:284-550+` (`create_sse_stream`).

**Intervento**:
1. Creare struct `ToolCallAccumulator` in nuovo modulo `src/streaming.rs` (o mantenerlo privato in `proxy.rs` se più piccolo):
   - stato: `BTreeMap<usize, PartialToolCall>`.
   - metodi: `push_delta(&mut self, index, delta) -> Option<CompletedToolCall>`, `flush(&mut self) -> Vec<CompletedToolCall>`.
2. Rifattorizzare solo la sezione relativa ai tool calls di `create_sse_stream`. Non toccare UTF-8 buffering né think-tag filter (già separato).
3. Test unitari su `ToolCallAccumulator` con chunk frammentati (prendere casi da test esistenti in `proxy.rs #[cfg(test)]`).

**Attenzione**: questo è l'unico task "refactor" del piano. Se durante l'implementazione la diff si allarga oltre ~150 righe toccate, **fermarsi e chiedere review** prima di proseguire.

---

### P2-9 — Backoff + error escalation in `model_cache::refresh`

**File**: `src/model_cache.rs` (~60 righe attuali).

**Intervento**:
1. Aggiungere campo `consecutive_failures: AtomicU32` alla cache struct.
2. Se `refresh` fallisce → incrementa counter, log `warn!`. Se counter ≥ 5 → log `error!` una volta e reset.
3. Backoff: su fallimento, prossimo sleep = `min(60s * 2^failures, 600s)`. Questo richiede che il loop viva in `model_cache` (oppure espone la prossima durata via funzione).

**No-go**: niente retry interni al singolo refresh (il loop è già retry).

---

### P2-10 — `fallback_models`: decidere

**File**: `src/proxy.rs:604, 627-639`.

**Intervento**:
1. Verificare via grep se `fallback_models` è letto da qualche parte oltre la costruzione di `Config`.
2. Se non letto: **rimuovere** campo + env var + documentazione.
3. Se letto: aprire issue separata (non implementare fallback logic in questo piano).

---

### P2-11 — Per-chunk timeout nello streaming

**File**: `src/proxy.rs` (nel loop che consuma `bytes_stream`).

**Intervento**:
1. Wrappare ogni `stream.next().await` in `tokio::time::timeout(Duration::from_secs(30), …)`.
2. In caso di timeout: chiudere stream verso il client con event SSE di errore + log `warn!`.
3. Rendere il valore configurabile via env `ANTHMORPH_STREAM_CHUNK_TIMEOUT_SECS` (default 30).

---

### P2-12 — `docs/TROUBLESHOOTING.md`

**Contenuto minimo**:
- Come abilitare log debug (`RUST_LOG=anthmorph=debug,tower_http=debug`).
- Errori comuni: 401 (ingress vs backend auth), 429, 503 overloaded, thinking rifiutato in `Strict`.
- Flow di autenticazione: diagramma ASCII `client → (client_key? → config.api_key?) → backend`.
- Note per backend: Chutes (supports top_k, reasoning), MiniMax (timeouts generosi).

**No-go**: non duplicare contenuto già in `CLAUDE_CODE_SETUP.md` o `README.md`; linkare.

---

### P2-13 — Test refresh `model_cache`

**File**: nuovo `tests/model_cache.rs` o sezione in `src/model_cache.rs #[cfg(test)]`.

**Intervento**:
1. Mock HTTP client (`wiremock` o `hyper` locale).
2. Test: refresh success popola cache; refresh failure lascia cache precedente; concurrent read durante update non panica.

---

### P2-14 — Bootstrap timeout configurabile in `anthmorphctl`

**File**: `scripts/anthmorphctl` e `docs/CLAUDE_CODE_SETUP.md`.

**Intervento**:
1. Aggiungere flag `--timeout-ms <N>` al comando `bootstrap claude-code` (default invariato: `6000000`).
2. Salvare il valore in `.anthmorph/config.env` come `API_TIMEOUT_MS` per riutilizzo.
3. Aggiornare docs con esempio.

---

## 4. Sequenza di esecuzione proposta

```
Fase 1 (P0 — security/rate limiting)
  1. P0-1  → commit
  2. P0-2  → commit
  ── pausa per review ──

Fase 2 (P1 — correttezza/robustezza)
  3. P1-3
  4. P1-5
  5. P1-4
  6. P1-7
  7. P1-6
  8. P1-8  (pausa se diff > 150 righe)
  ── pausa per review ──

Fase 3 (P2 — qualità/DX)
  9. P2-10 (cleanup veloce)
  10. P2-9
  11. P2-11
  12. P2-13
  13. P2-14
  14. P2-12
  ── release v0.1.5 candidate ──
```

---

## 5. Gate di release (solo a fine Fase 2)

Prima di proporre un bump di versione a `0.2.0`:
- [ ] `cargo test` (tutti) verde
- [ ] `scripts/docker_rust_test.sh` verde
- [ ] `scripts/docker_secret_scan.sh` verde
- [ ] `scripts/test_claude_code_patterns_real.sh` su almeno un backend reale
- [ ] `CHANGELOG.md` aggiornato
- [ ] Versione sincronizzata in `Cargo.toml` + `package.json`

---

## 6. Cose espressamente FUORI scope

- Refactor completo di `create_sse_stream` oltre al task P1-8.
- Migrazione runtime MCP / cambi di stack.
- Supporto backend aggiuntivi (es. Bedrock, Vertex).
- Persistenza del rate limit o metriche Prometheus (aprire task separato).
- Modifiche a `prebuilt/` o release ARM64.

---

**Fine piano.** Non procedere con l'implementazione senza approvazione esplicita dell'utente.
