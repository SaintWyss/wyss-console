#!/usr/bin/env python3
"""M3 on the real desktop, isolated: the fixture Claude Managed window on a display of its own.

    python3 -I benchmarks/m3_gui.py --binary src-tauri/target/debug/examples/m3_fixture_gui \
        --sdk-root <venv> --scenario inference --out docs/acceptance/evidence/m3-gui-2026-10-23

What runs is the real application window (Tauri/WebKit) with the real Claude Agent SDK and Claude
Code CLI behind it, against a mock provider API on loopback. Everything is made for the run: a
nested kwin_wayland + Xwayland on a display that must not exist, a session bus, HOME, XDG
directories, runtime directory and workspace. No installed Console, authority, host, provider,
credential, keyring or display of the owner is named or touched. Each part is a direct child
ended through the handle it got at launch (`owned.py`); nothing is found by its name.

A scenario is a list of steps (click, type, key, wait, shot, expect_file, expect_no_file,
expect_mock) read from `m3_gui_scenarios/<name>.json`; the harness checks the side effects on
disk (the workspace, the mock provider's request log) and takes pictures of the screen.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import m1_gui  # noqa: E402  (the nested display, the pointer and the picture helpers)
from owned import NotOurs, Owned  # noqa: E402

BASE_PATH = m1_gui.BASE_PATH
DISPLAY = m1_gui.DISPLAY
SCREEN = m1_gui.SCREEN


def mock_requests(state):
    path = os.path.join(state, "mock-requests.jsonl")
    if not os.path.exists(path):
        return []
    return [json.loads(l) for l in open(path) if l.strip()]


def generations(state):
    return [r for r in mock_requests(state) if r["path"].startswith("/v1/messages") and isinstance(r.get("body"), dict) and r["body"].get("stream")]


def windows(x_env):
    """Every window the private display holds: id, name, geometry. Diagnostic only."""
    rows = []
    found = subprocess.run(["/usr/bin/xdotool", "search", "--name", ""], env=x_env, capture_output=True, text=True, timeout=15).stdout.split()
    for wid in found:
        name = subprocess.run(["/usr/bin/xdotool", "getwindowname", wid], env=x_env, capture_output=True, text=True, timeout=15).stdout.strip()
        geo = subprocess.run(["/usr/bin/xdotool", "getwindowgeometry", wid], env=x_env, capture_output=True, text=True, timeout=15).stdout.split()
        rows.append({"id": wid, "name": name, "geometry": " ".join(geo)})
    return rows


def run_steps(steps, ctx):
    results = []
    for n, step in enumerate(steps):
        kind = step[0]
        ok = True
        detail = ""
        if kind == "click":
            ctx["pointer"].do(f"mousemove {step[1]} {step[2]} click 1")
        elif kind == "key":
            ctx["pointer"].do("key " + step[1])
        elif kind == "type":
            ctx["pointer"].do("type --delay 40 " + step[1])
        elif kind == "scroll":
            ctx["pointer"].do(f"mousemove 860 800 click --repeat {step[1]} --delay 40 {5 if step[1] > 0 else 4}".replace("--repeat -", "--repeat "))
        elif kind == "wait":
            time.sleep(step[1])
        elif kind == "shot":
            m1_gui.shot(ctx["x_env"], os.path.join(ctx["out"], f"{ctx['scenario']}-{step[1]}.png"))
        elif kind == "expect_file":
            path = os.path.join(ctx["ws"], step[1])
            end = time.monotonic() + (step[3] if len(step) > 3 else 30)
            want = step[2] if len(step) > 2 else None

            def holds():
                return os.path.exists(path) and (want is None or open(path).read() == want)

            while time.monotonic() < end and not holds():
                time.sleep(0.1)
            ok = holds()
            detail = os.path.relpath(path, ctx["ws"])
        elif kind == "expect_no_file":
            time.sleep(0.8)
            ok = not os.path.exists(os.path.join(ctx["ws"], step[1]))
            detail = step[1]
        elif kind == "expect_generations":
            end = time.monotonic() + (step[2] if len(step) > 2 else 30)
            while time.monotonic() < end and len(generations(ctx["state"])) < step[1]:
                time.sleep(0.2)
            ok = len(generations(ctx["state"])) == step[1]
            detail = f"{len(generations(ctx['state']))} generation requests"
        elif kind == "expect_tools":
            tools = [t["name"] for t in generations(ctx["state"])[-1]["body"].get("tools", [])] if generations(ctx["state"]) else None
            ok = tools is not None and sorted(tools) == sorted(step[1])
            detail = f"tools offered: {tools}"
        elif kind == "expect_tool_result":
            body = json.dumps(generations(ctx["state"])[-1]["body"]["messages"]) if generations(ctx["state"]) else ""
            ok = step[1] in body
            detail = step[1]
        else:
            raise SystemExit(f"unknown step {kind}")
        results.append({"n": n, "step": step, "ok": ok, "detail": detail})
        if not ok:
            print(f"step {n} {step} FAILED {detail}", flush=True)
    return results


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True)
    ap.add_argument("--sdk-root", required=True)
    ap.add_argument("--scenario", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--bare", action="store_true", help="start the binary with no arguments (the plain application)")
    ap.add_argument("--hold", type=float, default=0.0, help="keep the window up this many seconds after the steps")
    args = ap.parse_args()
    binary = os.path.realpath(args.binary)
    sdk_root = os.path.realpath(args.sdk_root)
    scenario = json.load(open(os.path.join(HERE, "m3_gui_scenarios", args.scenario + ".json")))
    os.makedirs(args.out, exist_ok=True)
    root = tempfile.mkdtemp(prefix="m3gui-")
    os.chmod(root, 0o700)
    for sub in ("run", "state", "config", "data", "cache", "home", "logs", "ws"):
        os.makedirs(os.path.join(root, sub), mode=0o700, exist_ok=True)
    state = os.path.join(root, "state")
    logs = os.path.join(root, "logs")
    script_path = os.path.join(root, "mock-script.json")
    json.dump(scenario["mock"], open(script_path, "w"))
    for name, content in scenario.get("workspace", {}).items():
        with open(os.path.join(root, "ws", name), "w") as f:
            f.write(content)
    nest = app = pointer = None
    result = {"scenario": args.scenario, "binary": os.path.relpath(binary, os.path.dirname(HERE)), "display": DISPLAY, "screen": SCREEN, "steps": [],
              "loadavg": open("/proc/loadavg").read().split()[:3]}
    code = 1
    try:
        nest = m1_gui.Nest(root, logs)
        x_env = {"PATH": BASE_PATH, "DISPLAY": DISPLAY, "HOME": os.path.join(root, "nest-home")}
        app_env = {
            "PATH": BASE_PATH, "HOME": os.path.join(root, "home"),
            "XDG_RUNTIME_DIR": os.path.join(root, "run"), "XDG_STATE_HOME": state,
            "XDG_CONFIG_HOME": os.path.join(root, "config"), "XDG_DATA_HOME": os.path.join(root, "data"),
            "XDG_CACHE_HOME": os.path.join(root, "cache"), "DISPLAY": DISPLAY, "GDK_BACKEND": "x11",
            "NO_AT_BRIDGE": "1", "GTK_USE_PORTAL": "0", "XDG_CURRENT_DESKTOP": "KDE", "KDE_SESSION_VERSION": "6",
            "LANG": "C.UTF-8", "SHELL": os.environ.get("SHELL", "/bin/sh"), "DBUS_SESSION_BUS_ADDRESS": nest.bus("app-bus"),
        }
        argv = [binary] if args.bare else [binary, "--sdk-root", sdk_root, "--state", state, "--workspace", os.path.join(root, "ws"), "--script", script_path]
        log = open(os.path.join(logs, "app.log"), "wb")
        app = Owned(argv, binary, env=app_env, log=log)
        time.sleep(float(scenario.get("boot", 8)))
        result["windows_before"] = windows(x_env)
        m1_gui.place_window(x_env)
        time.sleep(1.5)
        result["windows_after"] = windows(x_env)
        pointer = m1_gui.Pointer(x_env)
        ctx = {"pointer": pointer, "x_env": x_env, "out": args.out, "scenario": args.scenario, "ws": os.path.join(root, "ws"), "state": state}
        result["steps"] = run_steps(scenario["steps"], ctx)
        if args.hold:
            time.sleep(args.hold)
        result["generations"] = len(generations(state))
        result["alive_at_end"] = app.alive()
        code = 0 if all(s["ok"] for s in result["steps"]) else 2
    except NotOurs as why:
        print(f"ABORT: {why}", file=sys.stderr)
    finally:
        if pointer:
            pointer.close()
        if app:
            app.adopt_tree()
            app.stop()
        if nest:
            nest.stop()
        result["leftovers"] = m1_gui.leftovers(root)
        with open(os.path.join(args.out, f"{args.scenario}.json"), "w") as f:
            json.dump(result, f, indent=1, default=str)
        for name in ("app.log",):
            src = os.path.join(logs, name)
            if os.path.exists(src):
                shutil.copy(src, os.path.join(args.out, f"{args.scenario}.{name}"))
        mounted = [l.split()[1] for l in open("/proc/mounts") if l.split()[1].startswith(root)]
        if mounted:
            print(f"left {root}: still mounted {mounted}", file=sys.stderr)
        else:
            shutil.rmtree(root, ignore_errors=True)
    return code


if __name__ == "__main__":
    sys.exit(main())
