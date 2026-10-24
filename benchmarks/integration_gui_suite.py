#!/usr/bin/env python3
"""Runs the integrated (M2 + M3) desktop scenarios, each on its own private display, bus, keyring,
HOME and XDG roots, with fixture providers and fixture Native CLIs, and writes one summary.

    python3 -I benchmarks/integration_gui_suite.py --binary src-tauri/target/debug/wyss-console \
        --sdk-root <verified venv> --unverifiable-root <venv the verifier must refuse> --out <dir>

The binary is the real application, built with `--features m3-fixture` (which also embeds the
frontend: build it first). Every scenario is `benchmarks/m1_gui.py --steps …`: the steps click the
real window and also check what happened on disk, in the Native host's state and in the mock
provider's request log. A scenario passes when the harness exits 0 and leaves no process behind.
"""
import argparse
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
EVIDENCE = os.path.join(REPO, "docs", "acceptance", "evidence", "integration-gui-2026-10-24")
M2_EVIDENCE = os.path.join(REPO, "docs", "acceptance", "evidence", "m2-gui-2026-10-23")

# name, steps, script, extra arguments, which SDK root
SCENARIOS = [
    ("native-then-managed-inference", "native-then-managed-inference.json", "inference.json", [], "good"),
    ("native-codex-and-managed-local-tools", "native-codex-and-managed-local-tools.json", "local-tools-accept-decline.json",
     ["--m3-seed", "notes.txt=before\\n"], "good"),
    ("managed-cancel-turn", "managed-cancel-turn.json", "local-tools-cancel.json", [], "good"),
    ("managed-recovery-after-stop", "managed-recovery-after-stop.json", "local-tools-recovery.json", [], "good"),
    ("m2-edit-undo-then-managed-inference", "m2-edit-undo-then-managed-inference.json", "inference.json", [], "good"),
    ("managed-refused-native-still-works", "managed-refused-native-still-works.json", "refused.json", [], "bad"),
    # M2's own acceptance scenarios, unchanged, on the integrated binary (no Managed fixture involved).
    ("m2-native-preview-start", "m2:native-preview-start-steps.json", None, [], "good"),
    ("m2-native-restart", "m2:native-restart-steps.json", None, [], "good"),
    ("m2-config-edit-cancel", "m2:config-edit-cancel-steps.json", None, [], "good"),
    ("m2-config-edit-apply", "m2:config-edit-apply-steps.json", None, [], "good"),
    ("m2-config-edit-undo-apply", "m2:config-edit-undo-apply-steps.json", None, [], "good"),
    ("m2-config-template-export", "m2:config-template-export-steps.json", None, [], "good"),
    ("m2-config-template-import-undo", "m2:config-template-import-undo-steps.json", None, [], "good"),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True)
    ap.add_argument("--sdk-root", required=True)
    ap.add_argument("--unverifiable-root", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--only", nargs="*")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    summary = {"scenarios": {}}
    for number, (name, steps, script, extra, which) in enumerate(SCENARIOS, start=1):
        if args.only and name not in args.only:
            continue
        out = os.path.join(args.out, name)
        os.makedirs(out, exist_ok=True)
        root = args.sdk_root if which == "good" else args.unverifiable_root
        started = time.monotonic()
        steps_path = os.path.join(M2_EVIDENCE, steps[3:]) if steps.startswith("m2:") else os.path.join(EVIDENCE, steps)
        managed = ["--m3-fixture", "--m3-sdk-root", root, "--m3-script", os.path.join(EVIDENCE, "scripts", script)] if script else []
        run = subprocess.run(
            [sys.executable, "-I", os.path.join(HERE, "m1_gui.py"), "--binary", args.binary, "--shape", "none",
             "--run", str(number), "--out", out, "--steps", steps_path, "--m2-fixtures", *managed, *extra],
            capture_output=True, text=True, timeout=1200)
        with open(os.path.join(out, "harness.log"), "w") as f:
            f.write(run.stdout[-20000:] + run.stderr[-20000:])
        record = {}
        try:
            record = json.load(open(os.path.join(out, f"run{number}.json")))
        except (OSError, ValueError):
            pass
        summary["scenarios"][name] = {
            "exit": run.returncode, "leftovers": record.get("leftovers"),
            "seconds": round(time.monotonic() - started, 1),
            "ok": run.returncode == 0 and record.get("leftovers") == [],
        }
        print(name, summary["scenarios"][name], flush=True)
    summary["all_ok"] = all(v["ok"] for v in summary["scenarios"].values())
    with open(os.path.join(args.out, "summary.json"), "w") as f:
        json.dump(summary, f, indent=1)
    return 0 if summary["all_ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
