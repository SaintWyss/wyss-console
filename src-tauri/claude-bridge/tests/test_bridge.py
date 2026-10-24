import asyncio, io, json, os, sys, unittest
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)
import bridge as B
import fakes as F


class Sink(io.BytesIO):
    def flush(self):
        pass


def frames(*cmds):
    out = b""
    for c in cmds:
        out += (c if isinstance(c, bytes) else json.dumps({"v": 1, **c}).encode()) + b"\n"
    return out


async def drive(data, sdk=None, hook=None):
    reader = asyncio.StreamReader(limit=B.MAX_FRAME + 1)
    reader.feed_data(data)
    reader.feed_eof()
    sink = Sink()
    sdk = sdk or F.make_sdk()
    b = B.Bridge(sdk, reader, sink)
    if hook:
        hook(b)
    rc = await b.run()
    events = [json.loads(l) for l in sink.getvalue().splitlines()]
    return rc, events, b


def run(coro):
    return asyncio.run(coro)


SESSION = "5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40"
CONF = {"c": "configure", "profile": "inference", "cwd": "/work", "session": SESSION, "broker_token": "abababababababababababababababababababababababababababababababab"}


class Framing(unittest.TestCase):
    def fatal(self, data):
        rc, ev, _ = run(drive(data))
        self.assertEqual(rc, 3, ev)
        self.assertEqual(ev[-1]["e"], "fatal")
        return ev[-1]["code"]

    def test_unknown_command(self):
        self.assertEqual(self.fatal(frames({"c": "exec", "cmd": "id"})), "unknown_command")

    def test_wrong_version(self):
        self.assertEqual(self.fatal(b'{"v":2,"c":"shutdown"}\n'), "unknown_command")

    def test_malformed(self):
        self.assertEqual(self.fatal(b"{not json\n"), "malformed_frame")
        self.assertEqual(self.fatal(b"[1,2]\n"), "unknown_command")

    def test_oversized(self):
        self.assertEqual(self.fatal(b'{"v":1,"c":"prompt","text":"' + b"a" * (B.MAX_FRAME + 10) + b'"}\n'), "frame_too_large")

    def test_truncated_without_newline(self):
        self.assertEqual(self.fatal(b'{"v":1,"c":"shutdown"}'), "truncated_frame")

    def test_prompt_before_configure(self):
        self.assertEqual(self.fatal(frames({"c": "prompt", "turn": "t", "text": "x"})), "not_configured")

    def test_double_and_bad_configure(self):
        self.assertEqual(self.fatal(frames(CONF, CONF)), "bad_configure")
        self.assertEqual(self.fatal(frames({**CONF, "profile": "bypass"})), "bad_configure")
        self.assertEqual(self.fatal(frames({**CONF, "cwd": "relative"})), "bad_configure")
        self.assertEqual(self.fatal(frames({**CONF, "model": 5})), "bad_configure")
        self.assertEqual(self.fatal(frames({**CONF, "session": "not-a-uuid"})), "bad_configure")
        self.assertEqual(self.fatal(frames({**CONF, "session": None})), "bad_configure")
        self.assertEqual(self.fatal(frames({**CONF, "resume": "nope"})), "bad_configure")

    def test_eof_is_clean_exit(self):
        rc, ev, _ = run(drive(b""))
        self.assertEqual(rc, 0)
        self.assertEqual([e["e"] for e in ev], ["hello"])


class Ready(unittest.TestCase):
    def test_configure_reports_attestation_and_pins_options(self):
        sdk = F.make_sdk()
        rc, ev, b = run(drive(frames(CONF), sdk))
        ready = next(e for e in ev if e["e"] == "ready")
        att = ready["attestation"]
        self.assertEqual((att["sdk_version"], att["claude_code_version"]), ("0.2.173", "2.1.309"))
        self.assertEqual(att["mcp_servers"], [])
        opts = F.FakeClient.instances[0].options
        self.assertEqual(opts.tools, [])
        self.assertEqual(opts.setting_sources, [])
        self.assertTrue(opts.env["ANTHROPIC_API_KEY"].startswith(B.profiles.SYNTHETIC_PREFIX))
        self.assertGreater(len(opts.env["ANTHROPIC_API_KEY"]), len(B.profiles.SYNTHETIC_PREFIX) + 20)
        self.assertTrue(opts.env["ANTHROPIC_BASE_URL"].startswith("http://127.0.0.1:"))
        self.assertNotIn("sk-", json.dumps(att))

    def test_the_bridge_has_no_credential_to_leak_and_refuses_one(self):
        # The credential goes to the broker, never here: a frame that carries one is not a frame.
        sdk = F.make_sdk()
        rc, ev, _ = run(drive(frames({**CONF, "api_key": "sk-REAL-SECRET"}), sdk))
        self.assertEqual((rc, ev[-1]["e"]), (3, "fatal"))
        self.assertNotIn("sk-REAL-SECRET", json.dumps(ev))
        self.assertFalse(F.FakeClient.instances and "sk-REAL-SECRET" in json.dumps(F.FakeClient.instances[-1].options.env))

    def test_a_missing_or_malformed_broker_token_is_fatal(self):
        for bad in (None, "", "ab" * 31, "zz" * 32, 7):
            conf = {k: v for k, v in CONF.items() if k != "broker_token"}
            if bad is not None:
                conf["broker_token"] = bad
            rc, ev, _ = run(drive(frames(conf)))
            self.assertEqual((rc, ev[-1]["e"], ev[-1]["code"]), (3, "fatal", "bad_broker_token"), bad)

    def test_the_tap_points_at_the_broker_socket_and_nothing_else(self):
        sdk = F.make_sdk()
        _, _, b = run(drive(frames(CONF), sdk))
        self.assertEqual((b.tap.broker_socket, b.tap.broker_token), (B.profiles.BROKER_SOCKET, "ab" * 32))

    def test_local_tools_registers_only_authority_tools(self):
        sdk = F.make_sdk()
        run(drive(frames({**CONF, "profile": "local_tools"}), sdk))
        opts = F.FakeClient.instances[0].options
        self.assertEqual(list(opts.mcp_servers), ["wyss"])
        self.assertEqual(sorted(opts.mcp_servers["wyss"].tools), ["run_command", "write_file"])


class Binding(unittest.TestCase):
    def test_a_new_conversation_is_named_up_front(self):
        sdk = F.make_sdk()
        run(drive(frames(CONF), sdk))
        opts = F.FakeClient.instances[0].options
        self.assertEqual(opts.session_id, SESSION)
        self.assertFalse(hasattr(opts, "resume"))

    def test_resume_replaces_the_client_before_any_turn_and_reattests(self):
        other = "7a1d2c3b-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
        sdk = F.make_sdk()
        rc, ev, _ = run(drive(frames(CONF, {"c": "bind", "resume": other}), sdk))
        self.assertEqual(rc, 0)
        self.assertEqual([e["e"] for e in ev if e["e"] in ("ready", "bound")], ["ready", "ready", "bound"])
        first, second = (c.options for c in F.FakeClient.instances)
        self.assertEqual(first.session_id, SESSION)
        self.assertEqual(second.resume, other)
        self.assertFalse(hasattr(second, "session_id"))
        self.assertEqual(next(e for e in ev if e["e"] == "bound")["session"], other)

    def test_resume_is_refused_after_a_turn_for_the_same_id_or_a_bad_id(self):
        for bad in ("nope", SESSION):
            rc, ev, _ = run(drive(frames(CONF, {"c": "bind", "resume": bad}), F.make_sdk()))
            self.assertEqual((rc, ev[-1]["code"]), (3, "bad_bind"), bad)

    def test_resume_after_a_turn_is_refused(self):
        other = "7a1d2c3b-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
        async def go():
            reader = asyncio.StreamReader(limit=B.MAX_FRAME + 1)
            sink = Sink()
            b = B.Bridge(F.make_sdk(), reader, sink)
            task = asyncio.create_task(b.run())
            reader.feed_data(frames(CONF))
            await asyncio.sleep(0.05)
            F.FakeClient.instances[0].script = [F.SystemMessage("init", {}), F.ResultMessage()]
            reader.feed_data(frames({"c": "prompt", "turn": "t1", "text": "hi"}))
            await asyncio.sleep(0.2)
            reader.feed_data(frames({"c": "bind", "resume": other}))
            reader.feed_eof()
            rc = await task
            return rc, [json.loads(l) for l in sink.getvalue().splitlines()]
        rc, ev = asyncio.run(go())
        self.assertEqual((rc, ev[-1]["code"]), (3, "bad_bind"))


class Turns(unittest.TestCase):
    def turn(self, script, extra=(), profile="inference"):
        sdk = F.make_sdk()
        cmds = [{**CONF, "profile": profile}, {"c": "prompt", "turn": "t1", "text": "hi"}, *extra]

        async def go():
            reader = asyncio.StreamReader(limit=B.MAX_FRAME + 1)
            sink = Sink()
            b = B.Bridge(sdk, reader, sink)
            task = asyncio.create_task(b.run())
            reader.feed_data(frames(cmds[0]))
            await asyncio.sleep(0.05)
            F.FakeClient.instances[0].script = script
            reader.feed_data(frames(*cmds[1:]))
            for _ in range(100):
                await asyncio.sleep(0.02)
                if any(json.loads(l)["e"] == "turn_idle" for l in sink.getvalue().splitlines()):
                    break
            reader.feed_eof()
            await task
            return [json.loads(l) for l in sink.getvalue().splitlines()]

        return asyncio.run(go())

    def test_streams_text_once_and_forwards_usage_once(self):
        init = F.SystemMessage("init", {"session_id": "s1", "tools": [], "mcp_servers": []})
        script = [init,
                  F.StreamEvent({"type": "message_start", "message": {"id": "m1"}}),
                  F.StreamEvent({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "hel"}}),
                  F.StreamEvent({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "lo"}}),
                  F.AssistantMessage([F.TextBlock("hello")], usage={"input_tokens": 3, "output_tokens": 2}, message_id="m1"),
                  F.AssistantMessage([F.TextBlock("hello")], usage={"input_tokens": 3, "output_tokens": 2}, message_id="m1"),
                  F.ResultMessage()]
        ev = self.turn(script)
        self.assertEqual("".join(e["text"] for e in ev if e["e"] == "text"), "hello")
        self.assertEqual(len([e for e in ev if e["e"] == "usage"]), 1)
        self.assertEqual(next(e for e in ev if e["e"] == "turn_started")["provider_session"], "s1")

    def test_unstreamed_message_text_is_not_lost_when_another_message_streamed(self):
        script = [F.SystemMessage("init", {}),
                  F.StreamEvent({"type": "message_start", "message": {"id": "m1"}}),
                  F.StreamEvent({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "A"}}),
                  F.AssistantMessage([F.TextBlock("A")], message_id="m1"),
                  F.AssistantMessage([F.TextBlock("B")], message_id="m2"),
                  F.ResultMessage()]
        ev = self.turn(script)
        self.assertEqual("".join(e["text"] for e in ev if e["e"] == "text"), "AB")

    def test_reasoning_is_never_forwarded(self):
        script = [F.SystemMessage("init", {}), F.AssistantMessage([F.ThinkingBlock("SECRET THOUGHT"), F.TextBlock("ok")], message_id="m1"), F.ResultMessage()]
        ev = self.turn(script)
        self.assertNotIn("SECRET THOUGHT", json.dumps(ev))

    def test_missing_usage_is_unknown_not_zero(self):
        script = [F.SystemMessage("init", {}), F.AssistantMessage([F.TextBlock("x")], usage=None, message_id="m1"),
                  F.ResultMessage(usage=None, total_cost_usd=None)]
        ev = self.turn(script)
        u = next(e for e in ev if e["e"] == "usage")
        self.assertEqual((u["input"], u["output"], u["cache_read"], u["cache_creation"]), (None, None, None, None))
        r = next(e for e in ev if e["e"] == "turn_result")
        self.assertIsNone(r["usage"]["input"])
        self.assertIsNone(r["cost_usd_estimate"])

    def test_negative_and_boolean_counters_are_unknown(self):
        self.assertEqual(B.usage_fields({"input_tokens": -1, "output_tokens": True, "cache_read_input_tokens": "5"}),
                         {"input": None, "output": None, "cache_creation": None, "cache_read": None, "reasoning": None})

    def test_reasoning_tokens_come_from_the_result_only_when_reported(self):
        self.assertEqual(B.usage_fields({"output_tokens": 9, "output_tokens_details": {"thinking_tokens": 4}})["reasoning"], 4)
        self.assertIsNone(B.usage_fields({"output_tokens": 9})["reasoning"])
        self.assertIsNone(B.usage_fields({"output_tokens_details": {"thinking_tokens": "4"}})["reasoning"])

    def test_long_text_is_chunked_not_truncated(self):
        big = "é" * 9000
        script = [F.SystemMessage("init", {}), F.AssistantMessage([F.TextBlock(big)], message_id="m1"), F.ResultMessage()]
        ev = self.turn(script)
        texts = [e["text"] for e in ev if e["e"] == "text"]
        self.assertEqual("".join(texts), big)
        self.assertTrue(all(len(json.dumps(e).encode()) < B.MAX_FRAME for e in ev))

    def test_rate_limit_and_unknown_messages_are_forwarded_as_such(self):
        class Weird: pass
        script = [F.SystemMessage("init", {}), F.RateLimitEvent(), Weird(), F.ResultMessage()]
        ev = self.turn(script)
        self.assertEqual(next(e for e in ev if e["e"] == "rate_limit")["resets_at"], 1)
        self.assertTrue(any(e["e"] == "notice" and e["subtype"] == "message:Weird" for e in ev))

    def test_a_message_level_provider_error_is_its_own_event(self):
        script = [F.SystemMessage("init", {}), F.AssistantMessage([F.TextBlock("API Error: 529 overloaded")], error="server_error", message_id="m1"), F.ResultMessage(is_error=True)]
        ev = self.turn(script)
        self.assertFalse(any(e["e"] == "text" for e in ev), "the CLI's error banner is not model text")
        self.assertIn({"v": 1, "e": "provider_error", "turn": "t1", "kind": "server_error"}, ev)
        self.assertFalse(any(e["e"] == "notice" and "error" in e["subtype"] for e in ev))

    def test_second_prompt_while_active_is_refused(self):
        async def slow(client):
            await asyncio.sleep(0.3)
            return F.ResultMessage()
        ev = self.turn([F.SystemMessage("init", {}), slow], extra=[{"c": "prompt", "turn": "t2", "text": "again"}])
        self.assertTrue(any(e["e"] == "error" and e["code"] == "turn_active" for e in ev))

    def test_sdk_exception_reports_class_only(self):
        async def boom(client):
            raise RuntimeError("leaks sk-ant-SECRET")
        ev = self.turn([boom])
        err = next(e for e in ev if e["e"] == "error")
        self.assertEqual(err["code"], "sdk_error:RuntimeError")
        self.assertNotIn("SECRET", json.dumps(ev))


class PerTurnModel(unittest.TestCase):
    def drive_turns(self, turns, configure=None):
        async def go():
            reader = asyncio.StreamReader(limit=B.MAX_FRAME + 1)
            sink = Sink()
            b = B.Bridge(F.make_sdk(), reader, sink)
            task = asyncio.create_task(b.run())
            reader.feed_data(frames({**CONF, **(configure or {})}))
            await asyncio.sleep(0.05)
            client = F.FakeClient.instances[0]
            for i, model in enumerate(turns):
                client.script = [F.SystemMessage("init", {}), F.ResultMessage()]
                cmd = {"c": "prompt", "turn": "t%d" % i, "text": "hi"}
                if model:
                    cmd["model"] = model
                reader.feed_data(frames(cmd))
                for _ in range(100):
                    await asyncio.sleep(0.01)
                    if sum(1 for l in sink.getvalue().splitlines() if json.loads(l)["e"] == "turn_idle") > i:
                        break
            reader.feed_eof()
            await task
            return client, b
        return asyncio.run(go())

    def test_a_model_for_one_turn_applies_to_that_turn_only(self):
        client, b = self.drive_turns(["claude-haiku-5-5", None, None])
        self.assertEqual(client.models, ["claude-haiku-5-5", None])
        self.assertIsNone(b.tap.model if b.tap else None)

    def test_the_configured_model_is_the_baseline_to_return_to(self):
        client, _ = self.drive_turns([None, "claude-haiku-5-5", None], {"model": "claude-sonnet-5-5"})
        self.assertEqual(client.models, ["claude-haiku-5-5", "claude-sonnet-5-5"])

    def test_the_same_model_twice_is_not_set_twice(self):
        client, _ = self.drive_turns(["claude-haiku-5-5", "claude-haiku-5-5"])
        self.assertEqual(client.models, ["claude-haiku-5-5"])

    def test_a_bad_model_field_is_a_fatal_protocol_error(self):
        rc, ev, _ = run(drive(frames(CONF, {"c": "prompt", "turn": "t", "text": "x", "model": 5})))
        self.assertEqual((rc, ev[-1]["code"]), (3, "bad_prompt"))
        rc, ev, _ = run(drive(frames(CONF, {"c": "prompt", "turn": "t", "text": "x", "model": ""})))
        self.assertEqual((rc, ev[-1]["code"]), (3, "bad_prompt"))


class Proposals(unittest.TestCase):
    def make(self):
        sdk = F.make_sdk()
        reader = asyncio.StreamReader(limit=B.MAX_FRAME + 1)
        sink = Sink()
        b = B.Bridge(sdk, reader, sink)
        b.client = object()  # configured: `handle` refuses anything else before `configure`
        return b, reader, sink

    def test_proposal_blocks_until_result_and_returns_authority_output(self):
        async def go():
            b, reader, sink = self.make()
            b.turn = "t1"
            t = asyncio.create_task(b.propose("run_command", {"command": "ls"}, ("command",)))
            await asyncio.sleep(0.05)
            self.assertFalse(t.done())
            ev = [json.loads(l) for l in sink.getvalue().splitlines()]
            self.assertEqual(ev[0]["e"], "action_proposal")
            self.assertEqual(ev[0]["args"], {"command": "ls"})
            await b.handle({"c": "action_result", "action": ev[0]["action"], "outcome": "completed", "output": "files"})
            r = await t
            self.assertEqual(r["content"][0]["text"], "files")
            self.assertFalse(r["is_error"])
        run(go())

    def test_denied_cancelled_and_indeterminate_are_errors_without_replay_hint(self):
        async def one(outcome):
            b, reader, sink = self.make()
            b.turn = "t1"
            t = asyncio.create_task(b.propose("run_command", {"command": "ls"}, ("command",)))
            await asyncio.sleep(0.02)
            a = json.loads(sink.getvalue().splitlines()[0])["action"]
            await b.handle({"c": "action_result", "action": a, "outcome": outcome})
            return await t
        for o in ("denied", "cancelled", "indeterminate", "failed", "garbage"):
            r = run(one(o))
            self.assertTrue(r["is_error"], o)
            self.assertNotIn("(no output)", r["content"][0]["text"])
        self.assertIn("do not retry", run(one("indeterminate"))["content"][0]["text"])

    def test_oversize_or_malformed_proposals_are_refused_without_an_event(self):
        async def go():
            b, reader, sink = self.make()
            big = await b.propose("write_file", {"path": "a", "content": "x" * (B.MAX_ACTION_BYTES + 1)}, ("path", "content"))
            bad = await b.propose("run_command", {"command": 5}, ("command",))
            extra = await b.propose("run_command", {"command": "ls", "cwd": "/"}, ("command",))
            self.assertTrue(big["is_error"] and bad["is_error"] and extra["is_error"])
            self.assertEqual(sink.getvalue(), b"")
        run(go())

    def test_unknown_or_duplicate_answer_is_an_error_not_a_dispatch(self):
        async def go():
            b, reader, sink = self.make()
            b.turn = "t1"
            await b.handle({"c": "action_result", "action": "act-9", "outcome": "completed"})
            t = asyncio.create_task(b.propose("run_command", {"command": "ls"}, ("command",)))
            await asyncio.sleep(0.02)
            a = json.loads(sink.getvalue().splitlines()[1])["action"]
            await b.handle({"c": "action_result", "action": a, "outcome": "completed", "output": "once"})
            await t
            await b.handle({"c": "action_result", "action": a, "outcome": "completed", "output": "twice"})
            codes = [json.loads(l).get("code") for l in sink.getvalue().splitlines()]
            self.assertEqual(codes.count("unknown_action"), 2)
        run(go())

    def test_proposals_are_serialized(self):
        async def go():
            b, reader, sink = self.make()
            b.turn = "t1"
            t1 = asyncio.create_task(b.propose("run_command", {"command": "a"}, ("command",)))
            t2 = asyncio.create_task(b.propose("run_command", {"command": "b"}, ("command",)))
            await asyncio.sleep(0.05)
            self.assertEqual(len(sink.getvalue().splitlines()), 1)
            await b.handle({"c": "action_result", "action": "act-1", "outcome": "denied"})
            await asyncio.sleep(0.05)
            self.assertEqual(len(sink.getvalue().splitlines()), 2)
            await b.handle({"c": "action_result", "action": "act-2", "outcome": "denied"})
            await t1; await t2
        run(go())

    def test_close_cancels_pending(self):
        async def go():
            b, reader, sink = self.make()
            b.turn = "t1"
            t = asyncio.create_task(b.propose("run_command", {"command": "a"}, ("command",)))
            await asyncio.sleep(0.02)
            await b.close()
            r = await t
            self.assertTrue(r["is_error"])
        run(go())

    def test_pretooluse_denies_everything_but_authority_tools(self):
        async def go():
            b, reader, sink = self.make()
            b.profile = "local_tools"
            self.assertEqual(await b.pre_tool_use({"tool_name": "mcp__wyss__run_command"}, None, None), {})
            d = await b.pre_tool_use({"tool_name": "Bash"}, None, None)
            self.assertEqual(d["hookSpecificOutput"]["permissionDecision"], "deny")
            b.profile = "inference"
            d = await b.pre_tool_use({"tool_name": "mcp__wyss__run_command"}, None, None)
            self.assertEqual(d["hookSpecificOutput"]["permissionDecision"], "deny")
        run(go())


if __name__ == "__main__":
    unittest.main()
