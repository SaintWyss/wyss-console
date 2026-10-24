"""Records the bridge's real output (real SDK + real CLI + mock API) as JSON-lines fixtures that
the Rust codec tests parse. Usage: python3 capture_fixtures.py <sdk-venv> <out-dir>"""
import json, sys, os
import driver

venv, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)


def record(name, profile, script, decide=None, prompt="say hi", extra_cmds=()):
    s = driver.BridgeSession(venv, script=script)
    frames = []
    try:
        frames.append(s.recv())
        s.send(c="configure", profile=profile, cwd="/work/ws", session="5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40", upstream="http://127.0.0.1:18080")
        s.until(lambda e: e["e"] in ("ready", "fatal"), collect=frames)
        s.send(c="prompt", turn="turn-1", text=prompt)
        while True:
            ev = s.until(lambda e: e["e"] in ("action_proposal", "turn_idle"), collect=frames)
            if ev["e"] == "turn_idle":
                break
            s.send(c="action_result", action=ev["action"], **decide(ev))
        s.send(c="shutdown")
    finally:
        s.close()
    with open(os.path.join(out, name + ".jsonl"), "w") as f:
        for fr in frames:
            f.write(json.dumps(fr, sort_keys=True) + "\n")
    print(name, len(frames), "frames")


record("inference-turn", "inference", [{"text": "hello from the model. " * 3}])
cmd = {"tool_use": {"name": "mcp__wyss__run_command", "input": {"command": "ls -la"}}}
record("local-tools-approved", "local_tools", [cmd, {"text": "done"}], lambda ev: {"outcome": "completed", "output": "total 0"})
record("local-tools-denied", "local_tools", [cmd, {"text": "understood"}], lambda ev: {"outcome": "denied"})
wf = {"tool_use": {"name": "mcp__wyss__write_file", "input": {"path": "notes.txt", "content": "line one\nline two\n"}}}
record("local-tools-write-file", "local_tools", [wf, {"text": "written"}], lambda ev: {"outcome": "completed", "output": "ok"})
record("builtin-tool-demanded", "inference", [{"tool_use": {"name": "Bash", "input": {"command": "id"}}}, {"text": "no"}])
record("reroute", "inference", [{"text": "answered", "model": "claude-haiku-5-5"}])
