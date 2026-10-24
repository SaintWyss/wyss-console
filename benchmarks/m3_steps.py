"""Checks about the Claude Managed fixture that a desktop scenario can make about the disk and the
mock provider's request log (the pictures say what the window shows; these say what happened).

Shared by `m3_gui.py` (the Managed window alone) and `m1_gui.py` (the whole application, Native and
Managed together). A check returns `(ok, detail)`; it never raises for a failed expectation.
"""
import json
import os
import time


def mock_requests(state):
    path = os.path.join(state, "mock-requests.jsonl")
    if not os.path.exists(path):
        return []
    with open(path) as f:
        return [json.loads(l) for l in f if l.strip()]


def generations(state):
    return [r for r in mock_requests(state)
            if r["path"].startswith("/v1/messages") and isinstance(r.get("body"), dict) and r["body"].get("stream")]


def _wait(predicate, seconds):
    end = time.monotonic() + seconds
    while time.monotonic() < end and not predicate():
        time.sleep(0.1)
    return predicate()


def check(step, ctx):
    """`step` is `[kind, *args]` with `kind` in the set below; `ctx` has `state` and `ws`."""
    kind = step[0]
    if kind == "expect_file":
        path = os.path.join(ctx["ws"], step[1])
        want = step[2] if len(step) > 2 else None

        def holds():
            return os.path.exists(path) and (want is None or open(path).read() == want)
        ok = _wait(holds, step[3] if len(step) > 3 else 30)
        return ok, os.path.relpath(path, ctx["ws"])
    if kind == "expect_no_file":
        time.sleep(0.8)
        return not os.path.exists(os.path.join(ctx["ws"], step[1])), step[1]
    if kind == "expect_generations":
        ok = _wait(lambda: len(generations(ctx["state"])) >= step[1], step[2] if len(step) > 2 else 30)
        n = len(generations(ctx["state"]))
        return ok and n == step[1], f"{n} generation requests"
    if kind == "expect_tools":
        gens = generations(ctx["state"])
        tools = [t["name"] for t in gens[-1]["body"].get("tools", [])] if gens else None
        return tools is not None and sorted(tools) == sorted(step[1]), f"tools offered: {tools}"
    if kind == "expect_tool_result":
        gens = generations(ctx["state"])
        body = json.dumps(gens[-1]["body"]["messages"]) if gens else ""
        return step[1] in body, step[1]
    if kind == "expect_no_path":
        # Nothing of this name anywhere under the run's own root (e.g. the usage history's directory).
        found = [os.path.join(d, n) for d, dirs, files in os.walk(ctx["root"]) for n in dirs + files if n == step[1]]
        return not found, f"{step[1]}: {len(found)} found"
    raise ValueError(f"unknown M3 check {kind}")
