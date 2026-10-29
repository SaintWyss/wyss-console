"""A-1 orchestrator (host side). Builds disposable fixtures, runs one mode inside the sandbox,
judges with the pre-registered oracle, checks real-system integrity and leftover processes,
removes only the subtrees it created. Usage: python3 -I run_a1.py <A0_ROOT> H|I
"""
import hashlib
import json
import os
import shlex
import shutil
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import integrity  # noqa: E402
from sandbox import SYNTH_KEY, argv, base_env  # noqa: E402

ROOT = os.path.realpath(sys.argv[1])
MODE = sys.argv[2]
LABEL = sys.argv[3] if len(sys.argv) > 3 else ""
assert MODE in ("H", "I")
RUN = os.path.basename(ROOT)
H = os.path.join(ROOT, "harness")
EV = os.path.join(ROOT, "evidence", f"A1-{MODE}{LABEL}")
CLAUDE = os.path.realpath(os.path.expanduser("~/.local/share/claude/versions/2.1.312"))
CLAUDE_SHA = "4f4178ce41d83e23341ebfbaecdbcf8ea6bf7baea0690025abe49eeec3a5d01d"
POS = ["alpha", "beta", "gamma", "proj"]
NEG = ["hidden", "ghost"]


def inside(path):
    p = os.path.realpath(path)
    assert p.startswith(ROOT + os.sep), p
    return p


def canary(kind, name):
    return f"CANARY-{kind}-{name}-{RUN}"


def skill(dirpath, name, extra=""):
    os.makedirs(dirpath, mode=0o700, exist_ok=True)
    with open(os.path.join(dirpath, "SKILL.md"), "w") as f:
        f.write(f"---\nname: a0-{name}\ndescription: A0 fixture skill {name}. {canary('DESC', name)}\n"
                f"{extra}---\n\n{canary('BODY', name)}\n")


def sha(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest() if os.path.exists(path) else None


def tree(path):
    out = {}
    for d, _, files in os.walk(path):
        for n in files:
            p = os.path.join(d, n)
            if os.path.islink(p):
                out[os.path.relpath(p, path)] = "link"
            else:
                out[os.path.relpath(p, path)] = sha(p)
    return dict(sorted(out.items()))


def build(home, work):
    c = os.path.join(home, ".claude")
    for n in ("alpha", "beta", "gamma"):
        skill(os.path.join(c, "skills", f"a0-{n}"), n)
    skill(os.path.join(c, "skills", "a0-hidden"), "hidden", "disable-model-invocation: true\n")
    skill(os.path.join(c, "notskills", "a0-ghost"), "ghost")
    open(os.path.join(c, "settings.json"), "w").write("{}\n")
    state = {"hasCompletedOnboarding": True, "lastOnboardingVersion": "2.1.312", "numStartups": 1,
             "theme": "dark",
             "customApiKeyResponses": {"approved": [SYNTH_KEY[-20:]], "rejected": []},
             "projects": {"/work": {"hasTrustDialogAccepted": True,
                                    "hasCompletedProjectOnboarding": True,
                                    "projectOnboardingSeenCount": 1, "allowedTools": []}}}
    json.dump(state, open(os.path.join(home, ".claude.json"), "w"), indent=1)
    skill(os.path.join(work, ".claude", "skills", "a0-proj"), "proj")
    open(os.path.join(work, "AGENTS.md"), "w").write(f"# A0 project\n\n{canary('AGENTS', 'project')}\n")
    open(os.path.join(work, "CLAUDE.md"), "w").write("@AGENTS.md\n")
    subprocess.run(["git", "init", "-q", work], check=True, env={"PATH": "/usr/bin"},
                   stdin=subprocess.DEVNULL)


def leftovers():
    found = []
    for p in os.listdir("/proc"):
        if not p.isdigit():
            continue
        try:
            cmd = open(f"/proc/{p}/cmdline", "rb").read().replace(b"\0", b" ").decode("utf-8", "replace")
            exe = os.readlink(f"/proc/{p}/exe")
        except OSError:
            continue
        if int(p) == os.getpid():
            continue
        if "/opt/a0/" in exe or "/a0/harness" in cmd or (exe.endswith("/bwrap") and ROOT in cmd):
            found.append({"pid": int(p), "exe": exe, "cmd": cmd[:200]})
    return found


def oracle(markers):
    reqs = []
    rp = os.path.join(markers, "requests.jsonl")
    if os.path.exists(rp):
        reqs = [json.loads(l) for l in open(rp)]
    msgs = [r for r in reqs if r["path"].startswith("/v1/messages")
            and not r["path"].startswith("/v1/messages/count_tokens")]
    blob = "\n".join(json.dumps(r["body"]) for r in msgs)
    synth = hashlib.sha256(SYNTH_KEY.encode()).hexdigest()
    res = {
        "requests_total": len(reqs), "messages_requests": len(msgs),
        "paths": sorted({f"{r['method']} {r['path'].split('?')[0]}" for r in reqs}),
        "all_keys_synthetic": all(r["key_sha256"] == synth for r in reqs if r["has_key"]),
        "desc_present": {n: canary("DESC", n) in blob for n in POS + NEG},
        "body_present": {n: canary("BODY", n) in blob for n in POS + NEG},
        "agents_present": canary("AGENTS", "project") in blob,
        "proxy_attempts": [l.strip() for l in open(os.path.join(markers, "proxy-attempts.log"))
                           if l.strip()] if os.path.exists(os.path.join(markers, "proxy-attempts.log")) else None,
    }
    res["oracle_pass"] = (
        len(msgs) >= 1 and res["all_keys_synthetic"]
        and all(res["desc_present"][n] for n in POS)
        and not any(res["desc_present"][n] for n in NEG)
        and not any(res["body_present"].values())
        and res["agents_present"])
    return res


def observe(markers):
    import re
    obs = {}
    rp = os.path.join(markers, "requests.jsonl")
    if os.path.exists(rp):
        first = json.loads(open(rp).readline())["body"]
        msgs = json.dumps(first.get("messages"))
        block = msgs.split("The following skills are available for use with the Skill tool:")
        listed = re.findall(r"\\n- ([A-Za-z0-9:_-]+): ", block[1]) if len(block) > 1 else []
        obs["skill_listing_location"] = "messages" if len(block) > 1 else "not_found_in_messages"
        obs["listed_skills"] = listed
        obs["builtin_skills"] = [n for n in listed if not n.startswith("a0-")]
        obs["model"] = first.get("model")
        obs["tool_names"] = [t.get("name") for t in first.get("tools", [])]
    sp = os.path.join(markers, "screen.txt")
    if os.path.exists(sp):
        t = open(sp, errors="replace").read()
        obs["banner_auto_mode_default"] = "Auto mode is now Claude Code" in t
        obs["status_auto_mode_on"] = "auto mode on" in t
        obs["billing_label"] = "API Usage Billing" in t
    return obs


def main():
    os.makedirs(EV, mode=0o700, exist_ok=False)
    home = inside(os.path.join(ROOT, "homes", f"a1-{MODE}{LABEL}"))
    work = inside(os.path.join(ROOT, "work", f"a1-{MODE}{LABEL}"))
    markers = inside(os.path.join(EV, "markers"))
    for d in (home, work, markers):
        os.makedirs(d, mode=0o700, exist_ok=False)
    build(home, work)
    fixtures_before = {"home": tree(home), "work": tree(work)}
    json.dump(fixtures_before, open(os.path.join(EV, "fixtures-before.json"), "w"), indent=1)
    integrity.main(os.path.join(EV, "integrity-before.json"))
    env = base_env()
    cmd = argv(home=home, work=work, markers=markers, harness=H, env=env, claude_binary=CLAUDE,
               command=["/usr/bin/python3", "-I", "/a0/harness/a1_inside.py", MODE])
    open(os.path.join(EV, "command.txt"), "w").write(" ".join(shlex.quote(c) for c in cmd) + "\n")
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=300,
                       stdin=subprocess.DEVNULL, close_fds=True, env={})
    open(os.path.join(EV, "bwrap.stdout.txt"), "w").write(r.stdout)
    open(os.path.join(EV, "bwrap.stderr.txt"), "w").write(r.stderr)
    integrity.main(os.path.join(EV, "integrity-after.json"))
    cmp = subprocess.run([sys.executable, "-I", os.path.join(H, "compare_integrity.py"),
                          os.path.join(EV, "integrity-before.json"),
                          os.path.join(EV, "integrity-after.json")], capture_output=True, text=True)
    fixtures_after = {"home": tree(home), "work": tree(work)}
    json.dump(fixtures_after, open(os.path.join(EV, "fixtures-after.json"), "w"), indent=1)
    left = leftovers()
    o = oracle(markers)
    inside_res = json.load(open(os.path.join(markers, "inside-result.json"))) \
        if os.path.exists(os.path.join(markers, "inside-result.json")) else None
    out = os.path.join(markers, "stdout.txt" if MODE == "H" else "screen.txt")
    ok_text = os.path.exists(out) and "ok from mock" in open(out, errors="replace").read()
    settings_same = fixtures_before["home"].get(".claude/settings.json") == \
        fixtures_after["home"].get(".claude/settings.json")
    changed = sorted(k for k in set(fixtures_before["home"]) | set(fixtures_after["home"])
                     if fixtures_before["home"].get(k) != fixtures_after["home"].get(k))
    # Cleanup: only the subtrees created above.
    shutil.rmtree(home)
    shutil.rmtree(work)
    cleanup = {"home_removed": not os.path.exists(home), "work_removed": not os.path.exists(work),
               "evidence_kept": os.path.isdir(EV)}
    exit_ok = (inside_res or {}).get("exit") == 0 if MODE == "H" else \
        inside_res is not None and not inside_res.get("timed_out") \
        and (inside_res.get("exit") or {}).get("clean") is True
    summary = {
        "mode": MODE, "bwrap_exit": r.returncode, "inside": inside_res,
        "oracle": o, "ok_from_mock_in_output": ok_text,
        "integrity": json.loads(cmp.stdout) if cmp.stdout.strip() else cmp.stderr,
        "integrity_exit": cmp.returncode,
        "disposable_settings_unchanged": settings_same,
        "disposable_home_files_changed_or_created": changed,
        "leftover_processes": left, "cleanup": cleanup,
    }
    known_proxy = {"CONNECT downloads.claude.ai:443 HTTP/1.1", "CONNECT github.com:443 HTTP/1.1"}
    summary["proxy_unexpected"] = sorted(set(o["proxy_attempts"] or []) - known_proxy)
    summary["observations"] = observe(markers)
    summary["verdict"] = "PASS" if (exit_ok and o["oracle_pass"] and ok_text and cmp.returncode == 0
                                    and not left and settings_same and r.returncode == 0
                                    and not summary["proxy_unexpected"]
                                    and all(cleanup.values())) else \
        ("INCONCLUSIVE" if (inside_res or {}).get("timed_out") or o["messages_requests"] == 0 else "FAIL")
    json.dump(summary, open(os.path.join(EV, "summary.json"), "w"), indent=1)
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
