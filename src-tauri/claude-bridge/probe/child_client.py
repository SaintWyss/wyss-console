import asyncio, json, os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mock_anthropic import MockAnthropic
from claude_agent_sdk import ClaudeAgentOptions, ClaudeSDKClient

def plain(v):
    if hasattr(v, "__dict__"): return {k: plain(x) for k, x in vars(v).items()}
    if isinstance(v, dict): return {k: plain(x) for k, x in v.items()}
    if isinstance(v, (list, tuple)): return [plain(x) for x in v]
    return v if isinstance(v, (str, int, float, bool, type(None))) else str(v)

async def main():
    cfg = json.loads(sys.argv[1])
    mock = MockAnthropic("/tmp/r.jsonl", cfg.get("script")).start()
    os.environ["ANTHROPIC_BASE_URL"] = mock.url
    out = {}
    t0 = time.monotonic()
    c = ClaudeSDKClient(ClaudeAgentOptions(**cfg["options"]))
    await c.connect()
    out["connect_s"] = round(time.monotonic() - t0, 3)
    for name in ("get_server_info", "get_mcp_status", "get_context_usage"):
        try:
            v = await getattr(c, name)()
            out[name] = plain(v)
        except Exception as e:
            out[name] = "ERR %s: %s" % (type(e).__name__, str(e)[:200])
    msgs = []
    t1 = time.monotonic()
    await c.query("hello")
    first = None
    async for m in c.receive_response():
        if first is None: first = round(time.monotonic() - t1, 3)
        msgs.append({"t": round(time.monotonic() - t1, 3), "type": type(m).__name__, **plain(m)})
        if cfg.get("interrupt_after_text") and type(m).__name__ == "AssistantMessage":
            ti = time.monotonic(); await c.interrupt(); out["interrupt_call_s"] = round(time.monotonic() - ti, 3)
    out["first_msg_s"] = first
    out["messages"] = msgs
    await c.disconnect()
    mock.stop()
    print("@@RESULT@@" + json.dumps(out, default=str))
asyncio.run(main())
