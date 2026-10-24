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
import socket
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MAX_BODY = 8 * 1024 * 1024
MAX_REQUESTS = 512
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
            raise Violation("server_side:" + field)
    extra = sorted(set(names) - set(allowed_tools))
    if extra:
        raise Violation("tool_not_allowed:" + ",".join(extra[:4]))
    # Server-side tools (web search etc.) are typed entries with a `type`; refuse any.
    if any("type" in t and t.get("type") not in (None, "custom") for t in tools):
        raise Violation("server_tool")
    if model is not None and base == "/v1/messages" and body.get("model") != model:
        raise Violation("model_mismatch")
    out = body.get("output_config") or {}
    thinking = body.get("thinking") or {}
    return {
        "path": base,
        "tools": sorted(names),
        "model": body.get("model") if isinstance(body.get("model"), str) else None,
        "effort": out.get("effort") if isinstance(out, dict) else None,
        "thinking": thinking.get("type") if isinstance(thinking, dict) else None,
    }


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
    def __init__(self, *, broker_socket, broker_token, allowed_tools, model=None, on_event=None, client_key=None):
        self.broker_socket = broker_socket
        self.broker_token = broker_token
        self.allowed_tools = frozenset(allowed_tools)
        self.client_key = client_key
        self.model = model
        self.on_event = on_event or (lambda e: None)
        self.count = 0
        self.lock = threading.Lock()
        outer = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_):
                pass

            def _authorized(self):
                """Only the CLI this tap was made for, which holds the per-run key. A refusal says
                nothing to the bridge (a stranger must not be able to raise a violation) and ends
                the connection without reading a body."""
                if outer.client_key is None:
                    return True
                given = self.headers.get("x-api-key") or ""
                if hmac.compare_digest(given.encode(), outer.client_key.encode()):
                    return True
                self.close_connection = True
                self._refuse(401, "key")
                return False

            def _refuse(self, code, why):
                raw = json.dumps({"type": "error", "error": {"type": "permission_error", "message": "wyss tap: " + why}}).encode()
                self.send_response(code)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            def _forward(self, method, body):
                conn = UnixHTTPConnection(outer.broker_socket)
                # The CLI's own (synthetic) key never goes on; the broker adds the real one.
                headers = {k: v for k, v in self.headers.items()
                           if k.lower() not in ("host", "content-length", "connection", "x-api-key", "authorization", "transfer-encoding")}
                headers["x-wyss-broker"] = outer.broker_token
                if body is not None:
                    headers["content-length"] = str(len(body))
                conn.request(method, self.path, body=body, headers=headers)
                resp = conn.getresponse()
                if resp.status == 403 and resp.getheader("x-wyss-violation"):
                    outer.on_event({"e": "tap_violation", "why": "broker:" + resp.getheader("x-wyss-violation")[:100]})
                self.send_response(resp.status)
                streaming = "text/event-stream" in (resp.getheader("content-type") or "")
                for k, v in resp.getheaders():
                    if k.lower() in ("content-length", "transfer-encoding", "connection", "keep-alive", "x-wyss-violation"):
                        continue
                    self.send_header(k, v)
                if streaming:
                    self.send_header("transfer-encoding", "chunked")
                    self.end_headers()
                    while True:
                        chunk = resp.read1(8192) if hasattr(resp, "read1") else resp.read(8192)
                        if not chunk:
                            break
                        self.wfile.write(b"%x\r\n%s\r\n" % (len(chunk), chunk))
                        self.wfile.flush()
                    self.wfile.write(b"0\r\n\r\n")
                else:
                    data = resp.read()
                    self.send_header("content-length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)
                conn.close()

            def _method_not_allowed(self):
                if self._authorized():
                    self.close_connection = True
                    self._refuse(405, "method")

            do_PUT = do_DELETE = do_PATCH = do_HEAD = do_OPTIONS = _method_not_allowed

            def do_POST(self):
                if not self._authorized():
                    return
                length = self.headers.get("content-length") or "0"
                if self.headers.get("transfer-encoding") is not None or not length.isdigit() or len(length) > 9:
                    outer.on_event({"e": "tap_violation", "why": "framing"})
                    self.close_connection = True
                    return self._refuse(400, "framing")
                n = int(length)
                if n > MAX_BODY:
                    outer.on_event({"e": "tap_violation", "why": "body_too_large"})
                    self.close_connection = True
                    return self._refuse(413, "body")
                raw = self.rfile.read(n) if n else b""
                if len(raw) != n:
                    self.close_connection = True
                    return self._refuse(400, "framing")
                with outer.lock:
                    outer.count += 1
                    over = outer.count > MAX_REQUESTS
                if over:
                    outer.on_event({"e": "tap_violation", "why": "request_budget"})
                    return self._refuse(429, "budget")
                try:
                    body = json.loads(raw)
                    summary = check_request(self.path, body, outer.allowed_tools, outer.model)
                except (ValueError, Violation) as e:
                    why = str(e) if isinstance(e, Violation) else "json"
                    outer.on_event({"e": "tap_violation", "why": why[:120]})
                    return self._refuse(403, why[:120])
                outer.on_event({"e": "tap_request", **summary})
                try:
                    self._forward("POST", raw)
                except (OSError, http.client.HTTPException):
                    outer.on_event({"e": "tap_upstream_error"})
                    try:
                        self._refuse(502, "upstream")
                    except OSError:
                        pass

            def do_GET(self):
                if not self._authorized():
                    return
                # The CLI may list models; a read-only listing cannot carry tools.
                if self.path.split("?", 1)[0] != "/v1/models":
                    outer.on_event({"e": "tap_violation", "why": "path"})
                    return self._refuse(403, "path")
                try:
                    self._forward("GET", None)
                except (OSError, http.client.HTTPException):
                    self._refuse(502, "upstream")

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
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
