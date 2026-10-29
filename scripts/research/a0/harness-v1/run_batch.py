"""A0 Claude isolation batch: A-2, A-8, C-1..C-3 (owner turn turn:746733c8-a9ca-4713-85fe-cb9bec6d480e).
1) writes the pre-registered case table; 2) 31-point preflight (stop on failure);
3) runs cases in order, stopping on any stop-class result; 4) final integrity; 5) matrix.
Usage: python3 -I run_batch.py <A0_ROOT>
"""
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import integrity  # noqa: E402
from run_case import run_case  # noqa: E402

ROOT = os.path.realpath(sys.argv[1])
EVB = os.path.join(ROOT, "evidence", "isolation")
BUILTINS = ["dataviz", "update-config", "keybindings-help", "code-review", "simplify",
            "fewer-permission-prompts", "loop", "claude-api", "workflow-authoring", "run",
            "plugin-authoring", "init", "security-review"]
OV = "{OVERLAY}"
S1 = {"skillOverrides": {"a0-beta": "off", "a0-gamma": "off"}}          # select alpha + proj
S2 = {"skillOverrides": {"a0-alpha": "off", "a0-proj": "off"}}          # select beta + gamma
ALL = ["alpha", "beta", "gamma", "proj"]
TOOL = lambda cmd: [{"tool_use": {"name": "Bash", "input": {"command": cmd, "description": "a0"}}},
                    {"text": "ok from mock"}]

CASES = [
    # ---- A-2: individual exclusion (valid overlays; leak = STOP)
    dict(id="A2-01-user-off", group="A-2", purpose="user skill excluded via overlay",
         overlay={"skillOverrides": {"a0-beta": "off"}}, args=["--settings", OV],
         expect=dict(desc_present=["alpha", "gamma", "proj"], desc_absent=["beta", "hidden", "ghost"],
                     requests=(1, 9), agents=True, exit_code=0, validator=True), stop_on_fail=True),
    dict(id="A2-02-project-off", group="A-2", purpose="project skill excluded via overlay",
         overlay={"skillOverrides": {"a0-proj": "off"}}, args=["--settings", OV],
         expect=dict(desc_present=["alpha", "beta", "gamma"], desc_absent=["proj", "hidden", "ghost"],
                     requests=(1, 9), agents=True, exit_code=0, validator=True), stop_on_fail=True),
    dict(id="A2-03-slash-refused", group="A-2", purpose="user invocation of excluded skill refused, body never sent",
         overlay={"skillOverrides": {"a0-beta": "off"}}, args=["--settings", OV], prompt="/a0-beta",
         expect=dict(body_absent=["beta"], validator=True, record_only=["exit", "requests", "output"]),
         stop_on_fail=True),
    dict(id="A2-03c-slash-control", group="A-2", purpose="CONTROL: same invocation without overlay loads the body",
         prompt="/a0-beta", expect=dict(body_present=["beta"])),
    # ---- A-2 failure behaviour (characterised, not stop-class; validator must reject)
    dict(id="A2-F1-invalid-value", group="A-2/fail", purpose="invalid override value",
         overlay={"skillOverrides": {"a0-beta": "bogus"}}, args=["--settings", OV],
         characterize=True, expect=dict(validator=False)),
    dict(id="A2-F2-misspelled-key", group="A-2/fail", purpose="misspelled top-level key",
         overlay={"skillOverride": {"a0-beta": "off"}}, args=["--settings", OV],
         characterize=True, expect=dict(validator=False)),
    dict(id="A2-F3-misspelled-name", group="A-2/fail", purpose="misspelled skill name",
         overlay={"skillOverrides": {"a0-betta": "off"}}, args=["--settings", OV],
         characterize=True, expect=dict(validator=False)),
    dict(id="A2-F4-case-variant", group="A-2/fail", purpose="case-variant skill name",
         overlay={"skillOverrides": {"A0-BETA": "off"}}, args=["--settings", OV],
         characterize=True, expect=dict(validator=False)),
    dict(id="A2-F5-malformed-json", group="A-2/fail", purpose="malformed JSON overlay file",
         overlay_raw='{"skillOverrides": {"a0-beta": "off",}', args=["--settings", OV],
         characterize=True, expect=dict(validator=False)),
    dict(id="A2-F6-missing-file", group="A-2/fail", purpose="overlay path does not exist",
         args=["--settings", "/a0/case/does-not-exist.json"], characterize=True, expect=dict()),
    # ---- A-8: two alternative exact selections, no file modification (leak = STOP)
    *[dict(id=f"A8-S1-H{i}", group="A-8", purpose="selection S1 = {alpha, proj}",
           overlay=S1, args=["--settings", OV],
           expect=dict(listed_fixture_exact=["a0-alpha", "a0-proj"], desc_absent=["beta", "gamma", "hidden", "ghost"],
                       agents=True, exit_code=0, validator=True), stop_on_fail=True) for i in (1, 2, 3)],
    *[dict(id=f"A8-S2-H{i}", group="A-8", purpose="selection S2 = {beta, gamma}",
           overlay=S2, args=["--settings", OV],
           expect=dict(listed_fixture_exact=["a0-beta", "a0-gamma"], desc_absent=["alpha", "proj", "hidden", "ghost"],
                       agents=True, exit_code=0, validator=True), stop_on_fail=True) for i in (1, 2, 3)],
    dict(id="A8-S1-I1", group="A-8", purpose="selection S1, interactive TUI (Native launch shape)",
         mode="I", overlay=S1, args=["--settings", OV],
         expect=dict(listed_fixture_exact=["a0-alpha", "a0-proj"], desc_absent=["beta", "gamma", "hidden", "ghost"],
                     agents=True, clean_exit=True, validator=True), stop_on_fail=True),
    dict(id="A8-BI-builtins-off", group="A-8/builtin", purpose="S1 plus every built-in skill set to off",
         overlay={"skillOverrides": {**S1["skillOverrides"], **{b: "off" for b in BUILTINS}}},
         args=["--settings", OV], characterize=True,
         expect=dict(listed_fixture_exact=["a0-alpha", "a0-proj"], validator=False)),
    # ---- C-1..C-3: synthetic security hooks and permissions under a valid overlay (bypass = STOP)
    dict(id="C1-pos-hooks-fire", group="C-1", purpose="SessionStart/UserPromptSubmit hooks fire under valid overlay",
         settings="hooked", overlay=S1, args=["--settings", OV],
         expect=dict(markers_present=["hook-SessionStart", "hook-UserPromptSubmit"], exit_code=0, validator=True),
         stop_on_fail=True),
    dict(id="C1-neg-disableAllHooks", group="C-1", purpose="CONTROL: overlay with disableAllHooks suppresses markers (validator must reject)",
         settings="hooked", overlay={**S1, "disableAllHooks": True}, args=["--settings", OV],
         expect=dict(markers_absent=["hook-SessionStart", "hook-UserPromptSubmit"], validator=False)),
    dict(id="C2-pos-guard-blocks", group="C-2", purpose="blocking PreToolUse guard denies forbidden Bash under valid overlay",
         settings="hooked", overlay=S1, args=["--settings", OV], script=TOOL("touch /markers/ran-A0-FORBIDDEN"),
         expect=dict(markers_present=["guard-denied"], markers_absent=["ran-A0-FORBIDDEN"], validator=True),
         stop_on_fail=True),
    dict(id="C2-neg-no-guard", group="C-2", purpose="CONTROL: same command runs when the guard is absent",
         settings="hooked_noguard", overlay=S1, args=["--settings", OV], script=TOOL("touch /markers/ran-A0-FORBIDDEN"),
         expect=dict(markers_present=["ran-A0-FORBIDDEN"], validator=True)),
    dict(id="C3-pos-deny-rule", group="C-3", purpose="permissions.deny Bash(rm:*) holds under valid overlay",
         settings="hooked", overlay=S1, args=["--settings", OV], script=TOOL("rm /work/a0-victim"),
         expect=dict(victim_present=True, validator=True), stop_on_fail=True),
    dict(id="C3-neg-allow-rm", group="C-3", purpose="CONTROL: same rm succeeds when allowed and not denied",
         settings="hooked_allow_rm", overlay=S1, args=["--settings", OV], script=TOOL("rm /work/a0-victim"),
         expect=dict(victim_present=False, validator=True)),
]

WARN_TERMS = ["did not load", "invalid", "error", "skilloverrides", "settings", "not found", "unknown"]


def judge(case, r):
    e = case.get("expect", {})
    a = r["analysis"]
    common = {
        "integrity": r["integrity_pass"], "no_leftovers": not r["leftovers"],
        "cleanup": all(r["cleanup"].values()), "owned_files_unchanged": not r["owned_files_changed"],
        "synthetic_key_only": a["all_keys_synthetic"], "no_unexpected_proxy": not a["proxy_unexpected"],
        "bwrap_exit_0": r["bwrap_exit"] == 0,
    }
    checks = {}
    for n in e.get("desc_present", []):
        checks[f"desc_present:{n}"] = a["desc"][n]
    for n in e.get("desc_absent", []):
        checks[f"desc_absent:{n}"] = not a["desc"][n]
    for n in e.get("body_absent", ALL if not e.get("body_present") else []):
        checks[f"body_absent:{n}"] = not a["body"][n]
    for n in e.get("body_present", []):
        checks[f"body_present:{n}"] = a["body"][n]
    if "agents" in e:
        checks["agents_present"] = a["agents"] == e["agents"]
    if "requests" in e:
        lo, hi = e["requests"]
        checks["requests_in_range"] = lo <= a["messages_requests"] <= hi
    if "listed_fixture_exact" in e:
        checks["listed_fixture_exact"] = a["listed_fixture"] == e["listed_fixture_exact"]
    for m in e.get("markers_present", []):
        checks[f"marker_present:{m}"] = m in r["markers_present"]
    for m in e.get("markers_absent", []):
        checks[f"marker_absent:{m}"] = m not in r["markers_present"]
    for f in e.get("work_present", []):
        checks[f"work_present:{f}"] = f in r.get("work_ran_files", [])
    for f in e.get("work_absent", []):
        checks[f"work_absent:{f}"] = f not in r.get("work_ran_files", [])
    if "victim_present" in e:
        checks["victim_present"] = r["victim_present"] == e["victim_present"]
    if "exit_code" in e:
        checks["exit_code"] = r["inside"].get("exit") == e["exit_code"]
    if "clean_exit" in e:
        checks["clean_exit"] = (r["inside"].get("exit") or {}).get("clean") is True
    validator_ok = None
    if "validator" in e and r["validator"] is not None:
        validator_ok = r["validator"]["accepted"] == e["validator"]
    out = {"common": common, "checks": checks, "validator_matches": validator_ok}
    common_ok = all(common.values())
    if case.get("characterize"):
        exposed = a["desc"]["beta"]
        text = (r["stdout_excerpt"] + " " + r["stderr_excerpt"]).lower()
        warned = [t for t in WARN_TERMS if t in text]
        ex = r["inside"].get("exit")
        if a["messages_requests"] == 0 and ex not in (0, None):
            cls = "HARD_FAIL_NO_LAUNCH"
        elif not exposed:
            cls = "HONORED"
        elif warned:
            cls = "LOUD_FALLBACK"
        else:
            cls = "SILENT_FALLBACK"
        out["classification"] = cls
        out["warning_terms_seen"] = warned
        out["verdict"] = "RECORDED" if common_ok else "STOP_ISOLATION"
        if "listed_fixture_exact" in e:
            out["checks"]["listed_fixture_exact"] = a["listed_fixture"] == e["listed_fixture_exact"]
        return out
    case_ok = all(checks.values())
    if not common_ok:
        out["verdict"] = "STOP_ISOLATION"
    elif case_ok:
        out["verdict"] = "PASS"
    else:
        out["verdict"] = "STOP_FAIL" if case.get("stop_on_fail") else "FAIL"
    return out


def main():
    os.makedirs(EVB, mode=0o700, exist_ok=False)
    json.dump(CASES, open(os.path.join(EVB, "PRE-REGISTERED-CASES.json"), "w"), indent=1)
    integrity.main(os.path.join(EVB, "integrity-batch-start.json"))
    pf = subprocess.run([sys.executable, "-I", os.path.join(HERE, "run_preflight.py"), ROOT],
                        capture_output=True, text=True, timeout=120)
    os.rename(os.path.join(ROOT, "evidence", "preflight"), os.path.join(EVB, "preflight"))
    pfs = json.load(open(os.path.join(EVB, "preflight", "summary.json")))
    sb = json.load(open(os.path.join(EVB, "preflight", "sandbox.stdout.json")))
    matrix = {"preflight": {"sandbox_checks_pass": sum(v["pass"] for v in sb["checks"].values()),
                            "sandbox_checks_total": len(sb["checks"]),
                            "host_control_failures": len(pfs["host_control_failed_checks"])},
              "cases": [], "stopped_at": None}
    for rel in ("homes/preflight", "work/preflight"):
        p = os.path.join(ROOT, rel)
        if os.path.exists(p):
            import shutil
            shutil.rmtree(p)
    if not pfs["sandbox_all_pass"] or len(pfs["host_control_failed_checks"]) < 10:
        matrix["stopped_at"] = "preflight"
    else:
        for case in CASES:
            r = run_case(ROOT, case)
            j = judge(case, r)
            row = {"id": case["id"], "group": case["group"], "purpose": case["purpose"],
                   "verdict": j["verdict"], "classification": j.get("classification"),
                   "validator": r["validator"], "validator_matches": j["validator_matches"],
                   "failed_checks": sorted(k for k, v in {**j["common"], **j["checks"]}.items() if not v),
                   "requests": r["analysis"]["messages_requests"], "exit": r["inside"].get("exit"),
                   "listed_fixture": r["analysis"]["listed_fixture"],
                   "listed_builtin_count": len(r["analysis"]["listed_builtin"]),
                   "listed_builtin": r["analysis"]["listed_builtin"],
                   "markers": r["markers_present"], "victim_present": r["victim_present"],
                   "tool_results": r["analysis"]["tool_results"][:2],
                   "warning_terms": j.get("warning_terms_seen"),
                   "proxy_attempts": r["analysis"]["proxy_attempts"],
                   "provider_writes_home": r["provider_writes_home"],
                   "stdout_excerpt": r["stdout_excerpt"][:200], "stderr_excerpt": r["stderr_excerpt"][:300]}
            matrix["cases"].append(row)
            json.dump(j, open(os.path.join(EVB, case["id"], "judgement.json"), "w"), indent=1)
            print(f'{row["id"]:<26} {row["verdict"]:<15} {row["classification"] or "":<20} '
                  f'req={row["requests"]} exit={row["exit"] if not isinstance(row["exit"], dict) else row["exit"].get("code")} '
                  f'listed={row["listed_fixture"]} failed={row["failed_checks"]}', flush=True)
            if j["verdict"].startswith("STOP"):
                matrix["stopped_at"] = case["id"]
                break
    integrity.main(os.path.join(EVB, "integrity-batch-end.json"))
    c = subprocess.run([sys.executable, "-I", os.path.join(HERE, "compare_integrity.py"),
                        os.path.join(EVB, "integrity-batch-start.json"), os.path.join(EVB, "integrity-batch-end.json")],
                       capture_output=True, text=True)
    matrix["final_integrity"] = json.loads(c.stdout)
    json.dump(matrix, open(os.path.join(EVB, "MATRIX.json"), "w"), indent=1)
    print("STOPPED_AT", matrix["stopped_at"], "| final integrity", matrix["final_integrity"]["verdict"])


if __name__ == "__main__":
    main()
