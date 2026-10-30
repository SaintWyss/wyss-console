"""Focused qualification of Arsenal skill exclusion on Claude Code 2.1.314.

Runs the A0 harness-v2 (copied into a fresh run root, binary pinned to 2.1.314) with the exact
overlay shape Arsenal emits: a --settings file holding only `skillOverrides` with `off` values.
Synthetic key, loopback mock, bwrap --unshare-all, no real HOME. Cases are written to
PRE-REGISTERED.json before the first launch. Usage: python3 -I qualify_314.py <RUN_ROOT>
"""
import json
import os
import sys

ROOT = os.path.realpath(sys.argv[1])
sys.path.insert(0, os.path.join(ROOT, "harness-v2"))
from run_case import run_case  # noqa: E402

ALL = ["alpha", "beta", "gamma", "proj"]


def exclusion(off):
    return {"skillOverrides": {f"a0-{n}": "off" for n in off}}


def listed_case(cid, off, mode="H", settings="plain"):
    on = [n for n in ALL if n not in off]
    case = {
        "id": cid,
        "mode": mode,
        "settings": settings,
        "expect": {
            "requests": (1, 6),
            "listed_fixture_exact": [f"a0-{n}" for n in on],
            "desc_present": on,
            "desc_absent": off + ["hidden"],
            "body_absent": ALL,
            "agents": True,
        },
    }
    if off is not None:
        case["overlay"] = exclusion(off)
        case["args"] = ["--settings", "{OVERLAY}"]
        case["expect"]["validator"] = True
    if mode == "I":
        case["startup_wait"] = 15
        case["prompt"] = "ping"
        case["expect"].pop("body_absent")
    return case


CASES = [
    # Control: no overlay, every fixture skill is listed (the mechanism is what removes them).
    {**listed_case("Q-C0", []), "overlay": None, "args": []},
    *[listed_case(f"Q-S1-{i}", ["beta", "proj"]) for i in (1, 2, 3)],
    *[listed_case(f"Q-S2-{i}", ["alpha", "gamma"]) for i in (1, 2, 3)],
    # The owner's hooks keep firing under an Arsenal overlay.
    {
        **listed_case("Q-H1", ["beta", "proj"], settings="hooked"),
        "expect": {
            **listed_case("Q-H1", ["beta", "proj"])["expect"],
            "markers_present": ["hook-SessionStart", "hook-UserPromptSubmit"],
        },
    },
    # Native terminals are interactive: the same exclusion in the TUI.
    listed_case("Q-S1-I", ["beta", "proj"], mode="I"),
]
for case in CASES:
    if case.get("overlay") is None:
        case.pop("overlay", None)
        case["expect"].pop("validator", None)
        case["args"] = []

os.makedirs(os.path.join(ROOT, "evidence"), mode=0o700, exist_ok=True)
prereg = os.path.join(ROOT, "evidence", "PRE-REGISTERED.json")
if not os.path.exists(prereg):
    json.dump(CASES, open(prereg, "w"), indent=1, default=list)

results = []
for case in CASES:
    r = run_case(ROOT, os.path.join(ROOT, "evidence"), case, allow_provider=True)
    results.append(
        {
            "id": case["id"],
            "verdict": r.get("verdict"),
            "reasons": r.get("reasons"),
            "listed": (r.get("judgement") or {}).get("checks"),
            "exit": (r.get("launcher") or {}).get("exit"),
            "proxy": r.get("proxy"),
        }
    )
    print(case["id"], r.get("verdict"), r.get("reasons"), flush=True)
json.dump(results, open(os.path.join(ROOT, "evidence", "MATRIX.json"), "w"), indent=1)
