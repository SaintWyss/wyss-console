"""Wyss supervised Claude Agent SDK bridge (protocol v1).

One process per Managed runtime, started by the Wyss authority inside its own namespace.
It translates between a closed JSON-lines protocol on stdin/stdout and the official
`claude-agent-sdk`. It makes no security decision of its own: it reports what the SDK and the
CLI say (and what the loopback API tap sees) and the authority decides. Standard output
carries protocol frames only; everything else goes to stderr.

Frames: one JSON object per line, at most 65,536 bytes, `"v": 1`. Unknown commands, malformed
or oversized frames are fatal (`fatal` event, exit status 3). Text is chunked, never truncated
silently; reasoning content is never forwarded.
"""

import asyncio
import json
import os
import secrets
import sys
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:  # the authority hashes every file in this directory before launch
    sys.path.insert(0, HERE)

import profiles  # noqa: E402
from api_tap import ApiTap  # noqa: E402

PROTO = 1
MAX_FRAME = 65536
TEXT_CHUNK = 4000  # characters; at most 16,000 UTF-8 bytes
MAX_ACTION_BYTES = 32 * 1024
MAX_COMMAND_BYTES = 16 * 1024  # contracts::approval::MAX_COMMAND_BYTES: a command is shown whole or not at all
MAX_PATH_BYTES = 1024
MAX_OUTPUT_BYTES = 16 * 1024
COMMANDS = {"configure", "bind", "prompt", "interrupt", "action_result", "shutdown"}


class Fatal(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


def valid_uuid(v):
    try:
        return isinstance(v, str) and str(uuid.UUID(v)) == v.lower() and len(v) == 36
    except ValueError:
        return False


def _int(v):
    return v if isinstance(v, int) and not isinstance(v, bool) and v >= 0 else None


def _str(v, n=200):
    return v[:n] if isinstance(v, str) else None


def names(items, key="name", cap=64):
    out = []
    for it in (items or [])[:cap]:
        if isinstance(it, dict):
            it = it.get(key)
        s = _str(it, 120)
        if s:
            out.append(s)
    return out


def usage_fields(u):
    """Provider-reported counters. Anything absent or malformed stays `None`: unknown is
    never zero. `reasoning` is the provider's own count of thinking tokens (a part of
    `output`), reported only by the turn result."""
    u = u if isinstance(u, dict) else {}
    details = u.get("output_tokens_details")
    return {
        "input": _int(u.get("input_tokens")),
        "output": _int(u.get("output_tokens")),
        "cache_creation": _int(u.get("cache_creation_input_tokens")),
        "cache_read": _int(u.get("cache_read_input_tokens")),
        "reasoning": _int(details.get("thinking_tokens")) if isinstance(details, dict) else None,
    }


def inventory_from_init(data):
    data = data if isinstance(data, dict) else {}
    return {
        "tools": names(data.get("tools")),
        "mcp_servers": [
            {"name": _str(s.get("name"), 80), "status": _str(s.get("status"), 30), "source": _str(s.get("source"), 30)}
            for s in (data.get("mcp_servers") or [])[:32] if isinstance(s, dict)
        ],
        "agents": names(data.get("agents")),
        "skills": names(data.get("skills")),
        "plugins": [
            {"name": _str(p.get("name"), 80), "source": _str(p.get("source"), 80)}
            for p in (data.get("plugins") or [])[:32] if isinstance(p, dict)
        ],
        "slash_commands_n": len(data.get("slash_commands") or []),
        "permission_mode": _str(data.get("permissionMode"), 30),
        "model": _str(data.get("model"), 80),
        "cwd": _str(data.get("cwd"), 400),
        "api_key_source": _str(data.get("apiKeySource"), 40),
        "claude_code_version": _str(data.get("claude_code_version"), 40),
    }


def attestation_from(server_info, mcp, ctx, sdk_version, options_digest):
    si = server_info if isinstance(server_info, dict) else {}
    ctx = ctx if isinstance(ctx, dict) else {}
    acct = si.get("account") if isinstance(si.get("account"), dict) else {}
    models = []
    for m in (si.get("models") or [])[:32]:
        if isinstance(m, dict):
            models.append({
                "value": _str(m.get("value"), 80),
                "resolved": _str(m.get("resolvedModel"), 80),
                "efforts": [e for e in (m.get("supportedEffortLevels") or [])[:16] if isinstance(e, str)] if m.get("supportsEffort") else [],
            })
    return {
        "sdk_version": _str(sdk_version, 40),
        "claude_code_version": _str(si.get("claude_code_version"), 40),
        "python": "%d.%d.%d" % sys.version_info[:3],
        "options_digest": options_digest,
        "token_source": _str(acct.get("tokenSource"), 40),
        "api_key_source": _str(acct.get("apiKeySource"), 40),
        "api_provider": _str(acct.get("apiProvider"), 40),
        "analytics_disabled": si.get("analytics_disabled") if isinstance(si.get("analytics_disabled"), bool) else None,
        "permission_mode": _str(si.get("current_permission_mode"), 30),
        "agents": names(si.get("agents")),
        "commands_n": len(si.get("commands") or []),
        "mcp_servers": [
            {"name": _str(s.get("name"), 80), "status": _str(s.get("status"), 30), "source": _str(s.get("source"), 30)}
            for s in ((mcp or {}).get("mcpServers") or [])[:32] if isinstance(s, dict)
        ],
        "memory_files_n": len(ctx.get("memoryFiles") or []),
        "mcp_tools": names(ctx.get("mcpTools")),
        "mcp_instructions_n": len(ctx.get("mcpInstructions") or []),
        "skills_n": len(ctx.get("skills") or []) if isinstance(ctx.get("skills"), list) else None,
        "slash_commands_n": len(ctx.get("slashCommands") or []) if isinstance(ctx.get("slashCommands"), list) else None,
        "context_categories": names(ctx.get("categories"), cap=32),
        "models": models,
    }


class Translator:
    """SDK message -> protocol events. Pure apart from its own bookkeeping."""

    def __init__(self):
        self.turn = None
        self.streamed = set()  # message ids whose text already went out as deltas
        self.cur = None
        self.usage_seen = set()
        self.started = False

    def begin(self, turn):
        self.turn, self.streamed, self.cur, self.usage_seen, self.started = turn, set(), None, set(), False

    def text_events(self, text):
        return [{"e": "text", "turn": self.turn, "text": text[i:i + TEXT_CHUNK]} for i in range(0, len(text), TEXT_CHUNK)]

    def translate(self, m):
        kind = type(m).__name__
        t = self.turn
        out = []
        if kind == "SystemMessage":
            if m.subtype == "init" and not self.started:
                self.started = True
                d = m.data if isinstance(m.data, dict) else {}
                out.append({"e": "turn_started", "turn": t, "provider_session": _str(d.get("session_id"), 80), "init": inventory_from_init(d)})
            else:
                out.append({"e": "notice", "turn": t, "subtype": _str(getattr(m, "subtype", None), 40) or "unknown"})
        elif kind == "StreamEvent":
            ev = m.event if isinstance(m.event, dict) else {}
            if ev.get("type") == "message_start" and isinstance(ev.get("message"), dict):
                self.cur = _str(ev["message"].get("id"), 80)
            if ev.get("type") == "content_block_delta" and isinstance(ev.get("delta"), dict) and ev["delta"].get("type") == "text_delta":
                text = ev["delta"].get("text")
                if isinstance(text, str):
                    out += self.text_events(text)
                    self.streamed.add(self.cur)
        elif kind == "AssistantMessage":
            mid = _str(m.message_id, 80)
            for b in m.content:
                bk = type(b).__name__
                # A message that carries an API error holds the CLI's own error banner as its text:
                # it is not the model speaking, and it is reported as `provider_error` instead.
                if bk == "TextBlock" and mid not in self.streamed and not m.error:
                    out += self.text_events(b.text)
                elif bk == "ToolUseBlock":
                    out.append({"e": "tool_use", "turn": t, "name": _str(b.name, 120)})
                # ThinkingBlock and anything else: private reasoning is never forwarded.
            if mid is None or mid not in self.usage_seen:
                if mid is not None:
                    self.usage_seen.add(mid)
                out.append({"e": "usage", "turn": t, "message_id": mid, "model": _str(m.model, 80), **{k: v for k, v in usage_fields(m.usage).items() if k != "reasoning"}})
            if m.error:
                # The API's own error class for this message (authentication_failed, rate_limit,
                # server_error, ...). A closed set is enforced by the authority's codec.
                out.append({"e": "provider_error", "turn": t, "kind": _str(str(m.error), 40)})
        elif kind == "ResultMessage":
            mu = {}
            for name, u in list((m.model_usage or {}).items())[:8]:
                if isinstance(u, dict):
                    mu[_str(name, 80) or "?"] = {
                        "input": _int(u.get("inputTokens")), "output": _int(u.get("outputTokens")),
                        "cache_read": _int(u.get("cacheReadInputTokens")), "cache_creation": _int(u.get("cacheCreationInputTokens")),
                        "reasoning": None,
                    }
            cost = m.total_cost_usd
            out.append({
                "e": "turn_result", "turn": t, "subtype": _str(m.subtype, 60), "is_error": bool(m.is_error),
                "stop_reason": _str(m.stop_reason, 40), "terminal_reason": _str(m.terminal_reason, 40),
                "api_error_status": _int(m.api_error_status), "num_turns": _int(m.num_turns),
                "usage": usage_fields(m.usage), "model_usage": mu,
                "cost_usd_estimate": float(cost) if isinstance(cost, (int, float)) and not isinstance(cost, bool) else None,
                "permission_denials": len(m.permission_denials or []),
                "provider_session": _str(m.session_id, 80),
            })
        elif kind == "RateLimitEvent":
            i = m.rate_limit_info
            out.append({
                "e": "rate_limit", "status": _str(i.status, 30), "type": _str(i.rate_limit_type, 30),
                "resets_at": _int(i.resets_at), "utilization": float(i.utilization) if isinstance(i.utilization, (int, float)) and not isinstance(i.utilization, bool) else None,
            })
        elif kind == "UserMessage":
            pass  # tool results echoed by the CLI; never forwarded
        else:
            out.append({"e": "notice", "turn": t, "subtype": "message:" + kind[:30]})
        return out


class Bridge:
    def __init__(self, sdk, reader, writer, env=None):
        self.sdk = sdk
        self.reader = reader
        self.writer = writer
        self.env = env if env is not None else os.environ
        self.client = None
        self.tap = None
        self.profile = None
        self.turn_task = None
        self.tr = Translator()
        self.pending = {}  # action id -> future
        self.action_lock = asyncio.Lock()
        self.next_action = 0
        self.turn = None
        self.wlock = asyncio.Lock()
        self.used = False
        self.current_model = None
        self.session = None
        self.kw = None
        self.digest = None
        self.base = None

    # ---- framing --------------------------------------------------------------------
    async def emit(self, event):
        raw = json.dumps({"v": PROTO, **event}, separators=(",", ":"), ensure_ascii=True).encode() + b"\n"
        if len(raw) > MAX_FRAME:
            raw = json.dumps({"v": PROTO, "e": "error", "code": "frame_too_large"}, separators=(",", ":")).encode() + b"\n"
        async with self.wlock:
            self.writer.write(raw)
            self.writer.flush()

    async def read_command(self):
        try:
            line = await self.reader.readuntil(b"\n")
        except asyncio.IncompleteReadError as e:
            if not e.partial:
                return None
            raise Fatal("truncated_frame")
        except asyncio.LimitOverrunError:
            raise Fatal("frame_too_large")
        if len(line) > MAX_FRAME:
            raise Fatal("frame_too_large")
        try:
            cmd = json.loads(line)
        except ValueError:
            raise Fatal("malformed_frame")
        if not isinstance(cmd, dict) or cmd.get("v") != PROTO or cmd.get("c") not in COMMANDS:
            raise Fatal("unknown_command")
        return cmd

    # ---- lifecycle ------------------------------------------------------------------
    async def configure(self, cmd):
        profile = cmd.get("profile")
        if profile not in profiles.PROFILES or self.client is not None:
            raise Fatal("bad_configure")
        cwd, resume, session = cmd.get("cwd"), cmd.get("resume"), cmd.get("session")
        if (not isinstance(cwd, str) or not os.path.isabs(cwd) or not valid_uuid(session)
                or (resume is not None and not valid_uuid(resume))):
            raise Fatal("bad_configure")
        model, effort = cmd.get("model"), cmd.get("effort")
        if (model is not None and not isinstance(model, str)) or (effort is not None and not isinstance(effort, str)):
            raise Fatal("bad_configure")
        self.profile = profile
        self.turn_timeout = cmd.get("turn_timeout_s") if isinstance(cmd.get("turn_timeout_s"), int) and 1 <= cmd["turn_timeout_s"] <= 3600 else 600
        sdk = self.sdk
        self.base = {"cwd": cwd, "model": model, "effort": effort}
        self.current_model = model
        kw = profiles.option_kwargs(profile, cwd=cwd, session_id=None if resume else session, resume=resume, model=model, effort=effort)
        digest = profiles.frozen_digest(profile)
        self.digest = digest
        # The tap is the independent verifier of what the model is offered. Its credential
        # swap keeps the real key out of the CLI's environment.
        api_key = cmd.get("api_key")
        upstream = cmd.get("upstream") or "https://api.anthropic.com"
        loop = asyncio.get_running_loop()

        def on_tap(ev):
            asyncio.run_coroutine_threadsafe(self.emit(ev), loop)

        client_key = profiles.SYNTHETIC_PREFIX + secrets.token_urlsafe(24)
        self.tap = ApiTap(upstream=upstream, allowed_tools=profiles.allowed_api_tools(profile), api_key=api_key, model=model, on_event=on_tap, client_key=client_key).start()
        env = dict(profiles.CLI_ENV)
        env["ANTHROPIC_BASE_URL"] = self.tap.url
        env["ANTHROPIC_API_KEY"] = client_key
        kw["env"] = env
        if profile == "local_tools":
            kw["mcp_servers"] = {profiles.AUTHORITY_SERVER: self.authority_server()}
        kw["hooks"] = {"PreToolUse": [sdk.HookMatcher(matcher=None, hooks=[self.pre_tool_use])]}
        self.kw = kw
        self.session = resume or session
        await self.connect(kw)

    async def connect(self, kw):
        sdk = self.sdk
        self.client = sdk.ClaudeSDKClient(sdk.ClaudeAgentOptions(**kw))
        await self.client.connect()
        info = await self.client.get_server_info()
        mcp = await self.client.get_mcp_status()
        ctx = await self.client.get_context_usage()
        await self.emit({"e": "ready", "profile": self.profile, "attestation": attestation_from(info, mcp, ctx, getattr(sdk, "__version__", None), self.digest)})

    async def bind(self, resume):
        """Resume an earlier conversation: the CLI's session is fixed at connect, so this
        replaces the client. Only before any turn has run in this process."""
        if self.turn_task is not None or self.used or not valid_uuid(resume) or resume == self.session:
            raise Fatal("bad_bind")
        old = self.client
        self.client = None
        await asyncio.wait_for(old.disconnect(), 5)
        kw = dict(self.kw)
        kw.pop("session_id", None)
        kw["resume"] = resume
        self.session = resume
        await self.connect(kw)
        await self.emit({"e": "bound", "session": resume})

    async def pre_tool_use(self, input_data, tool_use_id, context):
        """Defense in depth only. The tool set offered is already closed (probe matrix, API
        tap); this denies a call to anything outside the authority's tools if a CLI build ever
        offered one."""
        name = (input_data or {}).get("tool_name")
        if name in profiles.AUTHORITY_TOOL_NAMES and self.profile == "local_tools":
            return {}
        await self.emit({"e": "notice", "turn": self.turn, "subtype": "pretooluse_denied"})
        return {"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "not an authority tool"}}

    # ---- authority-owned tools ------------------------------------------------------
    def authority_server(self):
        sdk = self.sdk

        def make(tool_name, description, schema, required):
            @sdk.tool(tool_name, description, schema)
            async def handler(args):
                return await self.propose(tool_name, args, required)
            return handler

        return sdk.create_sdk_mcp_server(profiles.AUTHORITY_SERVER, "1.0.0", [
            make("run_command", "Propose one shell command to run in the user's workspace. It runs only if the user approves this exact command.",
                 {"command": str}, ("command",)),
            make("write_file", "Propose creating or replacing one file in the user's workspace with this exact content. It is written only if the user approves.",
                 {"path": str, "content": str}, ("path", "content")),
        ])

    @staticmethod
    def result(text, error=False):
        return {"content": [{"type": "text", "text": text[:MAX_OUTPUT_BYTES]}], "is_error": error}

    async def propose(self, tool, args, required):
        """The tool body is a *proposal*, never an effect. It blocks until the authority has
        decided and, if approved, dispatched the action itself."""
        if not isinstance(args, dict) or set(args) != set(required) or not all(isinstance(args[k], str) for k in required):
            return self.result("invalid arguments; nothing was run", True)
        if (sum(len(args[k].encode("utf-8")) for k in required) > MAX_ACTION_BYTES
                or len(args.get("command", "").encode("utf-8")) > MAX_COMMAND_BYTES
                or len(args.get("path", "").encode("utf-8")) > MAX_PATH_BYTES):
            return self.result("proposal too large to review completely; nothing was run", True)
        async with self.action_lock:  # one pending proposal at a time
            self.next_action += 1
            action = "act-%d" % self.next_action
            fut = asyncio.get_running_loop().create_future()
            self.pending[action] = fut
            await self.emit({"e": "action_proposal", "turn": self.turn, "action": action, "tool": tool, "args": {k: args[k] for k in required}})
            try:
                outcome = await fut
            finally:
                self.pending.pop(action, None)
        o = outcome.get("outcome")
        if o == "completed":
            return self.result(outcome.get("output") or "(no output)")
        if o in ("denied", "cancelled"):
            return self.result("the user did not approve this action; nothing was run", True)
        return self.result("the outcome of this action is unknown; do not retry it", True)

    # ---- turns ----------------------------------------------------------------------
    async def run_turn(self, turn, text, model=None):
        self.turn = turn
        self.tr.begin(turn)
        try:
            # A model asked for one turn applies to that turn only: the next turn goes back to the
            # model the runtime was configured with (or the provider's default).
            want = model or self.base["model"]
            if want != self.current_model:
                self.tap.model = want
                await self.client.set_model(want)
                self.current_model = want
            await self.client.query(text)
            async for m in self.client.receive_response():
                for ev in self.tr.translate(m):
                    await self.emit(ev)
        except asyncio.CancelledError:
            raise
        except Exception as e:  # noqa: BLE001 - report the class, never the message (may carry secrets)
            await self.emit({"e": "error", "turn": turn, "code": "sdk_error:" + type(e).__name__[:40]})
        finally:
            self.turn_task = None
            await self.emit({"e": "turn_idle", "turn": turn})

    async def on_turn_timeout(self, turn, task):
        try:
            await asyncio.wait_for(asyncio.shield(task), self.turn_timeout)
        except asyncio.TimeoutError:
            await self.emit({"e": "error", "turn": turn, "code": "turn_timeout"})
            try:
                await self.client.interrupt()
            except Exception:  # noqa: BLE001
                pass
        except Exception:  # noqa: BLE001
            pass

    async def handle(self, cmd):
        c = cmd["c"]
        if c == "configure":
            await self.configure(cmd)
        elif self.client is None:
            raise Fatal("not_configured")
        elif c == "bind":
            await self.bind(cmd.get("resume"))
        elif c == "prompt":
            turn, text, model = cmd.get("turn"), cmd.get("text"), cmd.get("model")
            if (not isinstance(turn, str) or not isinstance(text, str) or not (0 < len(turn) <= 64)
                    or (model is not None and not (isinstance(model, str) and 0 < len(model) <= 128))):
                raise Fatal("bad_prompt")
            if self.turn_task is not None:
                await self.emit({"e": "error", "turn": turn, "code": "turn_active"})
                return
            self.used = True
            self.turn_task = asyncio.create_task(self.run_turn(turn, text, model))
            asyncio.create_task(self.on_turn_timeout(turn, self.turn_task))
        elif c == "interrupt":
            if self.turn_task is not None:
                await self.client.interrupt()
        elif c == "action_result":
            fut = self.pending.get(cmd.get("action"))
            if fut is None or fut.done():
                await self.emit({"e": "error", "code": "unknown_action"})
                return
            out = cmd.get("output")
            fut.set_result({"outcome": cmd.get("outcome"), "output": out if isinstance(out, str) else ""})
        elif c == "shutdown":
            raise SystemExit(0)

    async def run(self):
        await self.emit({"e": "hello", "proto": PROTO, "python": "%d.%d.%d" % sys.version_info[:3], "sdk_version": getattr(self.sdk, "__version__", None)})
        try:
            while True:
                cmd = await self.read_command()
                if cmd is None:
                    return 0  # EOF: the authority closed the pipe
                await self.handle(cmd)
        except Fatal as f:
            await self.emit({"e": "fatal", "code": f.code})
            return 3
        except SystemExit as s:
            return s.code or 0
        except Exception as e:  # noqa: BLE001
            await self.emit({"e": "fatal", "code": "bridge_error:" + type(e).__name__[:40]})
            return 2
        finally:
            await self.close()

    async def close(self):
        for fut in self.pending.values():
            if not fut.done():
                fut.set_result({"outcome": "cancelled"})
        if self.turn_task is not None:
            self.turn_task.cancel()
        if self.client is not None:
            try:
                await asyncio.wait_for(self.client.disconnect(), 5)
            except Exception:  # noqa: BLE001
                pass
        if self.tap is not None:
            self.tap.stop()


async def amain():
    import claude_agent_sdk as sdk

    out = sys.stdout.buffer
    sys.stdout = sys.stderr  # nothing but protocol frames may reach the real stdout
    loop = asyncio.get_running_loop()
    reader = asyncio.StreamReader(limit=MAX_FRAME + 1)
    await loop.connect_read_pipe(lambda: asyncio.StreamReaderProtocol(reader), sys.stdin)
    return await Bridge(sdk, reader, out).run()


if __name__ == "__main__":
    sys.exit(asyncio.run(amain()))
