"""End-to-end OFFLINE tests of harness-v2 (real bwrap sandbox, FAKE provider only; the real Claude
binary is not even mounted). Each case deliberately breaks one property and checks the harness
detects it. Usage: test_e2e_offline.py <A0_ROOT> <out.json>"""
import importlib.util
import json
import os
import sys
import threading
import time

ROOT = os.path.realpath(sys.argv[1])
V2 = os.path.join(ROOT, "harness-v2")
sys.path.insert(0, V2)
import limits  # noqa: E402
from run_case import run_case  # noqa: E402

EVB = os.path.join(ROOT, "remediation", "e2e-evidence")
os.makedirs(EVB, mode=0o700, exist_ok=False)
results = {}
FAKE = lambda mode: ["/usr/bin/python3", "-I", "/a0/harness/fake_provider.py", mode]  # noqa: E731
LIST_OK = {"requests": (1, 9), "desc_present": ["alpha", "proj"], "desc_absent": ["beta", "gamma"]}


def case(name, ok, detail):
    results[name] = {"pass": bool(ok), "detail": detail}
    print(("PASS " if ok else "FAIL ") + name, json.dumps(detail, default=str)[:260], flush=True)


def bwraps():
    out = []
    for p in os.listdir("/proc"):
        if p.isdigit():
            try:
                if os.readlink(f"/proc/{p}/exe").endswith("/bwrap") and ROOT.encode() in open(
                        f"/proc/{p}/cmdline", "rb").read():
                    out.append(int(p))
            except OSError:
                pass
    return out


def summary(r):
    return {"verdict": r.get("verdict"), "reasons": r.get("reasons"), "killed": (r.get("launcher") or {}).get("killed"),
            "leftovers": (r.get("launcher") or {}).get("leftovers"), "cleanup": r.get("cleanup")}


# 1 positive control: the harness can PASS a correct run
r = run_case(ROOT, EVB, {"id": "e2e-ok", "sut_argv": FAKE("ok"), "expect": LIST_OK})
case("e2e_ok_passes", r["verdict"] == "PASS" and r["evidence"]["complete"], summary(r))
# 2 tampering of requests.jsonl by the system under test
r = run_case(ROOT, EVB, {"id": "e2e-tamper", "sut_argv": FAKE("tamper_requests"), "expect": LIST_OK})
case("e2e_disk_tamper_fails", r["verdict"] == "FAIL" and "evidence_tampered" in r["reasons"], summary(r))
# 3 forging an evidence record through /proc/<harness>/fd/1
r = run_case(ROOT, EVB, {"id": "e2e-forge", "sut_argv": FAKE("forge_stream"), "expect": LIST_OK})
forged_detected = r["verdict"] == "FAIL" and "evidence_tampered" in r["reasons"]
blocked = "blocked" in (r.get("sut_stderr") or "")
case("e2e_forged_record_detected_or_blocked", forged_detected or (blocked and r["verdict"] == "PASS"),
     {**summary(r), "forge_blocked": blocked, "stream_problems": r["evidence"]["stream_problems"]})
# 4 no request where one is required
r = run_case(ROOT, EVB, {"id": "e2e-norequest", "sut_argv": FAKE("no_request"), "expect": LIST_OK})
case("e2e_no_request_not_pass", r["verdict"] in ("FAIL", "INCONCLUSIVE"), summary(r))
# 5 validator mismatch participates in the verdict
r = run_case(ROOT, EVB, {"id": "e2e-validator", "sut_argv": FAKE("ok"), "overlay": {"skillOverrides": {"a0-beta": "bogus"}},
                         "expect": {**LIST_OK, "validator": True}})
case("e2e_validator_mismatch_fails", r["verdict"] == "FAIL" and "validator_matches" in r["reasons"], summary(r))
# 6 expected tool call must be proven attempted
TOOL = {"requests": (2, 9), "victim_present": True, "tool_attempt": {"text_contains": "has been denied", "is_error": True}}
r = run_case(ROOT, EVB, {"id": "e2e-tool-yes", "sut_argv": FAKE("tool_attempt"), "expect": TOOL})
case("e2e_tool_attempt_proven_passes", r["verdict"] == "PASS", summary(r))
r = run_case(ROOT, EVB, {"id": "e2e-tool-no", "sut_argv": FAKE("ok"), "expect": TOOL})
case("e2e_tool_not_attempted_fails", r["verdict"] == "FAIL", summary(r))
# 7 hang -> timeout kill, no leftovers, cleanup
t = time.monotonic()
r = run_case(ROOT, EVB, {"id": "e2e-hang", "sut_argv": FAKE("hang"), "sut_timeout": 3600, "expect": LIST_OK}, timeout=6)
el = time.monotonic() - t
case("e2e_timeout_killed_bounded_clean", r["verdict"] != "PASS" and r["launcher"]["killed"] == "timeout"
     and not r["launcher"]["leftovers"] and r["cleanup"]["staging_removed"] and el < 20 and not bwraps(),
     {**summary(r), "elapsed_s": round(el, 1)})
# 8 storage: /markers is a kernel-capped tmpfs; host evidence is bounded by what the stream carries
r = run_case(ROOT, EVB, {"id": "e2e-flood", "sut_argv": FAKE("flood_markers"), "expect": LIST_OK}, timeout=60)
mk = os.path.join(EVB, "e2e-flood", "markers")
mk_size = limits.du(mk) if os.path.isdir(mk) else 0
floods = [n for n in os.listdir(mk) if n.startswith("flood-")] if os.path.isdir(mk) else []
flood_bytes = sum(os.path.getsize(os.path.join(mk, n)) for n in floods)
case("e2e_markers_tmpfs_cap_enforced", flood_bytes <= limits.MARKERS_TMPFS and mk_size <= limits.MARKERS_TMPFS + limits.MiB
     and "No space left" in (r.get("sut_stderr") or "") and r["verdict"] != "PASS" and not r["launcher"]["leftovers"],
     {**summary(r), "markers_evidence_mib": round(mk_size / limits.MiB, 2), "flood_files": len(floods),
      "flood_mib": round(flood_bytes / limits.MiB, 3),
      "enospc_seen": "No space left" in (r.get("sut_stderr") or "")})
ev_size = limits.du(os.path.join(EVB, "e2e-flood"))
case("e2e_flood_host_evidence_bounded", ev_size < limits.STREAM_CAP + 2 * limits.MARKERS_TMPFS,
     {"evidence_dir_mib": round(ev_size / limits.MiB, 1)})
# 8b stream flood through /proc/<harness>/fd/1 -> host stream cap kills, evidence bounded
r = run_case(ROOT, EVB, {"id": "e2e-flood-stream", "sut_argv": FAKE("flood_stream"), "expect": LIST_OK}, timeout=60)
sl = os.path.getsize(os.path.join(EVB, "e2e-flood-stream", "stream.log"))
case("e2e_stream_cap_enforced", r["verdict"] == "STOP_ISOLATION" and r["launcher"]["killed"] == "stream_cap"
     and sl <= limits.STREAM_CAP and not r["launcher"]["leftovers"],
     {**summary(r), "stream_log_mib": round(sl / limits.MiB, 1)})
# 9 kernel rlimit: single file above RLIMIT_FSIZE
r = run_case(ROOT, EVB, {"id": "e2e-bigfile", "sut_argv": FAKE("bigfile"),
                         "expect": {"requests": (0, 0), "stdout_contains": ["errno=27"], "stdout_absent": ["wrote 80 MiB"]}})
case("e2e_rlimit_fsize_enforced_EFBIG", r["verdict"] == "PASS", summary(r))
# 10 kernel tmpfs cap on HOME
r = run_case(ROOT, EVB, {"id": "e2e-fillhome", "sut_argv": FAKE("fill_home"),
                         "expect": {"requests": (0, 0), "stdout_contains": ["errno=28"], "stdout_absent": ["no limit hit"]}},
             sizes={"home_size": 64 * limits.MiB})
out = (r.get("sut_stdout") or "").strip()
case("e2e_home_tmpfs_cap_enforced_ENOSPC", r["verdict"] == "PASS", {**summary(r), "stdout": out})
# 11 orphan: namespace-based detection sees it; v1 path/name-based detection does not
spec = importlib.util.spec_from_file_location("rc1", os.path.join(ROOT, "harness", "run_case.py"))
saved_path = list(sys.path)
sys.path.insert(0, os.path.join(ROOT, "harness"))
for m in ("integrity", "overlay_validator", "sandbox"):
    sys.modules.pop(m, None)
rc1 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rc1)
sys.path[:] = saved_path
for m in ("integrity", "overlay_validator", "sandbox"):
    sys.modules.pop(m, None)
seen = {"v1_saw_sleep": False, "v2_saw_sleep": False}
stop = threading.Event()


def sampler():
    while not stop.is_set():
        for p in rc1.leftovers(ROOT):
            if p["exe"].endswith("/sleep"):
                seen["v1_saw_sleep"] = True
        for b in bwraps():
            try:
                ns = os.readlink(f"/proc/{b}/ns/pid")
            except OSError:
                continue
            for p in os.listdir("/proc"):
                if p.isdigit():
                    try:
                        if os.readlink(f"/proc/{p}/exe").endswith("/sleep") and \
                                os.readlink(f"/proc/{p}/ns/pid") != ns:
                            # in a different (child) namespace than bwrap's own host-side ns
                            pass
                    except OSError:
                        pass
        time.sleep(0.2)


th = threading.Thread(target=sampler, daemon=True)
th.start()
r = run_case(ROOT, EVB, {"id": "e2e-orphan", "sut_argv": FAKE("spawn_orphan"), "sut_env": {"A0_ORPHAN_HOLD": "3"},
                         "expect": {"requests": (0, 0), "stdout_contains": ["spawned orphan"], "stdout_absent": ["Traceback"]}})
stop.set()
th.join()
launcher = json.load(open(os.path.join(EVB, "e2e-orphan", "launcher.json")))
case("e2e_orphan_counted_in_namespace_and_none_left", launcher["members_max"] >= 4 and not launcher["leftovers"]
     and not bwraps(), {"members_max": launcher["members_max"], "leftovers": launcher["leftovers"], "verdict": r["verdict"]})
case("NEGCTL_v1_name_based_detector_misses_orphan", not seen["v1_saw_sleep"], seen)
# 12 exception inside the orchestrator after launch -> finally kills and cleans
r = run_case(ROOT, EVB, {"id": "e2e-inject", "sut_argv": FAKE("hang"), "expect": LIST_OK}, timeout=30,
             inject_failure="after_launch")
time.sleep(0.5)
case("e2e_injected_failure_cleans_up", r["verdict"] == "INCONCLUSIVE" and r["cleanup"]["staging_removed"]
     and not bwraps(), {**summary(r)})
# 13 root storage cap refuses before launch
r = run_case(ROOT, EVB, {"id": "e2e-rootcap", "sut_argv": FAKE("ok"), "expect": LIST_OK}, root_cap=1)
case("e2e_root_cap_refuses_launch", r.get("verdict") == "STOP_STORAGE" and not os.path.exists(os.path.join(EVB, "e2e-rootcap")),
     r)
# 14 hard guard: no provider execution in this step
r1 = run_case(ROOT, EVB, {"id": "e2e-guard-1", "expect": LIST_OK})
r2 = run_case(ROOT, EVB, {"id": "e2e-guard-2", "sut_argv": ["/opt/a0/bin/claude"], "expect": LIST_OK})
case("e2e_provider_guard_refuses", r1.get("verdict") == "REFUSED" and r2.get("verdict") == "REFUSED", [r1, r2])

left_dirs = os.listdir(os.path.join(ROOT, "cases-v2")) if os.path.isdir(os.path.join(ROOT, "cases-v2")) else []
case("e2e_no_staging_left", left_dirs == [], left_dirs)
json.dump(results, open(sys.argv[2], "w"), indent=1, default=str)
print("ALL_PASS" if all(v["pass"] for v in results.values()) else "SOME_FAIL")
sys.exit(0 if all(v["pass"] for v in results.values()) else 1)
