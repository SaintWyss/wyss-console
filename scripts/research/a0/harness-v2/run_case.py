"""Host-side orchestration of one v2 case (no default provider launch in this step's tests:
offline tests set spec `sut_argv` to fake_provider.py).

Fixes vs v1 (independent review 2026-10-29): fixtures staged read-only and copied into sized
tmpfs inside the sandbox; enforcement launcher (timeout, storage cap, PID-namespace leftovers,
stream cap); HMAC-chained live evidence verified against disk; validator result part of the
verdict; per-case freeze manifest of ALL evidence written host-side after the run; cleanup of
staging in `finally`; refuses to launch if the A0 root exceeds ROOT_CAP.
"""
import hashlib
import json
import os
import secrets
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import integrity  # noqa: E402
import limits  # noqa: E402
import oracle  # noqa: E402
import stream  # noqa: E402
from launch import run_sandbox  # noqa: E402
from overlay_validator import validate  # noqa: E402
from sandbox import SYNTH_KEY, argv, base_env  # noqa: E402

CLAUDE = os.path.realpath(os.path.expanduser("~/.local/share/claude/versions/2.1.312"))
FIXTURE_SKILLS = ["alpha", "beta", "gamma", "proj", "hidden", "ghost"]
INVENTORY = {"a0-alpha", "a0-beta", "a0-gamma", "a0-proj", "a0-hidden"}
KNOWN_PROXY = {"CONNECT downloads.claude.ai:443 HTTP/1.1", "CONNECT github.com:443 HTTP/1.1"}
HOOK = "/usr/bin/python3 -I /a0/harness/hooks/{} {}"


def harness_tree_hash():
    h = hashlib.sha256()
    for d, dirs, files in os.walk(HERE):
        dirs[:] = sorted(x for x in dirs if x != "__pycache__")
        for n in sorted(files):
            p = os.path.join(d, n)
            h.update(os.path.relpath(p, HERE).encode() + b"\0" + hashlib.sha256(open(p, "rb").read()).digest())
    return h.hexdigest()


def settings_variant(name):
    if name == "plain":
        return {}
    hooks = {
        "SessionStart": [{"hooks": [{"type": "command", "command": HOOK.format("marker.py", "SessionStart")}]}],
        "UserPromptSubmit": [{"hooks": [{"type": "command", "command": HOOK.format("marker.py", "UserPromptSubmit")}]}],
        "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": HOOK.format("guard.py", "")}]}],
    }
    if name == "hooked":
        return {"hooks": hooks, "permissions": {"allow": ["Bash(touch:*)"], "deny": ["Bash(rm:*)"]}}
    if name == "hooked_noguard":
        h = {k: v for k, v in hooks.items() if k != "PreToolUse"}
        return {"hooks": h, "permissions": {"allow": ["Bash(touch:*)"], "deny": ["Bash(rm:*)"]}}
    if name == "hooked_allow_rm":
        return {"hooks": hooks, "permissions": {"allow": ["Bash(touch:*)", "Bash(rm:*)"]}}
    raise ValueError(name)


def stage_fixture(run, fx, settings):
    home, work = os.path.join(fx, "home"), os.path.join(fx, "work")
    c = os.path.join(home, ".claude")

    def skill(d, name, extra=""):
        os.makedirs(d, mode=0o700, exist_ok=True)
        open(os.path.join(d, "SKILL.md"), "w").write(
            f"---\nname: a0-{name}\ndescription: A0 fixture skill {name}. CANARY-DESC-{name}-{run}\n"
            f"{extra}---\n\nCANARY-BODY-{name}-{run}\n")
    for n in ("alpha", "beta", "gamma"):
        skill(os.path.join(c, "skills", f"a0-{n}"), n)
    skill(os.path.join(c, "skills", "a0-hidden"), "hidden", "disable-model-invocation: true\n")
    skill(os.path.join(c, "notskills", "a0-ghost"), "ghost")
    json.dump(settings_variant(settings), open(os.path.join(c, "settings.json"), "w"), indent=1)
    state = {"hasCompletedOnboarding": True, "lastOnboardingVersion": "2.1.312", "numStartups": 1,
             "theme": "dark", "customApiKeyResponses": {"approved": [SYNTH_KEY[-20:]], "rejected": []},
             "projects": {"/work": {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True,
                                    "projectOnboardingSeenCount": 1, "allowedTools": []}}}
    json.dump(state, open(os.path.join(home, ".claude.json"), "w"), indent=1)
    skill(os.path.join(work, ".claude", "skills", "a0-proj"), "proj")
    open(os.path.join(work, "AGENTS.md"), "w").write(f"# A0 project\n\nCANARY-AGENTS-project-{run}\n")
    open(os.path.join(work, "CLAUDE.md"), "w").write("@AGENTS.md\n")
    open(os.path.join(work, "a0-victim"), "w").write("victim\n")
    subprocess.run(["git", "init", "-q", work], check=True, env={"PATH": "/usr/bin"}, stdin=subprocess.DEVNULL)


def owned(t, side):
    if side == "home":
        return {k: v for k, v in t.items()
                if k == ".claude/settings.json" or k.startswith((".claude/skills/", ".claude/notskills/"))}
    return {k: v for k, v in t.items() if k != "a0-victim" and not k.startswith("ran-")}


def reconstruct(records, markers):
    """Write exported /markers files host-side from authenticated FILE records only."""
    import base64
    bufs, problems = {}, []
    for rec in records:
        k = rec.get("kind")
        if k not in ("FILE_CHUNK", "FILE_END"):
            continue
        rel = os.path.normpath(rec.get("path", ""))
        if rel.startswith(("..", "/")) or rel in ("", "."):
            problems.append(f"bad_export_path:{rel[:40]}")
            continue
        if k == "FILE_CHUNK":
            bufs.setdefault(rel, bytearray()).extend(base64.b64decode(rec["b64"]))
        else:
            data = bytes(bufs.pop(rel, b""))
            if hashlib.sha256(data).hexdigest() != rec["sha256"] or len(data) != rec["size"]:
                problems.append(f"export_digest_mismatch:{rel[:40]}")
                continue
            dst = os.path.join(markers, rel)
            os.makedirs(os.path.dirname(dst), mode=0o700, exist_ok=True)
            open(dst, "wb").write(data)
    if bufs:
        problems.append("export_incomplete")
    return problems


def file_hashes(path):
    out = {}
    for d, _, files in os.walk(path):
        for n in files:
            p = os.path.join(d, n)
            if not os.path.islink(p):
                out[os.path.relpath(p, path)] = hashlib.sha256(open(p, "rb").read()).hexdigest()
    return dict(sorted(out.items()))


def run_case(root, ev_base, case, *, timeout=180, markers_cap=limits.MARKERS_CAP,
             root_cap=limits.ROOT_CAP, sizes=None, rlimits=None, inject_failure=None,
             allow_provider=False):
    run = os.path.basename(root)
    if not case.get("sut_argv") and not allow_provider:
        # Hard guard: this step authorizes no provider execution.
        return {"id": case["id"], "refused": "provider_execution_not_authorized", "verdict": "REFUSED"}
    if case.get("sut_argv") and "/opt/a0/bin/claude" in case["sut_argv"] and not allow_provider:
        return {"id": case["id"], "refused": "provider_execution_not_authorized", "verdict": "REFUSED"}
    ev = os.path.join(ev_base, case["id"])
    staging = os.path.join(root, "cases-v2", case["id"])
    markers = os.path.join(ev, "markers")
    for d in (ev, staging, markers):
        assert os.path.realpath(d).startswith(root + os.sep), d
    used = limits.du(root)
    if used > root_cap:
        return {"id": case["id"], "refused": f"root_cap_exceeded:{used}>{root_cap}", "verdict": "STOP_STORAGE"}
    os.makedirs(ev, mode=0o700, exist_ok=False)
    os.makedirs(markers, mode=0o700)
    result = {"id": case["id"]}
    try:
        os.makedirs(staging, mode=0o700, exist_ok=False)
        fx = os.path.join(staging, "fixture")
        stage_fixture(run, fx, case.get("settings", "plain"))
        cdir = os.path.join(staging, "case")
        os.makedirs(cdir, mode=0o700)
        overlay_raw = case.get("overlay_raw")
        if overlay_raw is None and case.get("overlay") is not None:
            overlay_raw = json.dumps(case["overlay"], indent=1)
        validator = None
        if overlay_raw is not None:
            open(os.path.join(cdir, "overlay.json"), "w").write(overlay_raw)
            errs = validate(overlay_raw.encode(), INVENTORY)
            validator = {"accepted": not errs, "errors": errs}
        spec = {"id": case["id"], "mode": case.get("mode", "H"), "prompt": case.get("prompt", "ping"),
                "script": case.get("script", [{"text": "ok from mock"}]), "args": case.get("args", []),
                "sut_argv": case.get("sut_argv"), "sut_timeout": case.get("sut_timeout", 120)}
        json.dump(spec, open(os.path.join(cdir, "spec.json"), "w"), indent=1)
        fixture_digest = file_hashes(fx)
        integrity.main(os.path.join(ev, "integrity-before.json"))
        key = secrets.token_bytes(32)
        env = base_env({"A0_RUN": run, **case.get("sut_env", {})})
        sz = sizes or {}
        build = lambda fd: argv(harness=HERE, env=env, fixture=fx,  # noqa: E731
                                claude_binary=CLAUDE if allow_provider else None,
                                extra_ro=[(cdir, "/a0/case")], status_fd=fd,
                                command=["/usr/bin/python3", "-I", "/a0/harness/case_inside.py"],
                                **{k: v for k, v in sz.items() if k in ("home_size", "work_size", "tmp_size", "markers_size")})
        r = run_sandbox(build, key=key.hex().encode(), timeout=timeout,
                        rlimits=rlimits, inject_failure=inject_failure)
        integrity.main(os.path.join(ev, "integrity-after.json"))
        cmp = subprocess.run([sys.executable, "-I", os.path.join(HERE, "compare_integrity.py"),
                              os.path.join(ev, "integrity-before.json"), os.path.join(ev, "integrity-after.json")],
                             capture_output=True, text=True)
        open(os.path.join(ev, "stream.log"), "w").write(r["stdout"])  # host-only copy, outside /markers
        open(os.path.join(ev, "bwrap.stderr.txt"), "w").write(r["stderr"])
        json.dump({k: v for k, v in r.items() if k not in ("stdout", "stderr")}, open(os.path.join(ev, "launcher.json"), "w"), indent=1)
        records, problems, foreign = stream.verify(key, r["stdout"])
        del key
        export_problems = reconstruct(records, markers)
        problems = list(problems) + export_problems
        req_lines = open(os.path.join(markers, "requests.jsonl")).read().splitlines(keepends=True) \
            if os.path.exists(os.path.join(markers, "requests.jsonl")) else []
        mf = file_hashes(markers)
        evidence = oracle.verify_evidence(records, problems, req_lines, mf)
        evidence["foreign_stdout_lines"] = foreign
        fixture_rec = next((x for x in records if x.get("kind") == "FIXTURE"), None)
        final = next((x for x in records if x.get("kind") == "FINAL"), None)
        done = next((x for x in records if x.get("kind") == "SUT_DONE"), None)
        inside = done["result"] if done else None
        analysis = oracle.analyse(run, req_lines, FIXTURE_SKILLS)
        proxy = [l.strip() for l in open(os.path.join(markers, "proxy-attempts.log")) if l.strip()] \
            if os.path.exists(os.path.join(markers, "proxy-attempts.log")) else []
        owned_changed = None
        work_ran = victim = None
        if fixture_rec and final:
            owned_changed = sorted(
                [f"home:{k}" for k in set(owned(fixture_rec["home"], "home")) | set(owned(final["home"], "home"))
                 if owned(fixture_rec["home"], "home").get(k) != owned(final["home"], "home").get(k)] +
                [f"work:{k}" for k in set(owned(fixture_rec["work"], "work")) | set(owned(final["work"], "work"))
                 if owned(fixture_rec["work"], "work").get(k) != owned(final["work"], "work").get(k)])
            work_ran = sorted(k for k in final["work"] if k.startswith("ran-"))
            victim = "a0-victim" in final["work"]
        isolation = {"integrity": cmp.returncode == 0, "no_leftovers": not r["leftovers"],
                     "no_unexpected_proxy": not (set(proxy) - KNOWN_PROXY),
                     "owned_files_unchanged": owned_changed == [] if owned_changed is not None else True,
                     "not_killed_for_storage": r["killed"] not in ("markers_cap", "stream_cap")}
        stdout_txt = (inside or {}).get("stdout", "")
        res = {"analysis": analysis, "evidence": evidence, "inside": inside, "isolation": isolation,
               "validator": validator,
               "markers_present": sorted(n for n in os.listdir(markers) if n.startswith(("hook-", "guard-"))),
               "work_ran": work_ran, "victim_present": victim, "stdout": stdout_txt,
               "stderr": (inside or {}).get("stderr", "")}
        j = oracle.judge(case, res, run, FIXTURE_SKILLS)
        if r["timed_out"] and j["verdict"] == "PASS":
            j = {**j, "verdict": "INCONCLUSIVE", "reasons": ["timed_out"]}
        result.update(verdict=j["verdict"], reasons=j["reasons"], judgement=j, launcher={
            k: r[k] for k in ("exit", "timed_out", "killed", "members_max", "leftovers", "pid_ns", "duration_s",
                              "markers_peak")}, validator=validator, evidence=evidence,
            proxy=proxy, owned_changed=owned_changed,
            sut_stdout=res["stdout"][:2000], sut_stderr=res["stderr"][:2000], fixture_digest_sha256=hashlib.sha256(
                json.dumps(fixture_digest, sort_keys=True).encode()).hexdigest())
    except Exception as e:
        result.update(verdict="INCONCLUSIVE", reasons=[f"harness_exception:{type(e).__name__}:{e}"])
    finally:
        shutil.rmtree(staging, ignore_errors=True)
        result["cleanup"] = {"staging_removed": not os.path.exists(staging), "evidence_kept": os.path.isdir(ev)}
        if not result["cleanup"]["staging_removed"] and result.get("verdict") == "PASS":
            result["verdict"] = "FAIL"
            result.setdefault("reasons", []).append("cleanup_failed")
        json.dump(result, open(os.path.join(ev, "result.json"), "w"), indent=1, default=str)
        # Per-case freeze manifest, host-side, after every evidence file is final.
        man = {"harness_v2_tree_sha256": harness_tree_hash(),
               "files": {os.path.relpath(p, ev): hashlib.sha256(open(p, "rb").read()).hexdigest()
                         for d, _, fs in os.walk(ev) for p in (os.path.join(d, n) for n in fs)
                         if os.path.basename(p) != "case-manifest.json"}}
        json.dump(man, open(os.path.join(ev, "case-manifest.json"), "w"), indent=1, sort_keys=True)
    return result
