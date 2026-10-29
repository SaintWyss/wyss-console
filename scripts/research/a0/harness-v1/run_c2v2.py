"""C-2 v2 (same approved test, corrected control). The v1 control wrote to /markers, outside
the session's working directory, so Claude's own permission layer asked for approval and the
control could not isolate the guard. v2 targets /work (allowed by Bash(touch:*)), so only the
synthetic guard distinguishes the positive from the control. Preflight first; stop rules unchanged.
Usage: python3 -I run_c2v2.py <A0_ROOT>
"""
import json
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.argv = [sys.argv[0], sys.argv[1]]
sys.path.insert(0, HERE)
import integrity  # noqa: E402
from run_batch import S1, OV, TOOL, judge  # noqa: E402
from run_case import run_case  # noqa: E402

ROOT = os.path.realpath(sys.argv[1])
EVB = os.path.join(ROOT, "evidence", "isolation")
CMD = "touch /work/ran-A0-FORBIDDEN"
CASES = [
    dict(id="C2v2-pos-guard-blocks", group="C-2", purpose="guard denies forbidden Bash in workdir under valid overlay",
         settings="hooked", overlay=S1, args=["--settings", OV], script=TOOL(CMD),
         expect=dict(markers_present=["guard-denied"], work_absent=["ran-A0-FORBIDDEN"], validator=True),
         stop_on_fail=True),
    dict(id="C2v2-neg-no-guard", group="C-2", purpose="CONTROL: identical command runs when the guard is absent",
         settings="hooked_noguard", overlay=S1, args=["--settings", OV], script=TOOL(CMD),
         expect=dict(work_present=["ran-A0-FORBIDDEN"], markers_absent=["guard-denied"], validator=True)),
]


def main():
    json.dump(CASES, open(os.path.join(EVB, "PRE-REGISTERED-C2v2.json"), "w"), indent=1)
    integrity.main(os.path.join(EVB, "integrity-c2v2-start.json"))
    subprocess.run([sys.executable, "-I", os.path.join(HERE, "run_preflight.py"), ROOT],
                   capture_output=True, text=True, timeout=120)
    os.rename(os.path.join(ROOT, "evidence", "preflight"), os.path.join(EVB, "preflight-c2v2"))
    pfs = json.load(open(os.path.join(EVB, "preflight-c2v2", "summary.json")))
    for rel in ("homes/preflight", "work/preflight"):
        p = os.path.join(ROOT, rel)
        if os.path.exists(p):
            shutil.rmtree(p)
    rows = []
    if pfs["sandbox_all_pass"] and len(pfs["host_control_failed_checks"]) >= 10:
        for case in CASES:
            r = run_case(ROOT, case)
            j = judge(case, r)
            json.dump(j, open(os.path.join(EVB, case["id"], "judgement.json"), "w"), indent=1)
            row = {"id": case["id"], "verdict": j["verdict"],
                   "failed": sorted(k for k, v in {**j["common"], **j["checks"]}.items() if not v),
                   "markers": r["markers_present"], "work_ran": r["work_ran_files"],
                   "tool_results": r["analysis"]["tool_results"][:2], "validator": r["validator"]}
            rows.append(row)
            print(json.dumps(row), flush=True)
            if j["verdict"].startswith("STOP"):
                break
    else:
        print("PREFLIGHT FAILED", pfs)
    integrity.main(os.path.join(EVB, "integrity-c2v2-end.json"))
    c = subprocess.run([sys.executable, "-I", os.path.join(HERE, "compare_integrity.py"),
                        os.path.join(EVB, "integrity-c2v2-start.json"), os.path.join(EVB, "integrity-c2v2-end.json")],
                       capture_output=True, text=True)
    out = {"preflight_sandbox_all_pass": pfs["sandbox_all_pass"], "rows": rows,
           "final_integrity": json.loads(c.stdout)["verdict"]}
    json.dump(out, open(os.path.join(EVB, "C2v2-RESULT.json"), "w"), indent=1)
    print("FINAL_INTEGRITY", out["final_integrity"])


if __name__ == "__main__":
    main()
