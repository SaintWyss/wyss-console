import json, sys, time
import driver
venv = sys.argv[1]
SESSION = "5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40"

def run(name, script, interrupt_after_text=False):
    s = driver.BridgeSession(venv, script=script)
    seen = []
    try:
        s.recv()
        s.send(c="configure", profile="inference", cwd="/work/ws", session=SESSION, upstream="http://127.0.0.1:18080")
        s.until(lambda e: e["e"] == "ready")
        s.send(c="prompt", turn="t1", text="hi")
        if interrupt_after_text:
            s.until(lambda e: e["e"] == "text", collect=seen)
            s.send(c="interrupt", turn="t1")
        s.until(lambda e: e["e"] == "turn_idle", collect=seen, timeout=120)
        print("==", name)
        for e in seen:
            if e["e"] not in ("tap_request", "text", "usage"):
                print("  ", json.dumps(e)[:420])
        s.send(c="shutdown")
    finally:
        s.close()

run("http 529", [{"http_error": 529}, {"text": "recovered"}])
run("http 401", [{"http_error": 401}])
run("interrupt", [{"text": "x" * 4000, "delay": 0.05}], interrupt_after_text=True)
