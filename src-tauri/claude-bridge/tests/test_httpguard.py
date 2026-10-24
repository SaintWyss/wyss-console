"""The review's S2, S3 and S7: what a client controls never reaches a response, framing is closed,
and what the endpoints spend is bounded. Every test talks to the real servers with raw bytes."""
import http.client, json, os, socket, sys, tempfile, threading, time, unittest
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "probe"))
from api_tap import ApiTap, UnixHTTPConnection, check_request, Violation
from broker import Broker
from httpguard import Limits, closed_code
from mock_anthropic import MockAnthropic
from profiles import allowed_api_tools

TOKEN = "ab" * 32
KEY = "the-run-key"


def short_dir():
    d = tempfile.mkdtemp(prefix="wg")
    os.chmod(d, 0o700)
    return d


def raw_exchange(connect, payload, read_for=1.0):
    """Sends `payload` and returns every byte the server sends back until it closes or goes quiet."""
    s = connect()
    s.settimeout(read_for)
    s.sendall(payload)
    out = b""
    try:
        while True:
            chunk = s.recv(65536)
            if not chunk:
                break
            out += chunk
    except (socket.timeout, ConnectionResetError):
        pass
    s.close()
    return out


class Chain(unittest.TestCase):
    def setUp(self):
        self.log = tempfile.mktemp()
        self.up = MockAnthropic(self.log, [{"text": "ok"}]).start()
        self.dir = short_dir()
        self.stops = [self.up.stop]

    def tearDown(self):
        for stop in reversed(self.stops):
            stop()

    def chain(self, profile="inference", limits=Limits(), tap_limits=None):
        broker = Broker(directory=self.dir, token=TOKEN, origin=self.up.url, profile=profile, limits=limits).start()
        self.stops.append(broker.stop)
        tap = ApiTap(broker_socket=broker.path, broker_token=TOKEN, allowed_tools=allowed_api_tools(profile),
                     client_key=KEY, limits=tap_limits or limits).start()
        self.stops.append(tap.stop)
        return tap, broker

    def to_tap(self, tap):
        return lambda: socket.create_connection(("127.0.0.1", tap.server.server_address[1]))

    def to_broker(self, broker):
        def connect():
            s = socket.socket(socket.AF_UNIX)
            s.connect(broker.path)
            return s
        return connect

    def provider_saw(self):
        return len(open(self.log).read().splitlines()) if os.path.exists(self.log) else 0

    def post(self, body, key=KEY, extra=b""):
        raw = json.dumps(body).encode()
        return (b"POST /v1/messages HTTP/1.1\r\nhost: x\r\nx-api-key: " + key.encode() + b"\r\n" + extra +
                b"content-length: " + str(len(raw)).encode() + b"\r\n\r\n" + raw)

    # ---- S2: nothing a client controls reaches a response --------------------------------------
    def test_violation_text_is_a_closed_word_and_a_crlf_tool_name_cannot_inject_a_header(self):
        tap, broker = self.chain()
        hostile = {"model": "m", "tools": [{"name": "evil\r\nX-Injected: yes\r\nSet-Cookie: a=b"}]}
        via_tap = raw_exchange(self.to_tap(tap), self.post(hostile))
        direct = raw_exchange(self.to_broker(broker), self.post(hostile, extra=b"x-wyss-broker: " + TOKEN.encode() + b"\r\n"))
        for name, wire in (("tap", via_tap), ("broker", direct)):
            head = wire.split(b"\r\n\r\n", 1)[0].lower()
            self.assertIn(b" 403 ", head.split(b"\r\n")[0] + b" ", name)
            self.assertNotIn(b"x-injected", head, name)
            self.assertNotIn(b"set-cookie", head, name)
            self.assertNotIn(b"evil", wire.lower(), name)
        # The broker's violation header is exactly one closed word.
        head = direct.split(b"\r\n\r\n", 1)[0].decode().lower()
        words = [l.split(":", 1)[1].strip() for l in head.split("\r\n") if l.startswith("x-wyss-violation")]
        self.assertEqual(words, ["tool_not_allowed"])
        self.assertEqual(self.provider_saw(), 0)

    def test_every_violation_code_is_a_closed_word(self):
        cases = [("/v1/files", {"tools": []}), ("/v1/messages", []), ("/v1/messages", {"tools": "x"}),
                 ("/v1/messages", {"tools": [{"nope": 1}]}), ("/v1/messages", {"mcp_servers": [1]}),
                 ("/v1/messages", {"tools": [{"name": "Bash\r\n"}]}),
                 ("/v1/messages", {"tools": [{"name": "x", "type": "web_search_20250305"}]})]
        for path, body in cases:
            with self.assertRaises(Violation) as cm:
                check_request(path, body, frozenset({"x"}))
            self.assertEqual(closed_code(str(cm.exception)), str(cm.exception), (path, body))
        self.assertEqual(closed_code("a\r\nb"), "violation")
        self.assertEqual(closed_code("UPPER"), "violation")
        self.assertEqual(closed_code("x" * 41), "violation")

    # ---- S7: the parser's own errors say nothing of the request --------------------------------
    def test_parser_errors_do_not_reflect_the_method_the_version_or_a_header(self):
        tap, broker = self.chain()
        for connect, extra in ((self.to_tap(tap), b""), (self.to_broker(broker), b"x-wyss-broker: " + TOKEN.encode() + b"\r\n")):
            for payload in (b"EVILMETHODSECRET / HTTP/1.1\r\nhost: x\r\n\r\n",
                            b"GET / HTTP/9.9SECRETVERSION\r\nhost: x\r\n\r\n",
                            b"GET / HTTP/1.1\r\nSECRETHEADER" + b"a" * 70000 + b": 1\r\n\r\n",
                            b"GET " + b"/SECRETPATH" * 9000 + b" HTTP/1.1\r\n\r\n",
                            b"\x00\x01SECRETBINARY\r\n\r\n"):
                wire = raw_exchange(connect, payload)
                self.assertNotIn(b"SECRET", wire)
                self.assertNotIn(b"EVILMETHOD", wire)
                if wire:
                    self.assertRegex(wire.split(b"\r\n", 1)[0], rb"^HTTP/1\.[01] [45]\d\d ")
        self.assertEqual(self.provider_saw(), 0)

    def test_ambiguous_or_malformed_framing_is_refused_before_anything_is_read(self):
        tap, broker = self.chain()
        body = b'{"model":"m","tools":[]}'
        n = str(len(body)).encode()
        key = b"x-api-key: " + KEY.encode() + b"\r\n"
        tok = b"x-wyss-broker: " + TOKEN.encode() + b"\r\n"
        for connect, auth in ((self.to_tap(tap), key), (self.to_broker(broker), tok)):
            bad = [
                b"content-length: " + n + b"\r\ncontent-length: " + n + b"\r\n",          # twice, equal
                b"content-length: " + n + b"\r\ncontent-length: 99999\r\n",                # twice, different
                b"content-length: " + n + b"\r\ntransfer-encoding: chunked\r\n",           # both
                b"content-length: -5\r\n", b"content-length: 1e3\r\n", b"content-length: \xd9\xa1\r\n",
                b"content-length: " + n + b"\r\nx-fold: a\r\n b\r\n",                      # obsolete line folding
                b"content-length: " + n + b"\r\nx-nul: a\x00b\r\n",
            ]
            for extra in bad:
                wire = raw_exchange(connect, b"POST /v1/messages HTTP/1.1\r\nhost: x\r\n" + auth + extra + b"\r\n" + body)
                status = wire.split(b"\r\n", 1)[0]
                self.assertRegex(status, rb"^HTTP/1\.[01] 4\d\d ", (extra, wire))
        self.assertEqual(self.provider_saw(), 0)
        # And a clean request through the same door still works.
        wire = raw_exchange(self.to_tap(tap), b"POST /v1/messages HTTP/1.1\r\nhost: x\r\n" + key + b"content-length: " + n + b"\r\n\r\n" + body)
        self.assertTrue(wire.startswith(b"HTTP/1.1 200"), wire[:80])

    # ---- S3: what is spent is bounded -----------------------------------------------------------
    def test_connections_beyond_the_limit_are_closed_unread_and_the_slot_is_given_back(self):
        _, broker = self.chain(limits=Limits(max_connections=3, read_timeout=5.0))
        held = []
        for _ in range(3):
            c = UnixHTTPConnection(broker.path)
            c.connect()
            held.append(c)
        time.sleep(0.2)
        s = socket.socket(socket.AF_UNIX)
        s.connect(broker.path)
        s.settimeout(2)
        started = time.monotonic()
        self.assertEqual(s.recv(10), b"", "the fourth connection is closed without an answer")
        self.assertLess(time.monotonic() - started, 1.5)
        s.close()
        held.pop().close()
        time.sleep(0.3)
        c = UnixHTTPConnection(broker.path)
        c.request("GET", "/v1/nothing", headers={"x-wyss-broker": TOKEN})
        self.assertEqual(c.getresponse().status, 403)
        c.close()
        for c in held:
            c.close()

    def test_a_client_that_stops_sending_is_dropped_at_the_read_deadline(self):
        tap, broker = self.chain(limits=Limits(read_timeout=0.6))
        for connect, auth in ((self.to_tap(tap), b"x-api-key: " + KEY.encode() + b"\r\n"),
                              (self.to_broker(broker), b"x-wyss-broker: " + TOKEN.encode() + b"\r\n")):
            s = connect()
            s.settimeout(4)
            # headers say 1000 bytes, then nothing
            s.sendall(b"POST /v1/messages HTTP/1.1\r\nhost: x\r\n" + auth + b"content-length: 1000\r\n\r\nabc")
            started = time.monotonic()
            try:
                data = s.recv(100)
            except socket.timeout:
                self.fail("the server kept the half-sent request open")
            self.assertLess(time.monotonic() - started, 3.0)
            self.assertNotIn(b"abc", data)
            s.close()

    def test_bytes_in_flight_and_requests_are_budgeted_before_any_body_is_read(self):
        tap, broker = self.chain(limits=Limits(max_inflight_bytes=1024, max_requests=3))
        auth = b"x-wyss-broker: " + TOKEN.encode() + b"\r\n"
        # Declares 2 KiB with a 1 KiB budget: refused with nothing of the body sent or read.
        wire = raw_exchange(self.to_broker(broker), b"POST /v1/messages HTTP/1.1\r\nhost: x\r\n" + auth + b"content-length: 2048\r\n\r\n")
        self.assertRegex(wire.split(b"\r\n", 1)[0], rb" 503 ")
        # The refused one counted (the budget is taken before anything is read), so two more fit;
        # the next is refused on the headers alone.
        small = b'{"model":"m","tools":[]}'
        for i in range(2):
            wire = raw_exchange(self.to_broker(broker), b"POST /v1/messages HTTP/1.1\r\nhost: x\r\n" + auth + b"content-length: " + str(len(small)).encode() + b"\r\n\r\n" + small)
            self.assertTrue(wire.startswith(b"HTTP/1.1 200"), (i, wire[:60]))
        started = time.monotonic()
        wire = raw_exchange(self.to_broker(broker), b"POST /v1/messages HTTP/1.1\r\nhost: x\r\n" + auth + b"content-length: 1000\r\n\r\n", read_for=3.0)
        self.assertRegex(wire.split(b"\r\n", 1)[0], rb" 429 ")
        self.assertLess(time.monotonic() - started, 2.0, "answered without waiting for a body")

    def test_a_non_streaming_response_over_the_cap_is_refused_before_a_byte_of_it_is_forwarded(self):
        class Big(MockAnthropic):
            pass
        up = socket.socket()
        up.bind(("127.0.0.1", 0))
        up.listen(4)
        port = up.getsockname()[1]
        stop = threading.Event()

        def serve():
            up.settimeout(0.2)
            while not stop.is_set():
                try:
                    c, _ = up.accept()
                except OSError:
                    continue
                c.recv(65536)
                c.sendall(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 5000\r\n\r\n" + b"x" * 5000)
                c.close()
        t = threading.Thread(target=serve, daemon=True)
        t.start()
        broker = Broker(directory=self.dir, token=TOKEN, origin="http://127.0.0.1:%d" % port, profile="inference",
                        limits=Limits(max_response=1000)).start()
        try:
            c = UnixHTTPConnection(broker.path)
            c.request("POST", "/v1/messages", json.dumps({"model": "m", "tools": []}), {"x-wyss-broker": TOKEN})
            r = c.getresponse()
            body = r.read()
            self.assertEqual(r.status, 502)
            self.assertNotIn(b"xxxxx", body)
            c.close()
        finally:
            broker.stop()
            stop.set()
            t.join(2)
            up.close()


if __name__ == "__main__":
    unittest.main()
