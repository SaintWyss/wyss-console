"""Runs inside the sandbox: one SDK `query()` with the options named by argv[1] (JSON), against
the mock API, and prints one JSON document with what the SDK reported and what the mock saw."""

import asyncio
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mock_anthropic import ConnectLogger, MockAnthropic  # noqa: E402
from claude_agent_sdk import ClaudeAgentOptions, query  # noqa: E402

cfg = json.loads(sys.argv[1])


def plain(value):
    if hasattr(value, "__dict__"):
        return {k: plain(v) for k, v in vars(value).items()}
    if isinstance(value, dict):
        return {k: plain(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [plain(v) for v in value]
    return value if isinstance(value, (str, int, float, bool, type(None))) else str(value)


async def main():
    log = "/tmp/requests.jsonl"
    mock = MockAnthropic(log, cfg.get("script")).start()
    os.environ["ANTHROPIC_BASE_URL"] = mock.url
    errs = []
    proxy = None
    if cfg.get("proxy"):
        proxy = ConnectLogger("/tmp/proxy.log").start()
        os.environ.update(HTTPS_PROXY=proxy.url, HTTP_PROXY=proxy.url, NO_PROXY="127.0.0.1,localhost")
    kwargs = dict(cfg["options"])
    sdk_tools = cfg.get("sdk_tools")
    if sdk_tools:
        from claude_agent_sdk import create_sdk_mcp_server, tool

        @tool(sdk_tools["name"], "probe tool", {"command": str})
        async def probe_tool(args):
            with open("/markers/sdk_tool_called", "a") as f:
                f.write(json.dumps(args) + "\n")
            return {"content": [{"type": "text", "text": "tool-result-from-authority"}]}

        kwargs["mcp_servers"] = {"wyss": create_sdk_mcp_server("wyss", "1.0.0", [probe_tool])}
    opts = ClaudeAgentOptions(**kwargs, stderr=errs.append)
    out = {"messages": [], "error": None, "stderr": errs}
    try:
        async for m in query(prompt=cfg["prompt"], options=opts):
            out["messages"].append({"type": type(m).__name__, **plain(m)})
    except Exception as e:  # noqa: BLE001 - the experiment records every failure
        out["error"] = "%s: %s" % (type(e).__name__, str(e)[:300])
    reqs = []
    if os.path.exists(log):
        for line in open(log):
            reqs.append(json.loads(line))
    out["requests"] = reqs
    mock.stop()
    out["proxy"] = open("/tmp/proxy.log").read().splitlines() if proxy and os.path.exists("/tmp/proxy.log") else []
    print("@@RESULT@@" + json.dumps(out))


asyncio.run(main())
