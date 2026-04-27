# Legacy Claude Code Notes

AnthMorph 0.2.x is a Codex Responses-native daemon. The supported public generation ingress is:

```text
POST /v1/responses
```

Claude-style `/v1/messages` ingress is not part of the 0.2.x public runtime surface. Older helper code and tests may remain in the tree while the project transitions, but new client setup should target Codex or codex-vl with `wire_api = "responses"`.

For current setup, use the root [README](../README.md).
