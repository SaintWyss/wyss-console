"""A fake `claude_agent_sdk` for protocol-level tests. Class names match the real SDK's
message classes because the bridge dispatches on them."""
import asyncio
from types import SimpleNamespace


class SystemMessage:
    def __init__(self, subtype, data=None):
        self.subtype, self.data = subtype, data or {}


class StreamEvent:
    def __init__(self, event, uuid="u"):
        self.event, self.uuid = event, uuid


class TextBlock:
    def __init__(self, text):
        self.text = text


class ThinkingBlock:
    def __init__(self, thinking):
        self.thinking = thinking


class ToolUseBlock:
    def __init__(self, name):
        self.name = name


class AssistantMessage:
    def __init__(self, content, model="m", usage=None, message_id=None, error=None):
        self.content, self.model, self.usage, self.message_id, self.error = content, model, usage, message_id, error


class ResultMessage:
    def __init__(self, **kw):
        base = dict(subtype="success", is_error=False, stop_reason="end_turn", terminal_reason="completed", api_error_status=None,
                    num_turns=1, usage={"input_tokens": 1, "output_tokens": 2}, model_usage={}, total_cost_usd=0.01,
                    permission_denials=[], session_id="s1")
        base.update(kw)
        self.__dict__.update(base)


class RateLimitEvent:
    def __init__(self, status="allowed_warning", type_="five_hour", resets_at=1, utilization=0.8):
        self.rate_limit_info = SimpleNamespace(status=status, rate_limit_type=type_, resets_at=resets_at, utilization=utilization)


class UserMessage:
    pass


class FakeClient:
    instances = []

    def __init__(self, options):
        self.options = options
        self.script = []
        self.interrupted = False
        self.queries = []
        FakeClient.instances.append(self)

    async def connect(self):
        pass

    async def get_server_info(self):
        return {"claude_code_version": "2.1.309", "account": {"tokenSource": "none", "apiKeySource": "ANTHROPIC_API_KEY", "apiProvider": "firstParty"},
                "agents": [{"name": "claude"}], "commands": [], "models": [{"value": "m", "resolvedModel": "m1", "supportsEffort": True, "supportedEffortLevels": ["low"]}],
                "current_permission_mode": "dontAsk", "analytics_disabled": True}

    async def get_mcp_status(self):
        return {"mcpServers": []}

    async def get_context_usage(self):
        return {"memoryFiles": [], "mcpTools": [], "mcpInstructions": [], "categories": [{"name": "Messages"}]}

    async def query(self, text):
        self.queries.append(text)

    async def receive_response(self):
        for m in self.script:
            if callable(m):
                m = await m(self)
            yield m
            await asyncio.sleep(0)

    async def interrupt(self):
        self.interrupted = True

    async def set_model(self, model):
        self.models = getattr(self, "models", []) + [model]

    async def disconnect(self):
        pass


def make_sdk():
    FakeClient.instances = []

    def tool(name, desc, schema):
        def deco(fn):
            fn.tool_name = name
            return fn
        return deco

    return SimpleNamespace(
        __version__="0.2.173",
        ClaudeSDKClient=FakeClient,
        ClaudeAgentOptions=lambda **kw: SimpleNamespace(**kw),
        HookMatcher=lambda matcher=None, hooks=None: SimpleNamespace(matcher=matcher, hooks=hooks),
        tool=tool,
        create_sdk_mcp_server=lambda name, version, tools: SimpleNamespace(name=name, tools={t.tool_name: t for t in tools}),
    )
