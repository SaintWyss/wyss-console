#!/usr/bin/env python3
"""Runs every M3 desktop scenario on its own private display and writes one summary.

    python3 -I benchmarks/m3_gui_suite.py --binary <m3_fixture_gui> --sdk-root <verified venv> \
        --unverifiable-root <venv the verifier must refuse> --out .artifacts/benchmarks/m3-gui-...

Each scenario starts the application afresh (nothing carries over), so a run can be repeated and
compared. A scenario passes when every expectation it makes about the disk and the mock provider's
request log held and nothing of it was left running.
"""
import argparse
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True)
    ap.add_argument("--sdk-root", required=True)
    ap.add_argument("--unverifiable-root", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--only", nargs="*")
    args = ap.parse_args()
    names = sorted(f[:-5] for f in os.listdir(os.path.join(HERE, "m3_gui_scenarios")) if f.endswith(".json"))
    if args.only:
        names = [n for n in names if n in args.only]
    os.makedirs(args.out, exist_ok=True)
    summary = {"scenarios": {}}
    for name in names:
        root = args.unverifiable_root if name == "refused" else args.sdk_root
        t0 = time.monotonic()
        run = subprocess.run([sys.executable, "-I", os.path.join(HERE, "m3_gui.py"), "--binary", args.binary,
                              "--sdk-root", root, "--scenario", name, "--out", args.out],
                             capture_output=True, text=True, timeout=600)
        record = {}
        try:
            record = json.load(open(os.path.join(args.out, name + ".json")))
        except (OSError, ValueError):
            pass
        failed = [s["step"] for s in record.get("steps", []) if not s["ok"]]
        summary["scenarios"][name] = {
            "exit": run.returncode, "steps": len(record.get("steps", [])), "failed_steps": failed,
            "leftovers": record.get("leftovers"), "seconds": round(time.monotonic() - t0, 1),
            "ok": run.returncode == 0 and not failed and record.get("leftovers") == [],
        }
        print(name, summary["scenarios"][name], flush=True)
    summary["all_ok"] = all(v["ok"] for v in summary["scenarios"].values())
    with open(os.path.join(args.out, "summary.json"), "w") as f:
        json.dump(summary, f, indent=1)
    return 0 if summary["all_ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
