"""Second experiment batch: forced tool_use, network destinations, model/effort, usage,
authority-owned SDK tool. Same rig as matrix.py. Usage: python3 matrix2.py <venv> <out.json>"""
import json, sys
import matrix as m

venv, out = sys.argv[1], sys.argv[2]
R = {}
H = dict(m.H)
HARD = {**H, "extra_args": {"safe-mode": None, "disable-slash-commands": None, "no-session-persistence": None, "permission-prompts": "none"}}

def go(name, options, prompt, **kw):
    res = m.run_case(venv, name, options, prompt, managed=False, **kw)
    s = m.summarize(res)
    s["messages"] = res.get("messages", [])
    s["proxy"] = res.get("proxy")
    s["stderr"] = res.get("stderr", [])[-3:]
    s["sdk_error"] = res.get("error")
    R[name] = s
    return s

# a. the model demands a tool that was not offered
s = go("forced-bash-no-tools", HARD, "run it", script=[{"tool_use": {"name": "Bash", "input": {"command": "/usr/bin/touch /markers/bash_executed"}}}, {"text": "done"}])
print("a.", s["markers"], s["api_tools"], s["sdk_error"])
# b. destinations contacted
s = go("network", HARD, "hello", proxy=True)
print("b.", s["proxy"])
# c. model / effort requested
s = go("model-effort", {**HARD, "model": "claude-sonnet-5-5", "effort": "low"}, "hello")
print("c.", s["api_model"], s["api_effort"], s["api_thinking"])
# d. reroute: provider answers with another model
s = go("reroute", {**HARD, "model": "claude-sonnet-5-5"}, "hello", script=[{"text": "hi", "model": "claude-haiku-5-5"}])
print("d.", [ (x["type"], x.get("model")) for x in s["messages"]])
# e. authority-owned tool offered, builtin absent, model calls it
s = go("sdk-tool", {**HARD, "allowed_tools": ["mcp__wyss__run_command"], "permission_mode": "dontAsk"}, "use the tool",
       script=[{"tool_use": {"name": "mcp__wyss__run_command", "input": {"command": "echo hi"}}}, {"text": "finished"}],
       sdk_tools={"name": "run_command"})
print("e.", s["api_tools"], s["markers"], s["sdk_error"])
# f. same but permission mode default and no allowed_tools: does the SDK tool run without a prompt?
s = go("sdk-tool-no-allow", {**HARD}, "use the tool",
       script=[{"tool_use": {"name": "mcp__wyss__run_command", "input": {"command": "echo hi"}}}, {"text": "finished"}],
       sdk_tools={"name": "run_command"})
print("f.", s["api_tools"], s["markers"], s["sdk_error"])
json.dump(R, open(out, "w"), indent=1, sort_keys=True, default=str)
