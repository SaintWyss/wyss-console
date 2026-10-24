import json, os, sys, time
import driver
venv = sys.argv[1]

def scenario(name, script, decide):
    s = driver.BridgeSession(venv, script=script)
    seen = []
    try:
        s.recv()
        s.send(c="configure", profile="local_tools", cwd="/work/ws", session="5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40", broker_token="abababababababababababababababababababababababababababababababab")
        ready = s.until(lambda e: e["e"] in ("ready", "fatal"))
        att = ready.get("attestation", {})
        print("[%s] ready mcp_tools=%s mcp=%s" % (name, att.get("mcp_tools"), att.get("mcp_servers")))
        s.send(c="prompt", turn="t1", text="do the thing")
        while True:
            ev = s.until(lambda e: e["e"] in ("action_proposal", "turn_idle"), collect=seen)
            if ev["e"] == "turn_idle":
                break
            print("[%s] PROPOSAL %s %s" % (name, ev["tool"], json.dumps(ev["args"])))
            s.send(c="action_result", action=ev["action"], **decide(ev))
        for e in seen:
            if e["e"] in ("tap_request", "tap_violation", "turn_started", "tool_use", "notice", "error", "turn_result"):
                d = dict(e); 
                if e["e"] == "turn_started": d = {"e": "turn_started", "tools": e["init"]["tools"], "mcp": e["init"]["mcp_servers"]}
                print("   ", json.dumps(d)[:300])
        print("[%s] markers=%s" % (name, sorted(os.listdir(s.markers))))
    finally:
        s.send(c="shutdown"); print("rc", s.close())

cmd = {"tool_use": {"name": "mcp__wyss__run_command", "input": {"command": "touch /markers/NOT_FROM_PROVIDER"}}}
scenario("approve", [cmd, {"text": "finished"}], lambda ev: {"outcome": "completed", "output": "authority ran it"})
scenario("deny", [cmd, {"text": "ok denied"}], lambda ev: {"outcome": "denied"})
scenario("builtin-bypass", [{"tool_use": {"name": "Bash", "input": {"command": "touch /markers/bash_executed"}}}, {"text": "x"}], lambda ev: {"outcome": "denied"})
