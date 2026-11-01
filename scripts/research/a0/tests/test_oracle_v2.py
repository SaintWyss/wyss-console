"""Offline tests for harness-v2/oracle.py (unit + historical re-judgement, read-only).
Usage: test_oracle_v2.py <A0_ROOT> <out.json>"""
import copy
import hashlib
import importlib.util
import json
import os
import sys

ROOT = sys.argv[1]
sys.path.insert(0, os.path.join(ROOT, "harness-v2"))
import oracle  # noqa: E402
import stream  # noqa: E402
from sandbox import SYNTH_KEY  # noqa: E402

RUN = "R"
SK = ["alpha", "beta", "gamma", "proj", "hidden", "ghost"]
results = {}


def case(name, ok, detail=""):
    results[name] = {"pass": bool(ok), "detail": detail}
    print(("PASS " if ok else "FAIL ") + name + (f"  [{json.dumps(detail, default=str)[:200]}]" if detail else ""))


def req_line(listed, extra_msgs=None, key=SYNTH_KEY):
    text = oracle.LIST_HEADER + "".join(f"\n- a0-{n}: x CANARY-DESC-{n}-{RUN}" for n in listed)
    msgs = [{"role": "user", "content": [{"type": "text", "text": text + f" CANARY-AGENTS-project-{RUN}"}]}]
    msgs += extra_msgs or []
    e = {"method": "POST", "path": "/v1/messages", "headers": {}, "has_key": True,
         "key_sha256": hashlib.sha256(key.encode()).hexdigest(), "body": {"messages": msgs}}
    return json.dumps(e) + "\n"


def build(lines, markers=None, tamper=None, drop_end=False, key=b"k" * 32):
    out = []

    class W:
        def write(self, s):
            out.append(s)

        def flush(self):
            pass
    em = stream.Emitter(key, W())
    em.emit("START")
    em.emit("FIXTURE", home={}, work={})
    for l in lines:
        em.emit("REQUEST", sha256=hashlib.sha256(l.encode()).hexdigest(), bytes=len(l))
    em.emit("SUT_DONE", result={"exit": 0})
    mk = markers or {"requests.jsonl": hashlib.sha256("".join(lines).encode()).hexdigest()}
    em.emit("FINAL", home={}, work={"a0-victim": "x"}, markers=mk)
    if not drop_end:
        em.emit("END")
    text = "".join(out)
    if tamper == "forge":
        text += 'A0REC 99 {"kind":"END"} ' + "0" * 64 + "\n"
    records, problems, foreign = stream.verify(key, text)
    disk = list(lines)
    if tamper == "disk":
        disk = [l.replace("CANARY-DESC-alpha", "CANARY-DESC-XXXXX") for l in lines]
    ev = oracle.verify_evidence(records, problems, disk, mk)
    return disk, ev


def res_for(lines, ev, **over):
    r = {"analysis": oracle.analyse(RUN, lines, SK), "evidence": ev, "inside": {"exit": 0, "timed_out": False},
         "isolation": {"integrity": True, "namespace_verified": True, "no_leftovers": True,
                       "no_unexpected_proxy": True, "owned_files_unchanged": True,
                       "not_killed_for_storage": True, "launcher_complete": True}, "validator": None,
         "markers_present": [], "work_ran": [], "victim_present": True, "stdout": "ok from mock", "stderr": ""}
    r.update(over)
    return r


BASE = {"id": "t", "expect": {"requests": (1, 9), "desc_present": ["alpha"], "desc_absent": ["beta"]}}

lines, ev = build([req_line(["alpha"])])
case("positive_pass", oracle.judge(BASE, res_for(lines, ev), RUN, SK)["verdict"] == "PASS")
lines2, ev2 = build([req_line(["alpha", "beta"])])
case("negative_condition_violated_fails", oracle.judge(BASE, res_for(lines2, ev2), RUN, SK)["verdict"] == "FAIL")
# vacuous truths
l0, e0 = build([])
j = oracle.judge(BASE, res_for(l0, e0), RUN, SK)
case("zero_requests_never_pass", j["verdict"] in ("FAIL", "INCONCLUSIVE"), j["reasons"])
c0 = {"id": "t0", "expect": {"requests": (0, 0), "stdout_contains": ["disabled via skillOverrides"],
                             "body_absent": ["beta"]}}
j = oracle.judge(c0, res_for(l0, e0, stdout="Skill disabled via skillOverrides"), RUN, SK)
case("zero_request_case_keys_not_counted_as_pass", "keys_synthetic" not in j["checks"] and j["verdict"] == "PASS", j)
j = oracle.judge(c0, res_for(l0, e0, stdout="something else"), RUN, SK)
case("refusal_text_required", j["verdict"] == "FAIL", j["reasons"])
# non-synthetic key
l3, e3 = build([req_line(["alpha"], key="sk-real-looking")])
case("non_synthetic_key_fails", oracle.judge(BASE, res_for(l3, e3), RUN, SK)["verdict"] == "FAIL")
# validator mismatch participates
cv = copy.deepcopy(BASE)
cv["overlay"] = {"skillOverrides": {"a0-beta": "off"}}
cv["expect"]["validator"] = True
j = oracle.judge(cv, res_for(lines, ev, validator={"accepted": False, "errors": ["x"]}), RUN, SK)
case("validator_mismatch_fails", j["verdict"] == "FAIL" and "validator_matches" in j["reasons"], j["reasons"])
cv2 = copy.deepcopy(cv)
del cv2["expect"]["validator"]
case("overlay_without_validator_expectation_is_spec_error",
     oracle.judge(cv2, res_for(lines, ev), RUN, SK)["verdict"] == "INCONCLUSIVE")
# tool attempt must be proven
ct = {"id": "c3", "expect": {"requests": (2, 9), "victim_present": True,
                             "tool_attempt": {"text_contains": "has been denied", "is_error": True}}}
j = oracle.judge(ct, res_for(lines, ev), RUN, SK)
case("tool_not_attempted_fails", j["verdict"] == "FAIL", j["reasons"])
tr = [{"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_mock_0", "is_error": True,
                                    "content": "Permission to use Bash with command rm has been denied."}]}]
l4, e4 = build([req_line(["alpha"]), req_line(["alpha"], extra_msgs=tr)])
case("tool_attempt_proven_passes", oracle.judge(ct, res_for(l4, e4), RUN, SK)["verdict"] == "PASS")
tr_ok = copy.deepcopy(tr)
tr_ok[0]["content"][0]["is_error"] = False
l5, e5 = build([req_line(["alpha"]), req_line(["alpha"], extra_msgs=tr_ok)])
case("tool_attempt_wrong_outcome_fails", oracle.judge(ct, res_for(l5, e5), RUN, SK)["verdict"] == "FAIL")
# lint: a case without a negative expectation cannot PASS
cl = {"id": "l", "expect": {"requests": (1, 9), "desc_present": ["alpha"]}}
case("lint_missing_negative_inconclusive", oracle.judge(cl, res_for(lines, ev), RUN, SK)["verdict"] == "INCONCLUSIVE")
cl2 = {"id": "l2", "expect": {"desc_present": ["alpha"], "desc_absent": ["beta"]}}
case("lint_missing_requests_inconclusive", oracle.judge(cl2, res_for(lines, ev), RUN, SK)["verdict"] == "INCONCLUSIVE")
# tamper evidence
ld, ed = build([req_line(["alpha"])], tamper="disk")
case("disk_tamper_detected_fail", ed["tampered"] and oracle.judge(BASE, res_for(ld, ed), RUN, SK)["verdict"] == "FAIL")
lf, efg = build([req_line(["alpha"])], tamper="forge")
case("forged_stream_record_detected", efg["tampered"] and any("bad_record" in p for p in efg["stream_problems"]),
     efg["stream_problems"])
le, ee = build([req_line(["alpha"])], drop_end=True)
case("missing_END_inconclusive", oracle.judge(BASE, res_for(le, ee), RUN, SK)["verdict"] == "INCONCLUSIVE")
lw, ew = build([req_line(["alpha"])], key=b"k" * 32)
import io
signed = io.StringIO()
stream.Emitter(b"k" * 32, signed).emit("END")
recs, probs, _ = stream.verify(b"other-key" * 4, signed.getvalue())
case("wrong_key_stream_incomplete", not recs and any(p.startswith("bad_record") for p in probs))
# isolation precedence
case("isolation_failure_stops", oracle.judge(BASE, res_for(lines, ev, isolation={"integrity": False}),
                                             RUN, SK)["verdict"] == "STOP_ISOLATION")
# listing names with '.'
ldot = [json.dumps({"method": "POST", "path": "/v1/messages", "has_key": False, "body": {"messages": [
    {"role": "user", "content": oracle.LIST_HEADER + "\n- pkg.tool: d\n- a0-alpha: d"}]}}) + "\n"]
a = oracle.analyse(RUN, ldot, SK)
case("listing_parser_accepts_dot_names", "pkg.tool" in a["listed_other"], a["listed_other"])
# pair discrimination
P = {"verdict": "PASS"}
case("pair_equal_observable_inconclusive", oracle.judge_pair(P, P, False, False)["verdict"] == "INCONCLUSIVE")
case("pair_discriminated_pass", oracle.judge_pair(P, P, False, True)["verdict"] == "PASS")
case("pair_member_fail_fails", oracle.judge_pair({"verdict": "FAIL"}, P, 1, 2)["verdict"] == "FAIL")

# Lint the frozen v1 case table (read-only): which v1 cases would not be gradable under v2
spec = importlib.util.spec_from_file_location("rb1", os.path.join(ROOT, "harness", "run_batch.py"))
saved = sys.argv
sys.argv = [saved[0], "/nonexistent"]
rb1 = importlib.util.module_from_spec(spec)
sys.path.insert(0, os.path.join(ROOT, "harness"))
try:
    spec.loader.exec_module(rb1)
finally:
    sys.path.pop(0)
    sys.argv = saved
lint = {c["id"]: oracle.lint(c) for c in rb1.CASES}
bad = {k: v for k, v in lint.items() if v}
case("v1_case_table_linted", True, {"v1_cases": len(lint), "not_gradable_under_v2": len(bad)})
json.dump(lint, open(os.path.join(os.path.dirname(sys.argv[2]), "v1_case_lint.json"), "w"), indent=1)

# Historical re-judgement, read-only: v1 evidence has no HMAC stream -> cannot be certified now
hist = []
for d in sorted(os.listdir(os.path.join(ROOT, "evidence", "isolation"))):
    rp = os.path.join(ROOT, "evidence", "isolation", d, "markers", "requests.jsonl")
    if os.path.isfile(rp):
        recs, probs, _ = stream.verify(b"\0" * 32, "")
        evh = oracle.verify_evidence(recs, probs, open(rp).read().splitlines(keepends=True), {})
        hist.append({"case": d, "complete": evh["complete"], "problems": evh["stream_problems"]})
case("historical_evidence_not_certifiable_retroactively", hist and all(not h["complete"] for h in hist),
     {"cases": len(hist)})

json.dump(results, open(sys.argv[2], "w"), indent=1, default=str)
print("ALL_PASS" if all(r["pass"] for r in results.values()) else "SOME_FAIL")
sys.exit(0 if all(r["pass"] for r in results.values()) else 1)
