"""Stand-in system under test for OFFLINE harness tests (no real provider, no network beyond the
in-sandbox loopback mock). Each mode exercises one defect class.
Usage (inside the sandbox): fake_provider.py <mode> [-p prompt]
Modes: ok, tamper_requests, forge_stream, flood_stream, no_request, hang, flood_markers, bigfile,
       fill_home, spawn_orphan, tool_attempt
"""
import json
import os
import subprocess
import sys
import time
import urllib.request

mode = sys.argv[1]
run = os.environ.get("A0_RUN", "fake")
base = os.environ.get("ANTHROPIC_BASE_URL", "")


def post(body):
    req = urllib.request.Request(base + "/v1/messages", data=json.dumps(body).encode(),
                                 headers={"content-type": "application/json",
                                          "x-api-key": os.environ.get("ANTHROPIC_API_KEY", "")})
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read() or b"{}")


def listing(names):
    lines = "".join(f"\n- {n}: A0 fixture skill {n[3:]}. CANARY-DESC-{n[3:]}-{run}" for n in names)
    return {"model": "fake", "messages": [{"role": "user", "content": [{"type": "text", "text":
            "The following skills are available for use with the Skill tool:" + lines + "\n\nping"}]}]}


names = [n for n in os.environ.get("A0_FAKE_LIST", "a0-alpha,a0-proj").split(",") if n]
if mode in ("ok", "tamper_requests", "forge_stream", "tool_attempt"):
    post(listing(names))
    if mode == "tool_attempt":
        post({"model": "fake", "messages": [{"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "toolu_mock_0", "is_error": True,
             "content": "Permission to use Bash with command rm /work/a0-victim has been denied."}]}]})
    print("ok from mock")
if mode == "tamper_requests":
    p = "/markers/requests.jsonl"
    data = open(p).read().replace("CANARY-DESC-alpha", "CANARY-DESC-XXXXX")
    open(p, "w").write(data)
if mode == "forge_stream":
    # try to inject a fake evidence record into the harness's stdout via /proc (same UID)
    try:
        with open(f"/proc/{os.getppid()}/fd/1", "w") as f:
            f.write('A0REC 3 {"kind":"END"} ' + "0" * 64 + "\n")
        print("forge: wrote", file=sys.stderr)
    except OSError as e:
        print(f"forge: blocked errno={e.errno}", file=sys.stderr)
if mode == "flood_stream":
    try:
        with open(f"/proc/{os.getppid()}/fd/1", "w") as f:
            line = "x" * 1023 + "\n"
            for _ in range(100 * 1024):
                f.write(line)
    except OSError as e:
        print(f"flood_stream: errno={e.errno}", file=sys.stderr)
if mode == "no_request":
    print("did nothing")
if mode == "hang":
    time.sleep(3600)
if mode == "flood_markers":
    chunk = b"x" * (1024 * 1024)
    for i in range(10_000):
        with open(f"/markers/flood-{i}", "wb") as f:
            f.write(chunk)
if mode == "bigfile":
    try:
        with open("/home/probe/big", "wb") as f:
            for _ in range(80):
                f.write(b"y" * (1024 * 1024))
        print("bigfile: wrote 80 MiB")
    except OSError as e:
        print(f"bigfile: errno={e.errno}")
if mode == "fill_home":
    written = 0
    try:
        for i in range(10_000):
            with open(f"/home/probe/fill-{i}", "wb") as f:
                f.write(b"z" * (32 * 1024 * 1024))
            written += 32
        print("fill_home: no limit hit")
    except OSError as e:
        print(f"fill_home: errno={e.errno} after_mib={written}")
if mode == "spawn_orphan":
    subprocess.Popen(["/usr/bin/sleep", "30"], start_new_session=True,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(float(os.environ.get("A0_ORPHAN_HOLD", "3")))
    print("spawned orphan")
sys.exit(0)
