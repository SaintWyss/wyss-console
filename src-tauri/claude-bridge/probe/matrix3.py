import json, sys
import matrix as m
venv, out = sys.argv[1], sys.argv[2]
HARD = {**m.H, "extra_args": {"safe-mode": None, "disable-slash-commands": None, "no-session-persistence": None, "permission-prompts": "none"}}
R = {}
def go(name, options, prompt, **kw):
    res = m.run_case(venv, name, options, prompt, managed=False, **kw)
    s = m.summarize(res); s["home_files"] = res.get("home_files"); s["messages"] = res.get("messages", []); R[name] = s; return s
s = go("deferred-placeholder", {**HARD, "allowed_tools": ["mcp__wyss__run_command"], "permission_mode": "dontAsk"}, "x",
       script=[{"tool_use": {"name": "DeferredToolPlaceholder", "input": {}}}, {"text": "finished"}], sdk_tools={"name": "run_command"})
print("g.", [ (x["type"], str(x.get("content"))[:200]) for x in s["messages"] if x["type"] in ("UserMessage",)], s["markers"])
s = go("home-persist-off", HARD, "hello"); print("h1.", s["home_files"])
nop = {k: v for k, v in HARD.items()}; nop["extra_args"] = {k: v for k, v in HARD["extra_args"].items() if k != "no-session-persistence"}
s = go("home-persist-on", nop, "hello"); print("h2.", s["home_files"])
json.dump(R, open(out, "w"), indent=1, sort_keys=True, default=str)
