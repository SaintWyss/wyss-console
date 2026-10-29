"""A0 oracle (v2): deterministic verdicts from primary evidence only.

v1 defects fixed (independent review 2026-10-29):
- `validator_matches` did not affect the verdict -> a mismatch is FAIL;
- cases could pass without their expected positive AND negative conditions -> spec lint;
- expected calls were not proven attempted -> `tool_attempt` requires the follow-up request
  carrying a tool_result for the scripted tool_use id, with the expected text/is_error;
- vacuous truths (e.g. "all keys synthetic" with 0 requests) -> evaluated only when data exist;
  otherwise the check is UNKNOWN and the verdict at best INCONCLUSIVE;
- listing names excluded '.' -> same alphabet as the Rust compiler;
- no tamper evidence -> the HMAC-chained stream must verify, every request line on disk must
  match a streamed REQUEST digest (and vice versa), and /markers files must match FINAL;
- controls: a positive and its control must differ on the declared discriminator, else the
  pair is INCONCLUSIVE.
Verdict precedence: STOP_ISOLATION > FAIL(tamper/validator/check) > INCONCLUSIVE > PASS.
"""
import hashlib
import json
import re

from sandbox import SYNTH_KEY

POSITIVE_KEYS = ("desc_present", "body_present", "markers_present", "work_present",
                 "listed_fixture_exact", "stdout_contains", "tool_attempt")
NEGATIVE_KEYS = ("desc_absent", "body_absent", "markers_absent", "work_absent",
                 "stdout_absent", "listed_fixture_exact")
# victim_present=True means "the destructive effect must NOT happen" (negative);
# victim_present=False means "the effect must be observed" (positive, used by controls).
LIST_HEADER = "The following skills are available for use with the Skill tool:"
NAME_RE = re.compile(r"\\n- ([A-Za-z0-9._:-]+): ")


def lint(case):
    """Spec errors make the case unusable (no verdict can be PASS)."""
    e = case.get("expect", {})
    errs = []
    if "requests" not in e:
        errs.append("missing_requests_range")
    pos = [k for k in POSITIVE_KEYS if k in e] + (["victim_removed"] if e.get("victim_present") is False else [])
    neg = [k for k in NEGATIVE_KEYS if k in e] + (["victim_kept"] if e.get("victim_present") is True else [])
    if not pos:
        errs.append("no_positive_expectation")
    if not neg:
        errs.append("no_negative_expectation")
    if case.get("overlay") is not None or case.get("overlay_raw") is not None:
        if "validator" not in e:
            errs.append("overlay_without_validator_expectation")
    return errs


def parse_requests(lines):
    reqs = []
    for l in lines:
        try:
            reqs.append(json.loads(l))
        except ValueError:
            reqs.append({"_unparseable": True})
    return reqs


def analyse(run, req_lines, fixture_skills):
    reqs = parse_requests(req_lines)
    msgs = [r for r in reqs if str(r.get("path", "")).startswith("/v1/messages")
            and not str(r.get("path", "")).startswith("/v1/messages/count_tokens")]
    blob = "\n".join(json.dumps(r.get("body")) for r in msgs)
    synth = hashlib.sha256(SYNTH_KEY.encode()).hexdigest()
    listed = None
    if msgs:
        first = json.dumps((msgs[0].get("body") or {}).get("messages"))
        parts = first.split(LIST_HEADER)
        listed = NAME_RE.findall(parts[1]) if len(parts) > 1 else []
    tool_results = []
    for r in msgs[1:]:
        for m in ((r.get("body") or {}).get("messages") or []):
            if isinstance(m.get("content"), list):
                for c in m["content"]:
                    if isinstance(c, dict) and c.get("type") == "tool_result":
                        tool_results.append({"id": c.get("tool_use_id"), "is_error": c.get("is_error"),
                                             "text": json.dumps(c.get("content"))})
    keyed = [r for r in reqs if r.get("has_key")]
    return {
        "requests_total": len(reqs), "messages_requests": len(msgs),
        "keys_synthetic": (all(r.get("key_sha256") == synth for r in keyed) if keyed else None),
        "desc": {n: f"CANARY-DESC-{n}-{run}" in blob for n in fixture_skills},
        "body": {n: f"CANARY-BODY-{n}-{run}" in blob for n in fixture_skills},
        "agents": f"CANARY-AGENTS-project-{run}" in blob if msgs else None,
        "listed_fixture": sorted(n for n in listed if n.startswith("a0-")) if listed is not None else None,
        "listed_other": [n for n in listed if not n.startswith("a0-")] if listed is not None else None,
        "tool_results": tool_results,
    }


def verify_evidence(records, problems, req_lines, markers_files):
    """markers_files: {relpath: sha256} computed by the HOST after the run."""
    out = {"stream_problems": list(problems)}
    streamed = [r["sha256"] for r in records if r.get("kind") == "REQUEST"]
    on_disk = [hashlib.sha256(l.encode()).hexdigest() for l in req_lines]
    out["requests_match_stream"] = streamed == on_disk
    final = next((r for r in records if r.get("kind") == "FINAL"), None)
    if final is None:
        out["markers_match_final"] = None
    else:
        fm = {k: v for k, v in final["markers"].items()}
        hm = {k: v for k, v in markers_files.items() if k in fm}
        out["markers_match_final"] = fm == hm and set(fm) <= set(markers_files)
    out["tampered"] = bool(problems and any(p.startswith("bad_record") for p in problems)) \
        or out["requests_match_stream"] is False or out["markers_match_final"] is False
    out["complete"] = not problems and final is not None
    return out


def judge(case, res, run, fixture_skills):
    """res: dict with keys analysis, evidence, inside, isolation (dict of bools), validator,
    markers_present, work_ran, victim_present, stdout, stderr."""
    e = case.get("expect", {})
    a, ev = res["analysis"], res["evidence"]
    spec_errors = lint(case)
    iso = res["isolation"]
    checks, unknown = {}, []

    def put(name, value):
        if value is None:
            unknown.append(name)
        else:
            checks[name] = bool(value)

    lo, hi = e.get("requests", (None, None))
    if lo is not None:
        put("requests_in_range", lo <= a["messages_requests"] <= hi)
    if a["messages_requests"] > 0 or (lo is not None and lo > 0):
        put("keys_synthetic", a["keys_synthetic"])
    for n in e.get("desc_present", []):
        put(f"desc_present:{n}", a["desc"].get(n) if a["messages_requests"] else None)
    for n in e.get("desc_absent", []):
        put(f"desc_absent:{n}", (not a["desc"].get(n)) if a["messages_requests"] else None)
    for n in e.get("body_present", []):
        put(f"body_present:{n}", a["body"].get(n) if a["messages_requests"] else None)
    for n in e.get("body_absent", []):
        put(f"body_absent:{n}", not a["body"].get(n))
    if "agents" in e:
        put("agents", (a["agents"] == e["agents"]) if a["agents"] is not None else None)
    if "listed_fixture_exact" in e:
        put("listed_fixture_exact",
            (a["listed_fixture"] == sorted(e["listed_fixture_exact"])) if a["listed_fixture"] is not None else None)
    for m in e.get("markers_present", []):
        put(f"marker_present:{m}", m in res["markers_present"])
    for m in e.get("markers_absent", []):
        put(f"marker_absent:{m}", m not in res["markers_present"])
    for f in e.get("work_present", []):
        put(f"work_present:{f}", f in res["work_ran"] if res["work_ran"] is not None else None)
    for f in e.get("work_absent", []):
        put(f"work_absent:{f}", f not in res["work_ran"] if res["work_ran"] is not None else None)
    if "victim_present" in e:
        put("victim_present", (res["victim_present"] == e["victim_present"])
            if res["victim_present"] is not None else None)
    for s in e.get("stdout_contains", []):
        put(f"stdout_contains:{s[:30]}", s in (res["stdout"] or ""))
    for s in e.get("stdout_absent", []):
        put(f"stdout_absent:{s[:30]}", s not in (res["stdout"] or ""))
    if "tool_attempt" in e:
        ta = e["tool_attempt"]
        hits = [t for t in a["tool_results"] if t["id"] == ta.get("id", "toolu_mock_0")]
        put("tool_attempted", bool(hits))
        if hits:
            put("tool_result_text", ta["text_contains"] in hits[0]["text"])
            put("tool_result_is_error", hits[0]["is_error"] is ta["is_error"])
    if "exit_code" in e:
        put("exit_code", (res["inside"].get("exit") == e["exit_code"]) if res["inside"] else None)
    if "clean_exit" in e:
        put("clean_exit", ((res["inside"].get("exit") or {}).get("clean") is True) if res["inside"] else None)
    validator_match = None
    if "validator" in e:
        validator_match = (res["validator"] is not None and res["validator"]["accepted"] == e["validator"])
        put("validator_matches", validator_match)

    reasons = []
    if not all(iso.values()):
        verdict = "STOP_ISOLATION"
        reasons = [k for k, v in iso.items() if not v]
    elif ev["tampered"]:
        verdict, reasons = "FAIL", ["evidence_tampered"]
    elif spec_errors:
        verdict, reasons = "INCONCLUSIVE", ["spec:" + s for s in spec_errors]
    elif not ev["complete"] or res["inside"] is None:
        verdict, reasons = "INCONCLUSIVE", ["evidence_incomplete"] + ev["stream_problems"]
    elif any(v is False for v in checks.values()):
        verdict, reasons = "FAIL", sorted(k for k, v in checks.items() if not v)
    elif unknown:
        verdict, reasons = "INCONCLUSIVE", ["unknown:" + u for u in unknown]
    elif not checks:
        verdict, reasons = "INCONCLUSIVE", ["no_checks"]
    else:
        verdict = "PASS"
    return {"verdict": verdict, "reasons": reasons, "checks": checks, "unknown": unknown,
            "spec_errors": spec_errors, "validator_matches": validator_match}


def judge_pair(pos, ctl, pos_observable, ctl_observable):
    """A positive and its control must both PASS *and* their observable outcome must differ
    (e.g. the forbidden file exists only in the control). Equal observables = the control did
    not isolate the mechanism under test -> INCONCLUSIVE, never PASS."""
    if "STOP_ISOLATION" in (pos["verdict"], ctl["verdict"]):
        return {"verdict": "STOP_ISOLATION", "reason": "member_isolation"}
    if "FAIL" in (pos["verdict"], ctl["verdict"]):
        return {"verdict": "FAIL", "reason": "member_fail"}
    if pos["verdict"] != "PASS" or ctl["verdict"] != "PASS":
        return {"verdict": "INCONCLUSIVE", "reason": "member_not_pass"}
    if pos_observable is None or ctl_observable is None:
        return {"verdict": "INCONCLUSIVE", "reason": "observable_unknown"}
    if pos_observable == ctl_observable:
        return {"verdict": "INCONCLUSIVE", "reason": "control_not_discriminating"}
    return {"verdict": "PASS", "reason": "discriminated"}
