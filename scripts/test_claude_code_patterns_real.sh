#!/bin/sh
set -eu

PROFILE=${1:-chutes}
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
PAYLOAD_DIR=${ANTHMORPH_CLAUDE_PAYLOAD_DIR:-/opt/claude-proxy/tests/payloads}
BIN_PATH=${ANTHMORPH_BIN:-$ROOT_DIR/target/release/anthmorph}
PORT=${ANTHMORPH_TEST_PORT:-3119}
LOG_FILE=${ANTHMORPH_TEST_LOG:-$ROOT_DIR/.anthmorph/claude-code-real.log}
PID_FILE=${ANTHMORPH_TEST_PID:-$ROOT_DIR/.anthmorph/claude-code-real.pid}

mkdir -p "$(dirname -- "$LOG_FILE")"

if [ ! -d "$PAYLOAD_DIR" ]; then
  echo "payload dir not found: $PAYLOAD_DIR" >&2
  exit 1
fi

if [ ! -x "$BIN_PATH" ]; then
  cargo build --release --quiet
  BIN_PATH=$ROOT_DIR/target/release/anthmorph
fi

case "$PROFILE" in
  chutes)
    : "${CHUTES_API_KEY:?missing CHUTES_API_KEY}"
    BACKEND_PROFILE=chutes
    BACKEND_URL=${CHUTES_BASE_URL:-https://llm.chutes.ai/v1}
    MODEL=${CHUTES_MODEL:-deepseek-ai/DeepSeek-V3.2-TEE}
    API_KEY=$CHUTES_API_KEY
    EXPECT_NO_THINK=0
    ;;
  minimax)
    : "${MINIMAX_API_KEY:?missing MINIMAX_API_KEY}"
    BACKEND_PROFILE=openai-generic
    BACKEND_URL=${MINIMAX_BASE_URL:-https://api.minimax.io/v1}
    MODEL=${MINIMAX_MODEL:-MiniMax-M2.5}
    API_KEY=$MINIMAX_API_KEY
    EXPECT_NO_THINK=1
    ;;
  *)
    echo "unsupported profile: $PROFILE" >&2
    exit 1
    ;;
esac

cleanup() {
  if [ -f "$PID_FILE" ]; then
    pid=$(cat "$PID_FILE" 2>/dev/null || true)
    if [ -n "${pid:-}" ]; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
    rm -f "$PID_FILE"
  fi
}
trap cleanup EXIT INT TERM

"$BIN_PATH" \
  --port "$PORT" \
  --backend-profile "$BACKEND_PROFILE" \
  --compat-mode compat \
  --backend-url "$BACKEND_URL" \
  --model "$MODEL" \
  --api-key "$API_KEY" \
  >"$LOG_FILE" 2>&1 &
echo $! > "$PID_FILE"

for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  if curl -fsS "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

PAYLOADS="
basic_request.json
content_blocks_text.json
content_blocks_mixed.json
conversation_3_system.json
conversation_2_followup.json
conversation_4_tools.json
tool_result.json
claude_code_adaptive_thinking.json
cache_control_request.json
documents_request.json
unknown_content_blocks.json
multi_tool_request.json
"

passed=0
quarantined=0

is_retryable() {
  case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
    *"maximum capacity"*|*"try again later"*|*"rate limit"*|*"temporarily unavailable"*|*"overloaded"*|*"timeout"*)
      return 0
      ;;
  esac
  return 1
}

for payload_name in $PAYLOADS; do
  payload_file=$PAYLOAD_DIR/$payload_name
  payload=$(sed "s|{{MODEL}}|$MODEL|g" "$payload_file")
  response_file=$(mktemp)
  status=$(curl -sS -N -o "$response_file" -w "%{http_code}" \
    "http://127.0.0.1:$PORT/v1/messages" \
    -H 'content-type: application/json' \
    -d "$payload")
  body=$(cat "$response_file")
  rm -f "$response_file"

  if [ "$status" != "200" ]; then
    if is_retryable "$body" || [ "$status" = "429" ] || [ "$status" -ge 500 ] 2>/dev/null; then
      echo "QUARANTINE $payload_name status=$status"
      quarantined=$((quarantined + 1))
      continue
    fi
    echo "FAIL $payload_name status=$status"
    printf '%s\n' "$body"
    exit 1
  fi

  printf '%s' "$body" | grep -q 'event: message_start' || {
    echo "FAIL $payload_name missing message_start"
    printf '%s\n' "$body"
    exit 1
  }
  printf '%s' "$body" | grep -q 'event: message_stop' || {
    echo "FAIL $payload_name missing message_stop"
    printf '%s\n' "$body"
    exit 1
  }
  if printf '%s' "$body" | grep -q '"choices"'; then
    echo "FAIL $payload_name leaked OpenAI wire format"
    printf '%s\n' "$body"
    exit 1
  fi
  if [ "$EXPECT_NO_THINK" = "1" ] && printf '%s' "$body" | grep -q '<think>'; then
    echo "FAIL $payload_name leaked <think> tags"
    printf '%s\n' "$body"
    exit 1
  fi

  echo "PASS $payload_name"
  passed=$((passed + 1))
done

echo "passed=$passed quarantined=$quarantined profile=$PROFILE"
