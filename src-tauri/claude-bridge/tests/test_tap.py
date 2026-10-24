import json, os, sys, tempfile, threading, unittest, http.client
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "probe"))
from api_tap import ApiTap, Violation, check_request
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


class Forwarding(unittest.TestCase):
    def setUp(self):
        self.log = tempfile.mktemp()
        self.up = MockAnthropic(self.log, [{"text": "streamed text " * 10}]).start()
        self.events = []

    def tearDown(self):
        self.up.stop()

    def post(self, tap, body, key="sk-ant-synthetic"):
        c = http.client.HTTPConnection("127.0.0.1", tap.server.server_address[1])
        c.request("POST", "/v1/messages", json.dumps(body), {"content-type": "application/json", "x-api-key": key})
        r = c.getresponse()
        data = r.read()
        c.close()
        return r.status, data

    def test_forwards_streaming_and_swaps_credential(self):
        tap = ApiTap(upstream=self.up.url, allowed_tools=frozenset(), api_key="REAL-KEY", on_event=self.events.append).start()
        try:
            status, data = self.post(tap, {"model": "m", "stream": True, "tools": []})
        finally:
            tap.stop()
        self.assertEqual(status, 200)
        self.assertIn(b"message_stop", data)
        seen = [json.loads(l) for l in open(self.log)]
        self.assertEqual(len(seen), 1)
        import hashlib
        self.assertEqual(seen[0]["key_sha256"], hashlib.sha256(b"REAL-KEY").hexdigest())  # the upstream saw the real key,
        self.assertNotEqual(seen[0]["key_sha256"], hashlib.sha256(b"sk-ant-synthetic").hexdigest())  # not the CLI's
        self.assertEqual([e["e"] for e in self.events], ["tap_request"])

    def test_a_stranger_without_the_run_key_cannot_spend_the_credential(self):
        tap = ApiTap(upstream=self.up.url, allowed_tools=frozenset(), api_key="REAL-KEY", client_key="the-run-key", on_event=self.events.append).start()
        try:
            for key in ("sk-ant-synthetic", "", "the-run-keY"):
                status, _ = self.post(tap, {"model": "m", "stream": True, "tools": []}, key=key)
                self.assertEqual(status, 401, key)
            c = http.client.HTTPConnection("127.0.0.1", tap.server.server_address[1])
            c.request("GET", "/v1/models")
            r = c.getresponse(); r.read(); c.close()
            self.assertEqual(r.status, 401)
            status, _ = self.post(tap, {"model": "m", "stream": True, "tools": []}, key="the-run-key")
        finally:
            tap.stop()
        self.assertEqual(status, 200)
        # Nothing reached the provider for the strangers, and the bridge was told nothing about them.
        self.assertEqual(len([json.loads(l) for l in open(self.log)]), 1)
        self.assertEqual([e["e"] for e in self.events], ["tap_request"])
        self.assertEqual(tap.count, 1)

    def test_a_body_that_asks_the_provider_to_run_something_is_refused(self):
        tap = ApiTap(upstream=self.up.url, allowed_tools=frozenset(), on_event=self.events.append).start()
        try:
            for extra in ({"mcp_servers": [{"type": "url", "url": "https://x.invalid", "name": "x"}]}, {"container": {"id": "c"}}):
                status, _ = self.post(tap, {"model": "m", "tools": [], **extra})
                self.assertEqual(status, 403, extra)
        finally:
            tap.stop()
        self.assertFalse(os.path.exists(self.log) and open(self.log).read())

    def test_violation_is_blocked_before_upstream(self):
        tap = ApiTap(upstream=self.up.url, allowed_tools=frozenset(), on_event=self.events.append).start()
        try:
            status, _ = self.post(tap, {"model": "m", "tools": [{"name": "Bash"}]})
        finally:
            tap.stop()
        self.assertEqual(status, 403)
        self.assertFalse(os.path.exists(self.log) and open(self.log).read())
        self.assertEqual(self.events[0]["e"], "tap_violation")

    def test_unknown_path_and_bad_json_refused(self):
        tap = ApiTap(upstream=self.up.url, allowed_tools=frozenset(), on_event=self.events.append).start()
        try:
            c = http.client.HTTPConnection("127.0.0.1", tap.server.server_address[1])
            c.request("POST", "/v1/files", "{}")
            r = c.getresponse(); r.read(); c.close()
            self.assertEqual(r.status, 403)
            c = http.client.HTTPConnection("127.0.0.1", tap.server.server_address[1])
            c.request("POST", "/v1/messages", "not json")
            r = c.getresponse(); r.read(); c.close()
            self.assertEqual(r.status, 403)
        finally:
            tap.stop()


if __name__ == "__main__":
    unittest.main()
