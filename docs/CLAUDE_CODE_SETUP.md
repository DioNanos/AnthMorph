# Claude Code Setup

This guide connects Claude Code to a local AnthMorph instance.

## Quick path

1. Install AnthMorph.

```bash
npm install -g @mmmbuto/anthmorph
```

2. Initialize a backend profile.

```bash
export CHUTES_API_KEY=your_key_here
anthmorphctl init chutes --port 3107
```

3. Start the proxy.

```bash
anthmorphctl start
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

## What AnthMorph writes

The bootstrap command prepares Claude Code with:

- `ANTHROPIC_BASE_URL=http://127.0.0.1:$PORT`
- `ANTHROPIC_AUTH_TOKEN` from `INGRESS_API_KEY` if configured, otherwise `anthmorph-local`
- all Claude Code default model variables pointed at `PRIMARY_MODEL`
- `API_TIMEOUT_MS=6000000`

## Recommended runtime mode

For Claude Code CLI usage, prefer `compat` mode.

Example:

```bash
anthmorphctl init chutes --compat-mode compat
```

## Notes by backend

- `chutes`: preserves Chutes-specific strengths like `top_k` and reasoning-aware routing
- `openai_generic`: accepts Claude Code request shapes conservatively and suppresses backend-native reasoning noise by default

## Verification

Basic health check:

```bash
curl -fsS http://127.0.0.1:3107/health
```

Real Claude Code payload replay:

```bash
./scripts/test_claude_code_patterns_real.sh chutes
./scripts/test_claude_code_patterns_real.sh minimax
```
