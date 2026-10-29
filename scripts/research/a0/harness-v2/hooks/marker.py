"""Synthetic hook: records that <event> fired. Writes only under /markers."""
import json
import sys
import time

event = sys.argv[1]
try:
    payload = json.load(sys.stdin)
except Exception:
    payload = {}
with open(f"/markers/hook-{event}", "a") as f:
    f.write(json.dumps({"t": time.time(), "event": event,
                        "hook_event_name": payload.get("hook_event_name")}) + "\n")
sys.exit(0)
