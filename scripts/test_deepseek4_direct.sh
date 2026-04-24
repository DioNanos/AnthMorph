#!/bin/sh
set -eu

BASE_URL=${DEEPSEEK_BASE_URL:-https://api.deepseek.com}
API_KEY=${DEEPSEEK_API_KEY:-}

if [ -z "$API_KEY" ]; then
  echo "missing DEEPSEEK_API_KEY" >&2
  exit 1
fi

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

auth_header="Authorization: Bearer $API_KEY"

echo "== models =="
curl -fsS "$BASE_URL/v1/models" -H "$auth_header" | tee "$tmpdir/models.json" >/dev/null
python3 - <<'PY' "$tmpdir/models.json"
import json, sys
data = json.load(open(sys.argv[1]))
ids = [item["id"] for item in data.get("data", [])]
print("models:", ", ".join(ids))
assert "deepseek-v4-pro" in ids, "deepseek-v4-pro missing"
PY

echo "== non-stream =="
cat >"$tmpdir/nonstream.json" <<'JSON'
{
  "model": "deepseek-v4-pro",
  "messages": [
    {"role": "system", "content": "Reply exactly OK"},
    {"role": "user", "content": "Reply exactly OK"}
  ],
  "stream": false,
  "thinking": {"type": "enabled"},
  "reasoning_effort": "high",
  "max_tokens": 64
}
JSON
curl -fsS "$BASE_URL/chat/completions" \
  -H "$auth_header" \
  -H 'content-type: application/json' \
  -d @"$tmpdir/nonstream.json" | tee "$tmpdir/nonstream.out.json" >/dev/null
python3 - <<'PY' "$tmpdir/nonstream.out.json"
import json, sys
data = json.load(open(sys.argv[1]))
msg = data["choices"][0]["message"]
print("content:", msg.get("content"))
print("has_reasoning:", bool(msg.get("reasoning_content")))
PY

echo "== responses negative =="
status=$(curl -sS -o "$tmpdir/responses.out" -w '%{http_code}' "$BASE_URL/v1/responses" \
  -H "$auth_header" \
  -H 'content-type: application/json' \
  -d '{"model":"deepseek-v4-pro","input":"ping"}')
echo "responses status: $status"
cat "$tmpdir/responses.out"
echo

echo "== long tool name limit =="
python3 - <<'PY' > "$tmpdir/tool65.json"
import json
name = "x" * 65
payload = {
    "model": "deepseek-v4-pro",
    "messages": [{"role": "user", "content": "call the tool"}],
    "tools": [{
        "type": "function",
        "function": {
            "name": name,
            "description": "tool",
            "parameters": {"type": "object", "properties": {}}
        }
    }],
    "tool_choice": "auto",
    "stream": False
}
print(json.dumps(payload))
PY
status=$(curl -sS -o "$tmpdir/tool65.out" -w '%{http_code}' "$BASE_URL/chat/completions" \
  -H "$auth_header" \
  -H 'content-type: application/json' \
  -d @"$tmpdir/tool65.json")
echo "tool-65 status: $status"
cat "$tmpdir/tool65.out"
echo

echo "== anthropic long tool name limit =="
python3 - <<'PY' > "$tmpdir/anthropic65.json"
import json
name = "x" * 65
payload = {
    "model": "deepseek-v4-pro",
    "max_tokens": 64,
    "messages": [{"role": "user", "content": "call the tool"}],
    "tools": [{
        "name": name,
        "description": "tool",
        "input_schema": {"type": "object", "properties": {}}
    }]
}
print(json.dumps(payload))
PY
status=$(curl -sS -o "$tmpdir/anthropic65.out" -w '%{http_code}' "$BASE_URL/anthropic/v1/messages" \
  -H "$auth_header" \
  -H 'content-type: application/json' \
  -d @"$tmpdir/anthropic65.json")
echo "anthropic tool-65 status: $status"
cat "$tmpdir/anthropic65.out"
echo

echo "done"
