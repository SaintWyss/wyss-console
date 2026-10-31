#!/usr/bin/env python3
"""Real-provider acceptance of agent delegation against the installed terminal host.

Each scenario starts a real orchestrator session through the host (`native_start` with
`delegation: true`), hands it one bounded instruction as its first message, waits for the
delegation records it produces, and judges them from the records on disk and the parent's
screen, never from what the agents say alone.

    python3 -I scripts/qa/delegation_acceptance.py list
    python3 -I scripts/qa/delegation_acceptance.py run claude-to-claude --out <dir>

Scenarios that need Codex inference (`claude-to-codex`, `codex-to-claude`, `concurrency-codex`)
spend Codex quota: run them only when it is available. Every scenario uses a disposable parent
folder that the provider must already trust (`--parent-dir`); the script never answers a trust
prompt, it stops instead. Nothing here reads credentials or changes provider configuration.
"""

import argparse
import base64
import json
import os
import re
import socket
import struct
import sys
import time
from pathlib import Path

STATE = Path.home() / ".local/state/wyss-console/delegations"


def socket_path():
    runtime = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"
    return os.environ.get("WYSS_QA_HOST_SOCKET", f"{runtime}/wyss-console/host.sock")


def call(op, params=None, timeout=60):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(timeout)
        stream.connect(socket_path())
        data = json.dumps({"op": op, "params": params or {}}).encode()
        stream.sendall(struct.pack(">I", len(data)) + data)

        def read(n):
            out = b""
            while len(out) < n:
                part = stream.recv(n - len(out))
                if not part:
                    raise RuntimeError("host disconnected")
                out += part
            return out

        reply = json.loads(read(struct.unpack(">I", read(4))[0]))
    if "err" in reply:
        raise RuntimeError(json.dumps(reply["err"]))
    return reply["ok"]


def capability(provider, name):
    for item in call("delegate_capabilities", {"provider": provider, "query": name}):
        if item["name"] == name and item["compatibility"] == "supported":
            return {"capability": item["capability"], "revision": item["revision"]}
    raise SystemExit(f"no supported {provider} capability named {name}")


def screen_text(terminal_id):
    shot = call("term_screen", {"terminal_id": terminal_id, "scrollback_lines": 400})
    text = base64.b64decode(shot.get("repaint", "")).decode("utf-8", "replace")
    text = re.sub(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)", "", text)
    text = re.sub(r"\x1b\[[0-9;?]*[ -/]*[@-~]", " ", text)
    text = re.sub(r"\x1b[()][A-Z0-9]|\x1b[=>78]|[\x0e\x0f]", "", text)
    return re.sub(r"[ \t]+", " ", text)


def records(parent_session):
    found = []
    if not STATE.is_dir():
        return found
    for entry in STATE.iterdir():
        path = entry / "record.json"
        try:
            record = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        if record.get("parent_session") == parent_session:
            found.append(record)
    return sorted(found, key=lambda r: r.get("created_at", 0))


def child_task(marker, keep, absent, provider):
    use = (
        f"Use the exposed {keep} skill via the Skill tool"
        if provider == "claude_code"
        else f"Use the exposed {keep} skill: read its SKILL.md using the supported skill mechanism"
    )
    return (
        f"{use}, then write hello.txt containing {marker} in your current workspace. "
        f"Confirm {absent} is absent from the exposed skill catalogue; do not search the "
        f"filesystem for excluded skills. Reply with the marker and one rule from the skill. "
        + (
            "Do not delegate, run shell, access credentials or use network."
            if provider == "claude_code"
            else "Do not delegate, access credentials or use network."
        )
    )


def single_brief(s, skill):
    request = {
        "provider": s["worker"],
        "model": s["model"],
        "effort": s["effort"],
        "timeout_minutes": 3,
        "title": s["marker"],
        "skills": [skill],
        "task": child_task(s["marker"], s["keep"], s["absent"], s["worker"]),
    }
    return (
        "# Wyss delegation acceptance\n\n"
        "Harmless installed Wyss Console acceptance task. Use only the Wyss delegation MCP "
        f"tools. Call wyss_delegate with exactly this request: {json.dumps(request)}. Wait "
        "with wyss_delegation_wait until it ends. Then reply PARENT_RECEIVED_"
        f"{s['marker']} followed by the delegation's status and its verification state as "
        "returned. If a tool errors, report the exact error once; do not retry. No other "
        "tools, files or investigation."
    )


def concurrency_brief(s, skill):
    def req(i):
        return {
            "provider": s["worker"],
            "model": s["model"],
            "effort": s["effort"],
            "timeout_minutes": 3,
            "title": f"{s['marker']}_{i}",
            "skills": [skill],
            "task": (
                f"Write n{i}.txt containing {s['marker']}_{i} in your workspace, then reply "
                f"with {s['marker']}_{i}. Nothing else."
            ),
        }

    return (
        "# Wyss delegation concurrency acceptance\n\n"
        "Harmless installed Wyss Console acceptance task. Use only the Wyss delegation MCP "
        "tools, in this order, without waiting between the first three calls:\n"
        f"1. wyss_delegate {json.dumps(req(1))}\n"
        f"2. wyss_delegate {json.dumps(req(2))}\n"
        f"3. wyss_delegate {json.dumps(req(3))}\n"
        "4. wyss_delegation_cancel on the id returned by call 1.\n"
        "5. wyss_delegation_wait on every other id that started.\n"
        f"Then reply PARENT_RECEIVED_{s['marker']} and, for each call, the exact id or the "
        "exact error code it returned and the final status. Report errors once; never retry."
    )


SCENARIOS = {
    "claude-to-claude": {
        "parent": "claude_code",
        "worker": "claude_code",
        "model": "haiku",
        "effort": "low",
        "keep": "gsap-react",
        "absent": "solid-principles",
        "marker": "WYSS_CLAUDE_TO_CLAUDE_OK",
        "codex_inference": False,
    },
    "claude-to-codex": {
        "parent": "claude_code",
        "worker": "codex",
        "model": "gpt-6-luna",
        "effort": "low",
        "keep": "solid-principles",
        "absent": "gsap-react",
        "marker": "WYSS_CLAUDE_TO_CODEX_OK",
        "codex_inference": True,
    },
    "codex-to-claude": {
        "parent": "codex",
        "worker": "claude_code",
        "model": "haiku",
        "effort": "low",
        "keep": "gsap-react",
        "absent": "solid-principles",
        "marker": "WYSS_CODEX_TO_CLAUDE_OK",
        "codex_inference": True,
    },
    "concurrency-claude": {
        "parent": "claude_code",
        "worker": "claude_code",
        "model": "haiku",
        "effort": "low",
        "keep": "gsap-react",
        "absent": "solid-principles",
        "marker": "WYSS_CONCURRENCY_CLAUDE",
        "concurrency": True,
        "codex_inference": False,
    },
    "concurrency-codex": {
        "parent": "claude_code",
        "worker": "codex",
        "model": "gpt-6-luna",
        "effort": "low",
        "keep": "solid-principles",
        "absent": "gsap-react",
        "marker": "WYSS_CONCURRENCY_CODEX",
        "concurrency": True,
        "codex_inference": True,
    },
}


def start_parent(s, parent_dir, brief, codex_profile):
    call("native_remember_project", {"path": str(parent_dir)})
    request = {
        "agent_id": s["parent"],
        "project": str(parent_dir),
        "profile_id": None,
        "size": {"rows": 50, "cols": 160},
        "brief": brief,
        "delegation": True,
    }
    if s["parent"] == "codex":
        # A Codex start needs a profile (model and effort) and the exact preview it was shown.
        profile = codex_profile or call(
            "profile_create",
            {
                "name": "Delegation acceptance Codex parent",
                "applies_to": ["codex"],
                "model": "gpt-6-luna",
                "effort": "low",
                "permission": "read_only",
                "hooks": "inherit",
                "skills_off": [],
            },
        )["id"]
        preview = call("native_preview", {"agent_id": "codex", "profile_id": profile})
        request.update(
            profile_id=profile,
            plan_token=preview.get("plan_token"),
            # The preview does not echo the CLI version; the qualified Codex build is pinned.
            expected_cli_version="codex-cli 0.159.0",
        )
    return call("native_start", request)


def judge_single(s, recs, screen):
    problems = []
    if len(recs) != 1:
        problems.append(f"expected 1 delegation record, found {len(recs)}")
        return problems
    r = recs[0]
    if r["status"] != "succeeded":
        problems.append(f"status {r['status']} ({r.get('error')})")
    if r["provider"] != s["worker"] or r["model"] != s["model"] or r["effort"] != s["effort"]:
        problems.append("record provider/model/effort differ from the request")
    if r.get("verification", {}).get("state") != "verified":
        problems.append(f"verification {r.get('verification')}")
    if r.get("applied", {}).get("kept") != [s["keep"]]:
        problems.append(f"kept skills {r.get('applied', {}).get('kept')}")
    if s["marker"] not in (r.get("result") or ""):
        problems.append("worker result lacks the marker")
    hello = Path(r["workspace"]) / "hello.txt"
    if not hello.is_file() or s["marker"] not in hello.read_text(errors="replace"):
        problems.append("hello.txt missing or without the marker")
    if f"PARENT_RECEIVED_{s['marker']}" not in screen:
        problems.append("parent did not report the received result")
    return problems


def judge_concurrency(s, recs, screen):
    problems = []
    by_title = {r["title"]: r for r in recs}
    first = by_title.get(f"{s['marker']}_1")
    second = by_title.get(f"{s['marker']}_2")
    if not first or first["status"] != "cancelled":
        problems.append(f"first delegation not cancelled: {first and first['status']}")
    if not second or second["status"] != "succeeded":
        problems.append(f"second delegation not succeeded: {second and second['status']}")
    if f"{s['marker']}_3" in by_title:
        problems.append("third delegation started although the policy allows two at a time")
    if "delegation_limit" not in screen and "already running" not in screen:
        problems.append("parent screen shows no delegation_limit refusal for the third call")
    if f"PARENT_RECEIVED_{s['marker']}" not in screen:
        problems.append("parent did not report back")
    return problems


def run(name, parent_dir, out, codex_profile, minutes):
    s = SCENARIOS[name]
    out.mkdir(parents=True, exist_ok=True)
    skill = capability(s["worker"], s["keep"])
    brief = concurrency_brief(s, skill) if s.get("concurrency") else single_brief(s, skill)
    started = time.time()
    reply = start_parent(s, parent_dir, brief, codex_profile)
    session = reply["session"]["id"]
    terminal = reply["terminal"]["terminal_id"]
    (out / f"{name}-start.json").write_text(json.dumps(reply, indent=2))
    marker = f"PARENT_RECEIVED_{s['marker']}"
    screen = ""
    deadline = started + minutes * 60
    while time.time() < deadline:
        time.sleep(5)
        screen = screen_text(terminal)
        if re.search(r"trust (this|the files in this) folder|Do you trust", screen, re.I):
            screen += "\n[acceptance] stopped: the parent folder is not trusted"
            break
        recs = records(session)
        running = any(r["status"] in ("running", "starting") for r in recs)
        if marker in screen and recs and not running:
            break
    time.sleep(3)
    screen = screen_text(terminal)
    recs = records(session)
    (out / f"{name}-screen.txt").write_text(screen)
    (out / f"{name}-records.json").write_text(json.dumps(recs, indent=2))
    judge = judge_concurrency if s.get("concurrency") else judge_single
    problems = judge(s, recs, screen)
    for step in (("native_stop", {"id": session}), ("term_close", {"terminal_id": terminal})):
        try:
            call(*step)
        except RuntimeError as error:
            problems.append(f"cleanup {step[0]}: {error}")
    verdict = {
        "scenario": name,
        "parent_session": session,
        "seconds": round(time.time() - started),
        "verdict": "PASS" if not problems else "FAIL",
        "problems": problems,
        "records": [
            {k: r.get(k) for k in ("id", "title", "provider", "model", "effort", "status", "error")}
            | {"verification": r.get("verification", {}).get("state")}
            for r in recs
        ],
    }
    (out / f"{name}-verdict.json").write_text(json.dumps(verdict, indent=2))
    print(json.dumps(verdict, indent=2))
    return 0 if not problems else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list")
    r = sub.add_parser("run")
    r.add_argument("scenario", choices=sorted(SCENARIOS))
    r.add_argument("--parent-dir", type=Path, required=True)
    r.add_argument("--out", type=Path, required=True)
    r.add_argument("--codex-profile", help="existing Codex profile id for a Codex parent")
    r.add_argument("--minutes", type=float, default=6)
    args = parser.parse_args()
    if args.cmd == "list":
        for name, s in sorted(SCENARIOS.items()):
            need = "needs Codex inference" if s["codex_inference"] else "Claude only"
            print(f"{name:20} {s['parent']} -> {s['worker']} {s['model']}/{s['effort']}  {need}")
        return 0
    if not args.parent_dir.is_dir():
        raise SystemExit(f"{args.parent_dir} is not a folder")
    return run(args.scenario, args.parent_dir.resolve(), args.out, args.codex_profile, args.minutes)


if __name__ == "__main__":
    sys.exit(main())
