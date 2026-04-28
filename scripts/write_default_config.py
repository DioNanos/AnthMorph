#!/usr/bin/env python3
from __future__ import annotations

import os
import sys
from pathlib import Path


TEMPLATE = """# AnthMorph example user config
[server]
port = 3108
host = "127.0.0.1"
compat_mode = "compat"
stream_chunk_timeout_secs = 30

[profiles.responses_provider]
backend = "openai-generic"
upstream_api = "responses"
base_url = "https://api.example.com/v1"
model = "example/responses-model"
reasoning_model = "example/responses-model"
api_key_env = "ANTHMORPH_PROVIDER_API_KEY"
strict_model = true

[profiles.chat_provider]
backend = "openai-generic"
upstream_api = "chat-completions"
base_url = "https://api.example.com/v1"
model = "example/chat-model"
reasoning_model = "example/chat-model"
api_key_env = "ANTHMORPH_PROVIDER_API_KEY"

[runtime]
active_profile = "responses_provider"
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
