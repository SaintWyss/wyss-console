"""Real SDK: a conversation created in one bridge process is resumed in another."""
import json, shutil, sys, tempfile
import driver

venv = sys.argv[1]
root = tempfile.mkdtemp(prefix="wyss-resume-")
SESSION = "5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40"
try:
    import os; [os.makedirs(root + "/" + d, exist_ok=True) for d in ("home", "ws", "markers")]
    a = driver.BridgeSession(venv, script=[{"text": "first answer"}], root=root)
    a.recv(); a.send(c="configure", profile="inference", cwd="/work/ws", session=SESSION, broker_token="abababababababababababababababababababababababababababababababab")
    a.until(lambda e: e["e"] == "ready")
    a.send(c="prompt", turn="t1", text="remember the word PINEAPPLE")
    ev = a.until(lambda e: e["e"] == "turn_started")
    print("first provider session:", ev["provider_session"], "== minted:", ev["provider_session"] == SESSION)
    a.until(lambda e: e["e"] == "turn_idle"); a.send(c="shutdown"); print("rc a", a.close())
    b = driver.BridgeSession(venv, script=[{"text": "second answer"}], root=root)
    b.recv(); b.send(c="configure", profile="inference", cwd="/work/ws", session="9d7e2a10-1111-4222-8333-444455556666", broker_token="abababababababababababababababababababababababababababababababab")
    b.until(lambda e: e["e"] == "ready")
    b.send(c="bind", resume=SESSION)
    seen = []
    bound = b.until(lambda e: e["e"] == "bound", collect=seen)
    print("bound:", bound, "ready events during bind:", [e["e"] for e in seen])
    b.send(c="prompt", turn="t2", text="what was the word?")
    ts = b.until(lambda e: e["e"] == "turn_started")
    print("resumed provider session:", ts["provider_session"] == SESSION)
    b.until(lambda e: e["e"] == "turn_idle")
    b.send(c="shutdown"); rc = b.proc.wait(15)
    reqs = [json.loads(l) for l in open(root + "/markers/requests.jsonl")]
    gen = [r for r in reqs if r["path"].startswith("/v1/messages") and isinstance(r["body"], dict) and r["body"].get("stream")]
    carried = "PINEAPPLE" in json.dumps(gen[-1]["body"]["messages"])
    print("rc b", rc, "| earlier turn carried into the resumed request:", carried)
    b.close()
finally:
    shutil.rmtree(root, ignore_errors=True)
