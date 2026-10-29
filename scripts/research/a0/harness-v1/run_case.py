"""Host-side orchestration of one case: fixtures, pre-launch validator verdict, sandboxed run,
oracle, real-system integrity, leftovers, owner-file preservation in the disposable tree and
cleanup of only harness-created subtrees. Imported by run_batch.py."""
import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import integrity  # noqa: E402
from overlay_validator import validate  # noqa: E402
from sandbox import SYNTH_KEY, argv, base_env  # noqa: E402

CLAUDE = os.path.realpath(os.path.expanduser("~/.local/share/claude/versions/2.1.312"))
FIXTURE_SKILLS = ["alpha", "beta", "gamma", "proj", "hidden", "ghost"]
INVENTORY = {"a0-alpha", "a0-beta", "a0-gamma", "a0-proj", "a0-hidden"}
KNOWN_PROXY = {"CONNECT downloads.claude.ai:443 HTTP/1.1", "CONNECT github.com:443 HTTP/1.1"}
HOOK = "/usr/bin/python3 -I /a0/harness/hooks/{} {}"


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
        h = dict(hooks)
        del h["PreToolUse"]
        return {"hooks": h, "permissions": {"allow": ["Bash(touch:*)"], "deny": ["Bash(rm:*)"]}}
    if name == "hooked_allow_rm":
        return {"hooks": hooks, "permissions": {"allow": ["Bash(touch:*)", "Bash(rm:*)"]}}
    raise ValueError(name)


def sha(p):
    return hashlib.sha256(open(p, "rb").read()).hexdigest() if os.path.isfile(p) else None


def tree(path):
    out = {}
    for d, dirs, files in os.walk(path):
        if ".git" in dirs:
            dirs.remove(".git")
        for n in files:
            p = os.path.join(d, n)
            out[os.path.relpath(p, path)] = "link" if os.path.islink(p) else sha(p)
    return dict(sorted(out.items()))


def owned(t, side):
    """Files the user/project owns (must never be changed by the provider or the overlay)."""
    if side == "home":
        return {k: v for k, v in t.items()
                if k == ".claude/settings.json" or k.startswith((".claude/skills/", ".claude/notskills/"))}
    return {k: v for k, v in t.items() if k != "a0-victim" and not k.startswith("ran-")}


def build(run, home, work, settings):
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
    subprocess.run(["git", "init", "-q", work], check=True, env={"PATH": "/usr/bin"},
                   stdin=subprocess.DEVNULL)


def leftovers(root):
    found = []
    for p in os.listdir("/proc"):
        if not p.isdigit() or int(p) == os.getpid():
            continue
        try:
            exe = os.readlink(f"/proc/{p}/exe")
            cmd = open(f"/proc/{p}/cmdline", "rb").read().replace(b"\0", b" ").decode("utf-8", "replace")
        except OSError:
            continue
        if "/opt/a0/" in exe or (exe.endswith("/bwrap") and root in cmd) or \
                (exe.endswith("python3") and "/a0/harness/case_inside" in cmd):
            found.append({"pid": int(p), "exe": exe})
    return found


def analyse(run, markers):
    reqs = [json.loads(l) for l in open(os.path.join(markers, "requests.jsonl"))] \
        if os.path.exists(os.path.join(markers, "requests.jsonl")) else []
    msgs = [r for r in reqs if r["path"].startswith("/v1/messages")
            and not r["path"].startswith("/v1/messages/count_tokens")]
    blob = "\n".join(json.dumps(r["body"]) for r in msgs)
    synth = hashlib.sha256(SYNTH_KEY.encode()).hexdigest()
    listed = []
    if msgs:
        first = json.dumps(msgs[0]["body"].get("messages"))
        parts = first.split("The following skills are available for use with the Skill tool:")
        if len(parts) > 1:
            listed = re.findall(r"\\n- ([A-Za-z0-9:_-]+): ", parts[1])
    tool_results = []
    for r in msgs[1:]:
        for m in (r["body"].get("messages") or []):
            if isinstance(m.get("content"), list):
                for c in m["content"]:
                    if c.get("type") == "tool_result":
                        txt = json.dumps(c.get("content"))[:300]
                        tool_results.append({"is_error": c.get("is_error"), "text": txt})
    proxy = [l.strip() for l in open(os.path.join(markers, "proxy-attempts.log")) if l.strip()] \
        if os.path.exists(os.path.join(markers, "proxy-attempts.log")) else []
    return {
        "requests_total": len(reqs), "messages_requests": len(msgs),
        "paths": sorted({f"{r['method']} {r['path'].split('?')[0]}" for r in reqs}),
        "all_keys_synthetic": all(r["key_sha256"] == synth for r in reqs if r["has_key"]),
        "desc": {n: f"CANARY-DESC-{n}-{run}" in blob for n in FIXTURE_SKILLS},
        "body": {n: f"CANARY-BODY-{n}-{run}" in blob for n in FIXTURE_SKILLS},
        "agents": f"CANARY-AGENTS-project-{run}" in blob,
        "listed_fixture": sorted(n for n in listed if n.startswith("a0-")),
        "listed_builtin": [n for n in listed if not n.startswith("a0-")],
        "tool_results": tool_results,
        "proxy_attempts": proxy,
        "proxy_unexpected": sorted(set(proxy) - KNOWN_PROXY),
    }


def run_case(root, case):
    run = os.path.basename(root)
    ev = os.path.join(root, "evidence", "isolation", case["id"])
    cdir = os.path.join(root, "cases", case["id"])
    home = os.path.join(root, "homes", case["id"])
    work = os.path.join(root, "work", case["id"])
    markers = os.path.join(ev, "markers")
    for d in (ev, cdir, home, work, markers):
        assert os.path.realpath(d).startswith(root + os.sep)
        os.makedirs(d, mode=0o700, exist_ok=False)
    build(run, home, work, case.get("settings", "plain"))
    overlay_raw = case.get("overlay_raw")
    if overlay_raw is None and case.get("overlay") is not None:
        overlay_raw = json.dumps(case["overlay"], indent=1)
    if overlay_raw is not None:
        open(os.path.join(cdir, "overlay.json"), "w").write(overlay_raw)
    spec = {"mode": case.get("mode", "H"), "prompt": case.get("prompt", "ping"),
            "script": case.get("script", [{"text": "ok from mock"}]), "args": case.get("args", [])}
    json.dump(spec, open(os.path.join(cdir, "spec.json"), "w"), indent=1)
    validator = None
    if overlay_raw is not None:
        validator = validate(overlay_raw.encode(), INVENTORY)
        validator = {"accepted": not validator, "errors": validator}
    before = {"home": tree(home), "work": tree(work)}
    integrity.main(os.path.join(ev, "integrity-before.json"))
    cmd = argv(home=home, work=work, markers=markers, harness=HERE, env=base_env(), claude_binary=CLAUDE,
               extra_ro=[(cdir, "/a0/case")],
               command=["/usr/bin/python3", "-I", "/a0/harness/case_inside.py"])
    open(os.path.join(ev, "command.txt"), "w").write(" ".join(shlex.quote(c) for c in cmd) + "\n")
    shutil.copy(os.path.join(cdir, "spec.json"), os.path.join(ev, "spec.json"))
    if overlay_raw is not None:
        shutil.copy(os.path.join(cdir, "overlay.json"), os.path.join(ev, "overlay.json"))
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=300, stdin=subprocess.DEVNULL,
                       close_fds=True, env={})
    open(os.path.join(ev, "bwrap.stderr.txt"), "w").write(r.stderr)
    integrity.main(os.path.join(ev, "integrity-after.json"))
    cmp = subprocess.run([sys.executable, "-I", os.path.join(HERE, "compare_integrity.py"),
                          os.path.join(ev, "integrity-before.json"), os.path.join(ev, "integrity-after.json")],
                         capture_output=True, text=True)
    after = {"home": tree(home), "work": tree(work)}
    owned_changed = sorted(
        [f"home:{k}" for k in set(owned(before["home"], "home")) | set(owned(after["home"], "home"))
         if owned(before["home"], "home").get(k) != owned(after["home"], "home").get(k)] +
        [f"work:{k}" for k in set(owned(before["work"], "work")) | set(owned(after["work"], "work"))
         if owned(before["work"], "work").get(k) != owned(after["work"], "work").get(k)])
    provider_writes = sorted(k for k in set(before["home"]) | set(after["home"])
                             if before["home"].get(k) != after["home"].get(k))
    victim_present = os.path.exists(os.path.join(work, "a0-victim"))
    work_ran = sorted(n for n in os.listdir(work) if n.startswith("ran-"))
    left = leftovers(root)
    a = analyse(run, markers)
    inside = json.load(open(os.path.join(markers, "inside-result.json"))) \
        if os.path.exists(os.path.join(markers, "inside-result.json")) else {}
    out_text = open(os.path.join(markers, "stdout.txt"), errors="replace").read() \
        if os.path.exists(os.path.join(markers, "stdout.txt")) else ""
    err_text = open(os.path.join(markers, "stderr.txt"), errors="replace").read() \
        if os.path.exists(os.path.join(markers, "stderr.txt")) else ""
    markers_present = sorted(n for n in os.listdir(markers)
                             if n.startswith(("hook-", "guard-", "ran-")))
    for d in (home, work, cdir):
        shutil.rmtree(d)
    cleanup = {"home_removed": not os.path.exists(home), "work_removed": not os.path.exists(work),
               "case_dir_removed": not os.path.exists(cdir), "evidence_kept": os.path.isdir(ev)}
    res = {"id": case["id"], "group": case["group"], "purpose": case["purpose"],
           "validator": validator, "inside": inside, "analysis": a,
           "stdout_excerpt": out_text[:400], "stderr_excerpt": err_text[:600],
           "markers_present": markers_present, "victim_present": victim_present, "work_ran_files": work_ran,
           "integrity_pass": cmp.returncode == 0, "leftovers": left,
           "owned_files_changed": owned_changed, "provider_writes_home": provider_writes,
           "bwrap_exit": r.returncode, "cleanup": cleanup}
    json.dump(res, open(os.path.join(ev, "result.json"), "w"), indent=1)
    return res
