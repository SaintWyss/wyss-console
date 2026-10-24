"""Loopback API tap: an independent check of what the model is actually offered.

The Claude Code CLI is pointed at this tap (`ANTHROPIC_BASE_URL`). Every request is parsed
and checked *before* it is forwarded: the path must be one of the Messages endpoints, the
body must be bounded JSON, and `tools` must be a subset of the profile's allowed set. A
violation is answered with 403 and never reaches the provider. Whatever the SDK reports
about itself, this is what the provider receives.

It holds no credential and has no route to the provider. The CLI runs in a network namespace
with nothing but a loopback, the tap listens there, and what passes the checks is handed to the
credential broker (`broker.py`) over a unix socket, which re-applies the same policy outside this
sandbox, adds the credential and is the only process that connects to the provider. The CLI is
given a per-run synthetic key that is also the tap's password (`client_key`), so nothing else that
shares the loopback of the namespace can use it.

Standard library only; one thread per connection; bounded body, bounded request count.
"""

import hmac
import http.client
import json
import re
import os
import socket
import socketserver
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `-I` does not add the script's directory
import httpguard  # noqa: E402
from httpguard import BoundedServerMixin, Gauge, GuardedHandler, Limits, closed_code  # noqa: E402

MAX_BODY = Limits.max_body
MAX_REQUESTS = Limits.max_requests
ALLOWED_PATHS = ("/v1/messages", "/v1/messages/count_tokens")


class Violation(Exception):
    pass


def check_request(path, body, allowed_tools, model=None):
    """Pure policy. Returns a summary dict or raises `Violation`."""
    base = path.split("?", 1)[0]
    if base not in ALLOWED_PATHS:
        raise Violation("path")
    if not isinstance(body, dict):
        raise Violation("body")
    tools = body.get("tools", [])
    if not isinstance(tools, list):
        raise Violation("tools")
    names = []
    for t in tools:
        if not isinstance(t, dict) or not isinstance(t.get("name"), str):
            raise Violation("tool_shape")
        names.append(t["name"])
    # Fields that make the provider run something on its side, outside the tool list.
    for field in ("mcp_servers", "container"):
        if body.get(field):
            raise Violation("server_side")
    extra = sorted(set(names) - set(allowed_tools))
    if extra:
        # Which names is deliberately not said: they are the client's bytes, and a violation is
        # a closed word that travels in a header and an event.
        raise Violation("tool_not_allowed")
    # Server-side tools (web search etc.) are typed entries with a `type`; refuse any.
    if any("type" in t and t.get("type") not in (None, "custom") for t in tools):
        raise Violation("server_tool")
    if model is not None and base == "/v1/messages" and body.get("model") != model:
        raise Violation("model_mismatch")
    out = body.get("output_config") or {}
    thinking = body.get("thinking") or {}
    effort = out.get("effort") if isinstance(out, dict) else None
    mode = thinking.get("type") if isinstance(thinking, dict) else None
    return {
        "path": base,
        "tools": sorted(names),
        "model": _model_word(body.get("model")),
        "effort": _word(effort, EFFORTS),
        "thinking": _word(mode, THINKING),
    }


# What the summary of a request may carry. The request is the model's bytes; the summary is an event
# that reaches the authority's record, so a value outside these sets is reported as "other", never echoed.
EFFORTS = frozenset(("low", "medium", "high", "xhigh", "max"))
THINKING = frozenset(("enabled", "disabled", "adaptive"))
_OTHER = "other"
_MODEL = re.compile(r"[a-z0-9][a-z0-9._-]{0,63}")


def _word(value, allowed):
    if value is None:
        return None
    return value if isinstance(value, str) and value in allowed else _OTHER


def _model_word(value):
    if value is None:
        return None
    return value if isinstance(value, str) and _MODEL.fullmatch(value) else _OTHER


MAX_VIOLATION_EVENTS = 16


class UnixHTTPConnection(http.client.HTTPConnection):
    """HTTP over the broker's unix socket. A socket that is not there yet (the broker is still
    starting) is retried briefly; anything else is the caller's error."""

    def __init__(self, path, timeout=600):
        super().__init__("localhost", timeout=timeout)
        self._path = path

    def connect(self):
        last = None
        for _ in range(30):
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            sock.settimeout(self.timeout)
            try:
                sock.connect(self._path)
            except (FileNotFoundError, ConnectionRefusedError) as e:
                sock.close()
                last = e
                time.sleep(0.1)
                continue
            self.sock = sock
            return
        raise last


class ApiTap:
    def on_event(self, event):
        """Deliver an event to the bridge. Violations are capped: a client that keeps misbehaving
        gets the first `MAX_VIOLATION_EVENTS` reported and one closing word, not an unbounded queue
        of work on the bridge's loop. Refusals themselves are unaffected."""
        if event.get("e") == "tap_violation":
            with self.lock:
                self.violations += 1
                n = self.violations
            if n > MAX_VIOLATION_EVENTS + 1:
                return
            if n == MAX_VIOLATION_EVENTS + 1:
                event = {"e": "tap_violation", "why": "violations_suppressed"}
        self._deliver(event)

    def __init__(self, *, broker_socket, broker_token, allowed_tools, model=None, on_event=None, client_key=None,
                 limits=Limits()):
        self.broker_socket = broker_socket
        self.broker_token = broker_token
        self.allowed_tools = frozenset(allowed_tools)
        self.client_key = client_key
        self.model = model
        self._deliver = on_event or (lambda e: None)
        self.limits = limits
        self.count = 0
        self.violations = 0
        self.lock = threading.Lock()
        self.gauge = Gauge(limits.max_inflight_bytes)
        outer = self
        configured = limits

        class Handler(GuardedHandler):
            timeout = configured.read_timeout
            limits = configured

            # ---- answers: closed words only, and nothing a client sent -----------------------------
            def _say(self, code, word, violation=None):
                raw = json.dumps({"type": "error", "error": {"type": "permission_error", "message": "wyss tap: " + word}}).encode()
                self.send_response(code)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(raw)))
                if self.close_connection:
                    self.send_header("connection", "close")
                self.end_headers()
                if self.command != "HEAD":
                    self.wfile.write(raw)

            def _authorized(self):
                """Only the CLI this tap was made for, which holds the per-run key. A refusal says
                nothing to the bridge (a stranger must not be able to raise a violation) and ends
                the connection without reading a body."""
                if outer.client_key is None:
                    return True
                given = self.headers.get_all("x-api-key") or []
                if len(given) == 1 and hmac.compare_digest(given[0].encode(), outer.client_key.encode()):
                    return True
                self.refuse(401, "key")
                return False

            def _counted(self):
                with outer.lock:
                    outer.count += 1
                    return outer.count <= outer.limits.max_requests

            # ---- forwarding to the broker --------------------------------------------------------
            def _forward(self, method, body):
                conn = UnixHTTPConnection(outer.broker_socket)
                # The CLI's own (synthetic) key never goes on; the broker adds the real one.
                headers = {k: v for k, v in self.headers.items()
                           if k.lower() not in ("host", "content-length", "connection", "x-api-key", "authorization", "transfer-encoding")}
                headers["x-wyss-broker"] = outer.broker_token
                if body is not None:
                    headers["content-length"] = str(len(body))
                try:
                    conn.request(method, self.path, body=body, headers=headers)
                    resp = conn.getresponse()
                    violation = resp.getheader("x-wyss-violation")
                    if resp.status == 403 and violation:
                        outer.on_event({"e": "tap_violation", "why": "broker:" + closed_code(violation)})
                    streaming = "text/event-stream" in (resp.getheader("content-type") or "")
                    passed = [(k, v) for k, v in resp.getheaders()
                              if k.lower() not in ("content-length", "transfer-encoding", "connection", "keep-alive", "x-wyss-violation")]
                    if not streaming:
                        data = resp.read(outer.limits.max_response + 1)
                        if len(data) > outer.limits.max_response:
                            return self.refuse(502, "response_too_large")
                    self.send_response(resp.status)
                    for k, v in passed:
                        self.send_header(k, v)
                    if streaming:
                        self.send_header("transfer-encoding", "chunked")
                        self.end_headers()
                        sent = 0
                        until = self.exchange_started()
                        while True:
                            if time.monotonic() > until:
                                self.close_connection = True
                                return
                            chunk = resp.read1(8192) if hasattr(resp, "read1") else resp.read(8192)
                            if not chunk:
                                break
                            sent += len(chunk)
                            if sent > outer.limits.max_stream:
                                self.close_connection = True
                                return
                            self.wfile.write(b"%x\r\n%s\r\n" % (len(chunk), chunk))
                            self.wfile.flush()
                        self.wfile.write(b"0\r\n\r\n")
                    else:
                        self.send_header("content-length", str(len(data)))
                        self.end_headers()
                        self.wfile.write(data)
                finally:
                    conn.close()

            def _method_not_allowed(self):
                if self._authorized():
                    self.refuse(405, "method")

            do_PUT = do_DELETE = do_PATCH = do_HEAD = do_OPTIONS = _method_not_allowed

            def do_POST(self):
                if not self._authorized():
                    return
                n = self.length()
                if n is None:
                    return
                # Budget first: a refused request is never read.
                if not self._counted():
                    outer.on_event({"e": "tap_violation", "why": "request_budget"})
                    return self.refuse(429, "budget")
                raw = self.body(n, outer.gauge)
                if raw is None:
                    return
                try:
                    body = json.loads(raw)
                    summary = check_request(self.path, body, outer.allowed_tools, outer.model)
                except (ValueError, Violation) as e:
                    why = closed_code(str(e)) if isinstance(e, Violation) else "json"
                    outer.on_event({"e": "tap_violation", "why": why})
                    return self._say(403, why)
                outer.on_event({"e": "tap_request", **summary})
                try:
                    self._forward("POST", raw)
                except (OSError, http.client.HTTPException):
                    outer.on_event({"e": "tap_upstream_error"})
                    try:
                        self.refuse(502, "upstream")
                    except OSError:
                        pass

            def do_GET(self):
                if not self._authorized():
                    return
                n = self.length()
                if n is None:
                    return
                # The CLI may list models; a read-only listing cannot carry tools or a body.
                if n != 0 or self.path.split("?", 1)[0] != "/v1/models":
                    outer.on_event({"e": "tap_violation", "why": "path"})
                    return self.refuse(403, "path")
                if not self._counted():
                    return self.refuse(429, "budget")
                try:
                    self._forward("GET", None)
                except (OSError, http.client.HTTPException):
                    try:
                        self.refuse(502, "upstream")
                    except OSError:
                        pass

        class Server(BoundedServerMixin, socketserver.ThreadingMixIn, socketserver.TCPServer):
            daemon_threads = True
            allow_reuse_address = True

        self.server = Server(("127.0.0.1", 0), Handler)
        self.server.bound(limits)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def url(self):
        host, port = self.server.server_address
        return "http://%s:%d" % (host, port)

    def start(self):
        self.thread.start()
        return self

    def stop(self):
        self.server.shutdown()
        self.server.server_close()
