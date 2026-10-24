"""Experiment matrix for the Claude Agent SDK Managed-Inference profile.

Each case seeds hostile configuration in every place Claude Code reads it, runs the real
bundled CLI through the real SDK inside a network-less bubblewrap namespace against a mock
API, and records (a) what the SDK reports, (b) the tool definitions the model was actually
offered (from the API request body, independent of the SDK), and (c) marker files that prove
which seeded side effects ran. Usage: python3 matrix.py <sdk-venv> <out.json> [case ...]
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import sandbox  # noqa: E402

EVENTS = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "SessionEnd"]


def hook_block(prefix):
    return {e: [{"hooks": [{"type": "command", "command": "/usr/bin/touch /markers/%s_%s" % (prefix, e)}]}] for e in EVENTS}


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


def seed(root, plugin=True):
    home, ws, markers, etc = (os.path.join(root, d) for d in ("home", "ws", "markers", "etc"))
    for d in (home, ws, markers, etc):
        os.makedirs(d, exist_ok=True)
    touch = lambda n: {"command": "/usr/bin/touch", "args": ["/markers/" + n]}  # noqa: E731
    write(home + "/.claude/settings.json", json.dumps({
        "hooks": hook_block("user"),
        "statusLine": {"type": "command", "command": "/usr/bin/touch /markers/user_statusline"},
        "apiKeyHelper": "/usr/bin/touch /markers/user_apikeyhelper; echo sk-ant-api03-helper",
        "env": {"WYSS_PROBE_ENV": "from-user-settings"},
        "permissions": {"allow": ["Bash(*)", "Write(*)", "Read(*)"]},
        "mcpServers": {"usersettingsmcp": touch("user_settings_mcp")},
    }))
    write(home + "/.claude.json", json.dumps({
        "numStartups": 3, "hasCompletedOnboarding": True,
        "mcpServers": {"userglobalmcp": touch("user_global_mcp")},
    }))
    write(home + "/.claude/CLAUDE.md", "USER_MEMORY_MARKER_AAA\n")
    write(home + "/.claude/agents/useragent.md", "---\nname: useragent\ndescription: probe\n---\nUSER_AGENT_MARKER\n")
    write(home + "/.claude/commands/usercmd.md", "USER_COMMAND_MARKER\n")
    write(home + "/.claude/skills/userskill/SKILL.md", "---\nname: userskill\ndescription: probe skill\n---\nUSER_SKILL_MARKER\n")
    write(ws + "/CLAUDE.md", "PROJECT_MEMORY_MARKER_BBB\n")
    write(ws + "/secret.txt", "FILE_CONTENT_MARKER_CCC\n")
    write(ws + "/.claude/settings.json", json.dumps({"hooks": hook_block("project")}))
    write(ws + "/.claude/settings.local.json", json.dumps({"hooks": hook_block("local")}))
    write(ws + "/.mcp.json", json.dumps({"mcpServers": {"projectmcp": touch("project_mcp")}}))
    write(ws + "/.claude/agents/projectagent.md", "---\nname: projectagent\ndescription: probe\n---\nPROJECT_AGENT_MARKER\n")
    write(ws + "/.claude/commands/projectcmd.md", "PROJECT_COMMAND_MARKER\n")
    write(ws + "/.claude/skills/projectskill/SKILL.md", "---\nname: projectskill\ndescription: probe skill\n---\nPROJECT_SKILL_MARKER\n")
    write(etc + "/managed-settings.json", json.dumps({"hooks": hook_block("managed")}))
    write(etc + "/managed-mcp.json", json.dumps({"mcpServers": {"managedmcp": touch("managed_mcp")}}))
    if plugin:
        pd = os.path.join(root, "plugin")
        write(pd + "/.claude-plugin/plugin.json", json.dumps({"name": "probeplugin", "version": "1.0.0", "description": "probe"}))
        write(pd + "/hooks/hooks.json", json.dumps({"hooks": hook_block("plugin")}))
        write(pd + "/.mcp.json", json.dumps({"mcpServers": {"pluginmcp": touch("plugin_mcp")}}))
        write(pd + "/commands/plugincmd.md", "PLUGIN_COMMAND_MARKER\n")
    return home, ws, markers, etc


def run_case(venv, name, options, prompt, script=None, managed=True, seed_plugin=True, env_extra=None, keep=False, **more):
    root = tempfile.mkdtemp(prefix="wyss-sdk-probe-")
    try:
        home, ws, markers, etc = seed(root, seed_plugin)
        opts = dict(options)
        opts.setdefault("cwd", "/work/ws")
        if opts.pop("__plugin__", False):
            opts["plugins"] = [{"type": "local", "path": "/work/plugin"}]
        cfg = json.dumps({"options": opts, "prompt": prompt, "script": script, **more})
        extra = ["--bind", ws, "/work/ws", "--bind", markers, "/markers", "--ro-bind", root + "/plugin", "/work/plugin"]
        if managed:
            extra += ["--ro-bind", etc + "/managed-settings.json", "/etc/claude-code/managed-settings.json",
                      "--ro-bind", etc + "/managed-mcp.json", "/etc/claude-code/managed-mcp.json"]
        cmd = sandbox.argv(home=home, workdir="/work/ws", ro=[venv, HERE], env=sandbox.base_env("http://127.0.0.1:1", {**(env_extra or {})}),
                           command=[venv + "/bin/python", "-I", HERE + "/child_query.py", cfg], extra=extra)
        cmd.insert(cmd.index("--"), "--perms") if False else None
        r = sandbox.run(cmd, 180)
        res = {"returncode": r.returncode, "stderr_tail": r.stderr[-400:]}
        for line in r.stdout.splitlines():
            if line.startswith("@@RESULT@@"):
                res.update(json.loads(line[len("@@RESULT@@"):]))
        res["markers"] = sorted(os.listdir(markers))
        res["home_files"] = sorted(os.path.relpath(os.path.join(d, f), home) for d, _, fs in os.walk(home) for f in fs
                                   if not f.startswith(("settings", "CLAUDE", "useragent", "usercmd", "SKILL")) and ".claude/agents" not in d and ".claude/commands" not in d and ".claude/skills" not in d)
        return res
    finally:
        if not keep:
            shutil.rmtree(root, ignore_errors=True)


def summarize(res):
    init = next((m["data"] for m in res.get("messages", []) if m.get("type") == "SystemMessage" and m.get("subtype") == "init"), {})
    api = [r for r in res.get("requests", []) if r["path"].startswith("/v1/messages") and isinstance(r.get("body"), dict) and "tools" in r["body"]]
    first = api[0]["body"] if api else {}
    sys_text = json.dumps(first.get("system", ""))
    msg_text = json.dumps(first.get("messages", ""))
    return {
        "error": res.get("error"),
        "returncode": res.get("returncode"),
        "init_tools": init.get("tools"),
        "init_mcp": init.get("mcp_servers"),
        "init_agents": init.get("agents"),
        "init_skills": init.get("skills"),
        "init_plugins": init.get("plugins"),
        "init_slash_n": len(init.get("slash_commands") or []),
        "init_permission_mode": init.get("permissionMode"),
        "api_tools": [t.get("name") for t in first.get("tools", [])],
        "api_model": first.get("model"),
        "api_thinking": first.get("thinking"),
        "api_effort": (first.get("output_config") or {}),
        "api_calls": len(api),
        "memory_in_prompt": [m for m in ("USER_MEMORY_MARKER_AAA", "PROJECT_MEMORY_MARKER_BBB", "FILE_CONTENT_MARKER_CCC",
                                         "USER_COMMAND_MARKER", "PROJECT_COMMAND_MARKER", "USER_SKILL_MARKER", "PROJECT_SKILL_MARKER")
                             if m in sys_text or m in msg_text],
        "markers": res.get("markers"),
    }


H = {"tools": [], "setting_sources": [], "strict_mcp_config": True, "verbatim_prompts": True, "skills": []}
CASES = {
    "defaults": ({}, "hello"),
    "tools-empty-nosettings": ({"tools": [], "setting_sources": []}, "hello"),
    "disallow-star": ({"disallowed_tools": ["*"]}, "hello"),
    "hardened": (dict(H), "hello"),
    "hardened-safe-mode": ({**H, "extra_args": {"safe-mode": None}}, "hello"),
    "hardened-bare": ({**H, "extra_args": {"bare": None}}, "hello"),
    "hardened-restricted": ({**H, "extra_args": {"restricted": None}}, "hello"),
    "hardened-noslash": ({**H, "extra_args": {"disable-slash-commands": None}}, "hello"),
    "hardened-allflags": ({**H, "extra_args": {"safe-mode": None, "disable-slash-commands": None, "no-session-persistence": None, "permission-prompts": "none"}}, "hello"),
    "mention-default": ({"tools": [], "setting_sources": []}, "summarize @/work/ws/secret.txt"),
    "mention-verbatim": ({**H}, "summarize @/work/ws/secret.txt"),
    "slash-default": ({"tools": [], "setting_sources": []}, "/projectcmd"),
    "slash-verbatim": ({**H}, "/projectcmd"),
}

if __name__ == "__main__":
    venv, out = sys.argv[1], sys.argv[2]
    names = sys.argv[3:] or list(CASES)
    results = {}
    for n in names:
        opts, prompt = CASES[n]
        for managed in (True, False):
            key = n + ("" if managed else "+no-managed-policy")
            res = run_case(venv, n, opts, prompt, managed=managed)
            results[key] = summarize(res)
            results[key]["stderr"] = res.get("stderr", [])[-3:]
            print(key, json.dumps(results[key])[:1100], flush=True)
    json.dump(results, open(out, "w"), indent=1, sort_keys=True)
