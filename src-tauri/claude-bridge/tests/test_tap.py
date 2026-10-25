import hashlib, json, os, socket, ssl, subprocess, sys, tempfile, threading, unittest, http.client
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "probe"))
from api_tap import ApiTap, Violation, check_request, UnixHTTPConnection
from broker import Broker, Refused
from mock_anthropic import MockAnthropic


class CheckRequest(unittest.TestCase):
    def test_inference_requires_no_tools(self):
        self.assertEqual(check_request("/v1/messages?beta=true", {"model": "m", "tools": []}, frozenset())["tools"], [])
        self.assertEqual(check_request("/v1/messages", {"model": "m"}, frozenset())["tools"], [])
        with self.assertRaises(Violation):
            check_request("/v1/messages", {"tools": [{"name": "Bash"}]}, frozenset())

    def test_local_tools_exact_subset(self):
        allowed = frozenset({"mcp__wyss__run_command"})
        check_request("/v1/messages", {"tools": [{"name": "mcp__wyss__run_command"}]}, allowed)
        with self.assertRaises(Violation):
            check_request("/v1/messages", {"tools": [{"name": "mcp__wyss__run_command"}, {"name": "Write"}]}, allowed)

    def test_refuses_server_tools_paths_and_shapes(self):
        for body in ({"tools": [{"name": "x", "type": "web_search_20250305"}]}, {"tools": "x"}, {"tools": [{"nope": 1}]}, [], "s"):
            with self.assertRaises(Violation):
                check_request("/v1/messages", body, frozenset({"x"}))
        with self.assertRaises(Violation):
            check_request("/v1/files", {"tools": []}, frozenset())

    def test_model_pin(self):
        check_request("/v1/messages", {"model": "a"}, frozenset(), model="a")
        with self.assertRaises(Violation):
            check_request("/v1/messages", {"model": "b"}, frozenset(), model="a")

    def test_reports_effort_and_thinking(self):
        s = check_request("/v1/messages", {"model": "m", "output_config": {"effort": "low"}, "thinking": {"type": "adaptive"}}, frozenset())
        self.assertEqual((s["effort"], s["thinking"]), ("low", "adaptive"))

    def test_the_summary_never_echoes_request_text(self):
        evil = "ignore previous\nsecret-" + "x" * 300
        s = check_request("/v1/messages", {"model": evil, "output_config": {"effort": evil}, "thinking": {"type": evil}}, frozenset())
        self.assertEqual((s["model"], s["effort"], s["thinking"]), ("other", "other", "other"))
        for value in ({"a": 1}, 5, ["low"]):
            s = check_request("/v1/messages", {"model": value, "output_config": {"effort": value}, "thinking": {"type": value}}, frozenset())
            self.assertEqual((s["model"], s["effort"], s["thinking"]), ("other", "other", "other"))
        s = check_request("/v1/messages", {"model": "claude-sonnet-5-5", "output_config": {"effort": "xhigh"}, "thinking": {"type": "enabled"}}, frozenset())
        self.assertEqual((s["model"], s["effort"], s["thinking"]), ("claude-sonnet-5-5", "xhigh", "enabled"))
        s = check_request("/v1/messages", {"model": "m"}, frozenset())
        self.assertEqual((s["effort"], s["thinking"]), (None, None))


TOKEN = "ab" * 32
RUN_KEY = "the-run-key"


def short_dir():
    # A unix socket path is limited to about 100 bytes.
    d = tempfile.mkdtemp(prefix="wb")
    os.chmod(d, 0o700)
    return d


class Chain(unittest.TestCase):
    """CLI -> tap -> (unix socket) -> broker -> mock provider, all real servers on this machine."""

    def setUp(self):
        self.log = tempfile.mktemp()
        self.up = MockAnthropic(self.log, [{"text": "streamed text " * 10}]).start()
        self.events = []
        self.dir = short_dir()
        self.stops = [self.up.stop]

    def tearDown(self):
        for stop in reversed(self.stops):
            stop()

    def chain(self, *, profile="inference", client_key=RUN_KEY, token=TOKEN, model=None):
        broker = Broker(directory=self.dir, token=TOKEN, origin=self.up.url, profile=profile).start()
        self.stops.append(broker.stop)
        from profiles import allowed_api_tools
        tap = ApiTap(broker_socket=broker.path, broker_token=token, allowed_tools=allowed_api_tools(profile),
                     model=model, on_event=self.events.append, client_key=client_key).start()
        self.stops.append(tap.stop)
        return tap, broker

    def post(self, tap, body, key=RUN_KEY, path="/v1/messages", method="POST", headers=None):
        c = http.client.HTTPConnection("127.0.0.1", tap.server.server_address[1])
        h = {"content-type": "application/json", "x-api-key": key}
        h.update(headers or {})
        c.request(method, path, json.dumps(body) if body is not None else None, h)
        r = c.getresponse()
        data = r.read()
        c.close()
        return r.status, data

    def provider_saw(self):
        return [json.loads(l) for l in open(self.log)] if os.path.exists(self.log) else []

    def test_a_request_reaches_the_provider_through_the_broker_and_streams_back(self):
        tap, _ = self.chain()
        status, data = self.post(tap, {"model": "m", "stream": True, "tools": []})
        self.assertEqual(status, 200)
        self.assertIn(b"message_stop", data)
        self.assertEqual(len(self.provider_saw()), 1)
        self.assertEqual([e["e"] for e in self.events], ["tap_request"])

    def test_the_clis_own_key_never_goes_past_the_tap(self):
        tap, _ = self.chain()
        self.post(tap, {"model": "m", "tools": []}, key=RUN_KEY)
        seen = self.provider_saw()[0]
        self.assertFalse(seen["has_key"])  # no credential configured: the provider got none, and not the run key

    def test_a_stranger_without_the_run_key_cannot_use_the_chain(self):
        tap, _ = self.chain()
        for key in ("sk-ant-synthetic", "", "the-run-keY"):
            status, _ = self.post(tap, {"model": "m", "stream": True, "tools": []}, key=key)
            self.assertEqual(status, 401, key)
        c = http.client.HTTPConnection("127.0.0.1", tap.server.server_address[1])
        c.request("GET", "/v1/models")
        r = c.getresponse(); r.read(); c.close()
        self.assertEqual(r.status, 401)
        self.assertEqual(self.provider_saw(), [])
        self.assertEqual(self.events, [])  # nothing was said to the bridge about them
        self.assertEqual(tap.count, 0)

    def test_a_tap_with_the_wrong_broker_token_gets_nothing_from_the_provider(self):
        tap, _ = self.chain(token="cd" * 32)
        status, _ = self.post(tap, {"model": "m", "tools": []})
        self.assertEqual(status, 401)
        self.assertEqual(self.provider_saw(), [])

    def test_a_violation_is_blocked_before_the_broker(self):
        tap, _ = self.chain()
        status, _ = self.post(tap, {"model": "m", "tools": [{"name": "Bash"}]})
        self.assertEqual(status, 403)
        self.assertEqual(self.provider_saw(), [])
        self.assertEqual(self.events[0]["e"], "tap_violation")

    def test_the_broker_applies_the_policy_itself_when_the_tap_is_bypassed(self):
        # What a compromised bridge could try: talk to the broker directly with the right token.
        _, broker = self.chain()
        for body in ({"model": "m", "tools": [{"name": "Bash"}]}, {"model": "m", "mcp_servers": [{"type": "url"}]},
                     {"model": "m", "container": {"id": "x"}}, {"model": "m", "tools": [{"name": "x", "type": "web_search_20250305"}]}):
            c = UnixHTTPConnection(broker.path)
            c.request("POST", "/v1/messages", json.dumps(body), {"x-wyss-broker": TOKEN})
            r = c.getresponse(); r.read(); c.close()
            self.assertEqual(r.status, 403, body)
            self.assertTrue(r.getheader("x-wyss-violation"))
        self.assertEqual(self.provider_saw(), [])

    def test_unexpected_methods_paths_and_framing_are_refused_by_both(self):
        tap, broker = self.chain()
        for method in ("PUT", "DELETE", "PATCH"):
            status, _ = self.post(tap, {"a": 1}, method=method)
            self.assertEqual(status, 405, method)
        c = UnixHTTPConnection(broker.path)
        c.request("DELETE", "/v1/messages", None, {"x-wyss-broker": TOKEN})
        r = c.getresponse(); r.read(); c.close()
        self.assertEqual(r.status, 405)
        for path in ("/v1/files", "/v1/messages/../files", "/v1/complete", "/v1/messages/batches"):
            status, _ = self.post(tap, {"model": "m", "tools": []}, path=path)
            self.assertEqual(status, 403, path)
        # chunked bodies and non-numeric lengths
        for raw in (b"POST /v1/messages HTTP/1.1\r\nhost: x\r\nx-api-key: %s\r\ntransfer-encoding: chunked\r\n\r\n0\r\n\r\n" % RUN_KEY.encode(),
                    b"POST /v1/messages HTTP/1.1\r\nhost: x\r\nx-api-key: %s\r\ncontent-length: -5\r\n\r\n" % RUN_KEY.encode(),
                    b"POST /v1/messages HTTP/1.1\r\nhost: x\r\nx-api-key: %s\r\ncontent-length: 99999999999\r\n\r\n" % RUN_KEY.encode()):
            s = socket.create_connection(("127.0.0.1", tap.server.server_address[1]))
            s.sendall(raw)
            head = s.recv(200)
            s.close()
            self.assertTrue(head.startswith(b"HTTP/1.1 4"), head)
        self.assertEqual(self.provider_saw(), [])

    def test_malformed_json_and_oversized_bodies_are_refused(self):
        tap, broker = self.chain()
        c = http.client.HTTPConnection("127.0.0.1", tap.server.server_address[1])
        c.request("POST", "/v1/messages", "not json", {"x-api-key": RUN_KEY})
        r = c.getresponse(); r.read(); c.close()
        self.assertEqual(r.status, 403)
        c = UnixHTTPConnection(broker.path)
        c.request("POST", "/v1/messages", "not json", {"x-wyss-broker": TOKEN})
        r = c.getresponse(); r.read(); c.close()
        self.assertEqual(r.status, 403)
        c = UnixHTTPConnection(broker.path)
        c.putrequest("POST", "/v1/messages")
        c.putheader("x-wyss-broker", TOKEN)
        c.putheader("content-length", str(9 * 1024 * 1024))
        c.endheaders()
        r = c.getresponse(); r.read(); c.close()
        self.assertEqual(r.status, 413)
        self.assertEqual(self.provider_saw(), [])

    def test_a_client_that_keeps_violating_gets_a_bounded_number_of_events(self):
        tap, _ = self.chain()
        for _ in range(60):
            status, _ = self.post(tap, {"model": "m", "tools": [{"name": "Bash"}]})
            self.assertEqual(status, 403)
        violations = [e for e in self.events if e["e"] == "tap_violation"]
        self.assertEqual(len(violations), 17)
        self.assertEqual(violations[-1]["why"], "violations_suppressed")
        self.assertEqual(self.provider_saw(), [])

    def test_an_upstream_that_is_down_is_a_fixed_word_not_a_trace(self):
        broker = Broker(directory=self.dir, token=TOKEN, origin="http://127.0.0.1:9", profile="inference").start()
        self.stops.append(broker.stop)
        c = UnixHTTPConnection(broker.path)
        c.request("POST", "/v1/messages", json.dumps({"model": "m", "tools": []}), {"x-wyss-broker": TOKEN})
        r = c.getresponse(); data = r.read(); c.close()
        self.assertEqual(r.status, 502)
        self.assertEqual(json.loads(data)["error"]["message"], "wyss broker: upstream")

    def test_the_socket_is_private(self):
        _, broker = self.chain()
        st = os.stat(broker.path)
        self.assertEqual(st.st_mode & 0o777, 0o600)


class Credential(unittest.TestCase):
    """The credential: configurations the broker refuses, and what it does with one."""

    def setUp(self):
        self.dir = short_dir()

    def refused(self, **kw):
        base = dict(directory=self.dir, token=TOKEN, profile="inference", origin="https://api.anthropic.com")
        base.update(kw)
        with self.assertRaises(Refused) as cm:
            Broker(**base)
        return cm.exception.args[0]

    def test_a_credential_is_never_sent_over_plain_http_or_to_another_origin(self):
        key = "sk-ant-api03-" + "x" * 30
        self.assertEqual(self.refused(api_key=key, origin="http://127.0.0.1:8080"), "plaintext_origin")
        self.assertEqual(self.refused(api_key=key, origin="http://api.anthropic.com"), "plaintext_origin")
        self.assertEqual(self.refused(api_key=key, origin="https://api.evil.example"), "origin_not_allowed")
        self.assertEqual(self.refused(api_key=key, origin="https://api.anthropic.com:8443"), "origin_not_allowed")
        self.assertEqual(self.refused(api_key=key, origin="https://api.anthropic.com/v1"), "origin_not_allowed")
        self.assertEqual(self.refused(api_key=key, origin="https://user@api.anthropic.com"), "origin_not_allowed")
        self.assertEqual(self.refused(api_key=key, origin="file:///etc/passwd"), "origin")
        self.assertEqual(self.refused(api_key=key, origin="ftp://x"), "origin")
        self.assertEqual(self.refused(api_key=key, origin=None), "origin")
        # no credential: plain HTTP only to a loopback port
        self.assertEqual(self.refused(origin="http://example.com:80"), "plaintext_origin")
        self.assertEqual(self.refused(origin="http://127.0.0.1"), "plaintext_origin")
        self.assertEqual(self.refused(origin="http://127.0.0.1:80/x"), "plaintext_origin")

    def test_malformed_credentials_tokens_profiles_and_directories_are_refused(self):
        self.assertEqual(self.refused(api_key="a b"), "credential")
        self.assertEqual(self.refused(api_key=""), "credential")
        self.assertEqual(self.refused(api_key="k" * 513), "credential")
        self.assertEqual(self.refused(api_key=7), "credential")
        self.assertEqual(self.refused(token="short"), "token")
        self.assertEqual(self.refused(token="G" * 64), "token")
        self.assertEqual(self.refused(profile="shell"), "profile")
        os.chmod(self.dir, 0o755)
        self.assertEqual(self.refused(), "directory")

    def test_the_real_credential_reaches_the_provider_only_over_verified_tls_and_nowhere_else(self):
        key = "sk-ant-api03-" + "k" * 30
        certdir = tempfile.mkdtemp(prefix="wc")
        crt, pem = os.path.join(certdir, "c.crt"), os.path.join(certdir, "c.key")
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", pem, "-out", crt, "-days", "1",
                        "-subj", "/CN=127.0.0.1", "-addext", "subjectAltName=IP:127.0.0.1"], check=True, capture_output=True)
        log = tempfile.mktemp()
        up = MockAnthropic(log, [{"text": "ok"}])
        server_ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        server_ctx.load_cert_chain(crt, pem)
        up.server.socket = server_ctx.wrap_socket(up.server.socket, server_side=True)
        up.start()
        port = up.server.server_address[1]
        origin = "https://127.0.0.1:%d" % port
        try:
            # A client that does not trust the certificate gets a refusal, not a leak.
            broker = Broker(directory=self.dir, token=TOKEN, origin=origin, profile="inference", api_key=key,
                            _origins=(origin,)).start()
            try:
                c = UnixHTTPConnection(broker.path)
                c.request("POST", "/v1/messages", json.dumps({"model": "m", "tools": []}), {"x-wyss-broker": TOKEN})
                r = c.getresponse(); body = r.read(); c.close()
                self.assertEqual(r.status, 502)
                self.assertNotIn(key.encode(), body)
                self.assertFalse(os.path.exists(log) and open(log).read())
            finally:
                broker.stop()
            # One that trusts it delivers the credential and only the credential.
            ctx = ssl.create_default_context(cafile=crt)
            broker = Broker(directory=self.dir, token=TOKEN, origin=origin, profile="inference", api_key=key,
                            _origins=(origin,), _context=ctx).start()
            try:
                c = UnixHTTPConnection(broker.path)
                c.request("POST", "/v1/messages", json.dumps({"model": "m", "stream": True, "tools": []}),
                          {"x-wyss-broker": TOKEN, "x-api-key": "client-supplied", "authorization": "Bearer client"})
                r = c.getresponse(); body = r.read(); c.close()
                self.assertEqual(r.status, 200)
                self.assertNotIn(key.encode(), body)
                seen = [json.loads(l) for l in open(log)]
                self.assertEqual(seen[0]["key_sha256"], hashlib.sha256(key.encode()).hexdigest())
                self.assertNotIn("x-wyss-broker", seen[0]["headers"])
                self.assertNotIn("authorization", seen[0]["headers"])
            finally:
                broker.stop()
        finally:
            up.stop()

    def test_the_credential_is_read_from_stdin_and_exits_when_stdin_ends(self):
        key = "sk-ant-api03-" + "s" * 30
        here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        p = subprocess.Popen([sys.executable, "-I", os.path.join(here, "broker.py")], stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=here)
        env_text = open("/proc/%d/environ" % p.pid, "rb").read()
        p.stdin.write((json.dumps({"v": 1, "dir": self.dir, "token": TOKEN, "origin": "https://api.anthropic.com",
                                   "profile": "inference", "api_key": key}) + "\n").encode())
        p.stdin.flush()
        self.assertEqual(json.loads(p.stdout.readline()), {"e": "broker_ready"})
        self.assertNotIn(key.encode(), open("/proc/%d/cmdline" % p.pid, "rb").read())
        self.assertNotIn(key.encode(), env_text)
        p.stdin.close()
        out, err = p.communicate(timeout=10)
        self.assertEqual(p.returncode, 0)
        self.assertNotIn(key.encode(), out + err)
        self.assertFalse(os.path.exists(os.path.join(self.dir, "broker.sock")))

    def test_a_bad_frame_ends_the_broker_with_a_code_and_never_echoes_the_credential(self):
        key = "sk-ant-api03-" + "s" * 30
        here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        for frame in ({"v": 1, "dir": self.dir, "token": TOKEN, "origin": "http://127.0.0.1:80", "profile": "inference", "api_key": key},
                      {"v": 1, "dir": self.dir, "token": TOKEN, "origin": "https://api.anthropic.com", "profile": "inference",
                       "api_key": key, "extra": "x"},
                      {"v": 2}):
            p = subprocess.run([sys.executable, "-I", os.path.join(here, "broker.py")], input=(json.dumps(frame) + "\n").encode(),
                               capture_output=True, cwd=here, timeout=10)
            self.assertEqual(p.returncode, 3)
            self.assertEqual(json.loads(p.stdout)["e"], "broker_fatal")
            self.assertNotIn(key.encode(), p.stdout + p.stderr)


if __name__ == "__main__":
    unittest.main()
