#!/usr/bin/env python3
from __future__ import annotations

import os
import sys
from pathlib import Path


TEMPLATE = """# AnthMorph canonical user config
[server]
port = 3108
host = "127.0.0.1"
compat_mode = "compat"
stream_chunk_timeout_secs = 30

[profiles.deepseek4]
backend = "deepseek"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-pro[1m]"
reasoning_model = "deepseek-v4-pro[1m]"
api_key_env = "DEEPSEEK_API_KEY"
deepseek_anthropic_backend = true
strict_model = true

[profiles.deepseek4-pro]
backend = "deepseek"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-pro"
reasoning_model = "deepseek-v4-pro"
api_key_env = "DEEPSEEK_API_KEY"
strict_model = true

[profiles.deepseek4-flash]
backend = "deepseek"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
reasoning_model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
strict_model = true

[profiles.chutes]
backend = "chutes"
base_url = "https://llm.chutes.ai/v1"
model = "Qwen/Qwen3.5-397B-A17B-TEE"
reasoning_model = "Qwen/Qwen3.5-397B-A17B-TEE"
api_key_env = "CHUTES_API_KEY"

[runtime]
active_profile = "deepseek4"
"""


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: write_default_config.py PATH", file=sys.stderr)
        return 1
    path = Path(sys.argv[1]).expanduser()
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        path.write_text(TEMPLATE)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
