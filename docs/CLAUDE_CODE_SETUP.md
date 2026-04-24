# Claude Code Setup

This guide connects Claude Code to a local AnthMorph instance.

## DeepSeek V4 Path

1. Install AnthMorph.

```bash
npm install -g @mmmbuto/anthmorph
```

2. Initialize the DeepSeek V4 profile.

```bash
anthmorphctl config bootstrap
anthmorphctl init deepseek4 --port 3108 --compat-mode compat
anthmorphctl key set deepseek4
anthmorphctl key test deepseek4
```

3. Install or restart the local service.

```bash
anthmorphctl service install
anthmorphctl service restart
anthmorphctl status
```

4. Generate Claude Code settings.

Print the payload:

```bash
anthmorphctl bootstrap claude-code
```

Write `~/.claude/settings.json` directly:

```bash
anthmorphctl bootstrap claude-code --write
```

## What AnthMorph Writes

The bootstrap command prepares Claude Code with:

- `ANTHROPIC_BASE_URL=http://127.0.0.1:$PORT`
- `ANTHROPIC_AUTH_TOKEN` from `INGRESS_API_KEY` if configured, otherwise the local bootstrap token
- Claude Code model variables pointed at the active AnthMorph profile
- `API_TIMEOUT_MS=6000000`

## Model Probe

Check the provider-returned model before relying on a profile:

```bash
anthmorphctl model probe deepseek4
```

The default `deepseek4` profile requests `deepseek-v4-pro[1m]` and enables strict validation. If DeepSeek returns a different model, AnthMorph reports the mismatch instead of silently downgrading.

## Notes By Backend

- `deepseek`: primary DeepSeek V4 backend with tool-name normalization and strict model checks
- `chutes`: OpenAI-compatible fallback profile for supported hosted models
- `openai_generic`: conservative generic compatibility profile

## Verification

Basic health check:

```bash
curl -fsS http://127.0.0.1:3108/health
```

Real Claude Code payload replay:

```bash
./scripts/test_claude_code_patterns_real.sh chutes
```
