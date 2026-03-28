#!/bin/sh
set -eu

BACKEND=${1:?usage: ./scripts/smoke_test.sh <chutes|alibaba|minimax>}
PORT=${SMOKE_PORT:-3101}
PROMPT=${SMOKE_PROMPT:-Reply with exactly: anthmorph-smoke-ok}
LOG_FILE=${TMPDIR:-/tmp}/anthmorph-smoke-${BACKEND}-${PORT}.log
RESPONSE_FILE=${TMPDIR:-/tmp}/anthmorph-smoke-${BACKEND}-${PORT}.json

case "$BACKEND" in
  chutes)
    PROFILE=chutes
    BACKEND_URL=${CHUTES_BASE_URL:-https://llm.chutes.ai/v1}
    MODEL=${CHUTES_MODEL:-Qwen/Qwen3.5-397B-A17B-TEE,zai-org/GLM-5-TEE,deepseek-ai/DeepSeek-V3.2-TEE}
    API_KEY=${CHUTES_API_KEY:?CHUTES_API_KEY is required}
    ;;
  alibaba)
    PROFILE=openai-generic
    BACKEND_URL=${ALIBABA_BASE_URL:-https://coding-intl.dashscope.aliyuncs.com/v1}
    MODEL=${ALIBABA_MODEL:-qwen3-coder-plus}
    API_KEY=${ALIBABA_CODE_API_KEY:?ALIBABA_CODE_API_KEY is required}
    ;;
  minimax)
    PROFILE=openai-generic
    BACKEND_URL=${MINIMAX_BASE_URL:-https://api.minimax.io/v1}
    MODEL=${MINIMAX_MODEL:-MiniMax-M2.5}
    API_KEY=${MINIMAX_API_KEY:?MINIMAX_API_KEY is required}
    ;;
  *)
    echo "unsupported backend: $BACKEND" >&2
    exit 2
    ;;
esac

cleanup() {
  if [ -n "${SERVER_PID:-}" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

if [ ! -x ./target/debug/anthmorph ]; then
  cargo build --quiet
fi

./target/debug/anthmorph   --port "$PORT"   --backend-profile "$PROFILE"   --backend-url "$BACKEND_URL"   --model "$MODEL"   --api-key "$API_KEY"   >"$LOG_FILE" 2>&1 &
SERVER_PID=$!

READY=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if curl -fsS "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 1
done

if [ "$READY" -ne 1 ]; then
  echo "server did not become ready; log follows:" >&2
  cat "$LOG_FILE" >&2
  exit 1
fi

PAYLOAD=$(cat <<EOF
{"model":"claude-sonnet-4","max_tokens":128,"messages":[{"role":"user","content":"$PROMPT"}]}
EOF
)

curl -fsS "http://127.0.0.1:$PORT/v1/messages"   -H 'content-type: application/json'   -d "$PAYLOAD"   >"$RESPONSE_FILE"

cat "$RESPONSE_FILE"
