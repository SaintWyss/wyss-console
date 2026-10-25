import json, sys, time
import driver
venv = sys.argv[1]
profile = sys.argv[2] if len(sys.argv) > 2 else "inference"
script = [{"text": "hello from the model, " * 5}]
s = driver.BridgeSession(venv, script=script)
try:
    t0 = time.monotonic()
    print("hello", s.recv()); 
    s.send(c="configure", profile=profile, cwd="/work/ws", session="5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40", broker_token="abababababababababababababababababababababababababababababababab")
    ready = s.until(lambda e: e["e"] in ("ready", "fatal"))
    print("ready after %.2fs" % (time.monotonic() - t0)); print(json.dumps(ready)[:2500])
    seen = []
    s.send(c="prompt", turn="t1", text="say hi")
    s.until(lambda e: e["e"] == "turn_idle", collect=seen)
    for e in seen: print(json.dumps(e)[:400])
    s.send(c="shutdown")
    print("rc", s.close()); print(b"".join(s.stderr)[-500:])
except Exception as e:
    print("ERR", repr(e)); print(b"".join(s.stderr)[-1500:]); s.close()
