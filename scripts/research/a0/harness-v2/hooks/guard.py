"""Synthetic blocking PreToolUse guard (models a mandatory security hook).
Blocks any Bash command containing A0-FORBIDDEN with exit code 2 (Claude Code's documented
blocking code) and records the decision under /markers. Everything else passes.
"""
import json
import sys
import time

try:
    payload = json.load(sys.stdin)
except Exception:
    payload = {}
cmd = ((payload.get("tool_input") or {}).get("command")) or ""
rec = {"t": time.time(), "tool": payload.get("tool_name"), "has_token": "A0-FORBIDDEN" in cmd}
with open("/markers/guard-seen", "a") as f:
    f.write(json.dumps(rec) + "\n")
if "A0-FORBIDDEN" in cmd:
    with open("/markers/guard-denied", "a") as f:
        f.write(json.dumps(rec) + "\n")
    print("A0 guard: command blocked by synthetic security hook", file=sys.stderr)
    sys.exit(2)
sys.exit(0)
