"""Credential broker: the only process of a Claude runtime that has a route to the provider and the
only one that ever holds the credential.

The supervised bridge and the Claude Code CLI run in a network namespace with nothing but a
loopback; the one thing they can reach outside is the unix socket of this broker. So what the
provider receives is decided here, outside the sandbox the model's process lives in:

- the origin is closed: HTTPS to `api.anthropic.com`, or (tests and the isolated fixture only, and
  only when there is no credential to send) plain HTTP to a loopback port;
- the credential is added here, to requests that passed the same policy the bridge's tap applies
  (`api_tap.check_request`: Messages paths only, bounded JSON, tools within the profile's set, no
  server-side tool fields); a request that fails it is never forwarded;
- every client must present the per-runtime token it was given; a stranger gets a silent 401;
- methods, paths, framing and sizes are closed; nothing a client sends reaches an error body, and
  no error body says anything but a fixed word.

Standard library only. It is started by the authority with the credential on stdin (never in an
environment or a command line) and ends when its stdin does.
"""

import hmac
import http.client
import json
import os
import socketserver
import ssl
import stat
import sys
import threading
import urllib.parse
from http.server import BaseHTTPRequestHandler

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `-I` does not add the script's directory
import api_tap  # noqa: E402
import profiles  # noqa: E402

ALLOWED_ORIGINS = ("https://api.anthropic.com",)
SOCKET_NAME = "broker.sock"
MAX_BODY = api_tap.MAX_BODY
MAX_REQUESTS = api_tap.MAX_REQUESTS
UPSTREAM_TIMEOUT = 600
# Hop-by-hop, identity and routing headers that never cross the broker.
DROPPED_HEADERS = frozenset((
    "host", "content-length", "connection", "keep-alive", "transfer-encoding", "te", "upgrade",
    "x-api-key", "authorization", "proxy-authorization", "cookie", "x-wyss-broker",
))


class Refused(Exception):
    """A configuration the broker will not run with. Carries a fixed word, never a value."""


def parse_origin(origin, has_credential, origins=ALLOWED_ORIGINS):
    """The scheme, host and port the broker will connect to, or `Refused`."""
    if not isinstance(origin, str) or len(origin) > 128:
        raise Refused("origin")
    parts = urllib.parse.urlsplit(origin)
    if parts.scheme == "https":
        if origin not in origins:
            raise Refused("origin_not_allowed")
        return "https", parts.hostname, parts.port or 443
    if parts.scheme == "http":
        # Plain HTTP can never carry the credential, and only to this machine.
        try:
            port = parts.port
        except ValueError:
            raise Refused("origin") from None
        if has_credential or parts.hostname != "127.0.0.1" or not port or parts.path not in ("", "/") \
                or parts.query or parts.fragment or parts.username or parts.password:
            raise Refused("plaintext_origin")
        return "http", "127.0.0.1", port
    raise Refused("origin")


class Broker:
    def __init__(self, *, directory, token, origin, profile, api_key=None, _origins=ALLOWED_ORIGINS, _context=None):
        # `_origins` and `_context` exist for the tests (a local TLS server); `main()` never passes
        # them, so no frame can widen the allowlist.
        if profile not in profiles.PROFILES:
            raise Refused("profile")
        if not isinstance(token, str) or len(token) != 64 or not all(c in "0123456789abcdef" for c in token):
            raise Refused("token")
        if api_key is not None and (not isinstance(api_key, str) or not api_key
                                    or len(api_key) > 512 or not all(33 <= ord(c) < 127 for c in api_key)):
            raise Refused("credential")
        self.scheme, self.host, self.port = parse_origin(origin, api_key is not None, _origins)
        self.token = token
        self.allowed_tools = profiles.allowed_api_tools(profile)
        self.api_key = api_key
        self.count = 0
        self.lock = threading.Lock()
        self.path = os.path.join(directory, SOCKET_NAME)
        st = os.lstat(directory)
        if not stat.S_ISDIR(st.st_mode) or st.st_uid != os.geteuid() or st.st_mode & 0o077:
            raise Refused("directory")
        try:
            if stat.S_ISSOCK(os.lstat(self.path).st_mode):
                os.unlink(self.path)  # a dead runtime's socket
        except FileNotFoundError:
            pass
        outer = self
        self.context = (_context or ssl.create_default_context()) if self.scheme == "https" else None

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_):
                pass

            # ---- answers: fixed words only ------------------------------------------------
            def _say(self, code, word, violation=None):
                raw = json.dumps({"type": "error", "error": {"type": "api_error", "message": "wyss broker: " + word}}).encode()
                self.send_response(code)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(raw)))
                if violation:
                    self.send_header("x-wyss-violation", violation[:120])
                self.send_header("connection", "close")
                self.end_headers()
                self.close_connection = True
                self.wfile.write(raw)

            def _authorized(self):
                given = (self.headers.get("x-wyss-broker") or "").encode()
                if hmac.compare_digest(given, outer.token.encode()):
                    return True
                self.close_connection = True
                self._say(401, "key")
                return False

            def _method_not_allowed(self):
                if self._authorized():
                    self._say(405, "method")

            do_PUT = do_DELETE = do_PATCH = do_HEAD = do_OPTIONS = _method_not_allowed

            # ---- forwarding -------------------------------------------------------------------
            def _forward(self, method, body):
                if outer.scheme == "https":
                    conn = http.client.HTTPSConnection(outer.host, outer.port, timeout=UPSTREAM_TIMEOUT, context=outer.context)
                else:
                    conn = http.client.HTTPConnection(outer.host, outer.port, timeout=UPSTREAM_TIMEOUT)
                headers = {k: v for k, v in self.headers.items() if k.lower() not in DROPPED_HEADERS}
                if outer.api_key:
                    headers["x-api-key"] = outer.api_key
                if body is not None:
                    headers["content-length"] = str(len(body))
                conn.request(method, self.path, body=body, headers=headers)
                resp = conn.getresponse()
                if 300 <= resp.status < 400:
                    conn.close()
                    return self._say(502, "redirect")
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

            def _budget(self):
                with outer.lock:
                    outer.count += 1
                    return outer.count <= MAX_REQUESTS

            def do_POST(self):
                if not self._authorized():
                    return
                if self.headers.get("transfer-encoding") is not None:
                    return self._say(400, "framing")
                length = self.headers.get("content-length") or "0"
                if not length.isdigit() or len(length) > 9:
                    return self._say(400, "framing")
                n = int(length)
                if n > MAX_BODY:
                    return self._say(413, "body")
                raw = self.rfile.read(n) if n else b""
                if len(raw) != n:
                    return self._say(400, "framing")
                if not self._budget():
                    return self._say(429, "budget")
                try:
                    body = json.loads(raw)
                    api_tap.check_request(self.path, body, outer.allowed_tools)
                except api_tap.Violation as e:
                    return self._say(403, "refused", violation=str(e))
                except ValueError:
                    return self._say(403, "refused", violation="json")
                try:
                    self._forward("POST", raw)
                except (OSError, http.client.HTTPException, ssl.SSLError):
                    try:
                        self._say(502, "upstream")
                    except OSError:
                        pass

            def do_GET(self):
                if not self._authorized():
                    return
                if self.path.split("?", 1)[0] != "/v1/models" or self.headers.get("content-length") not in (None, "0") \
                        or self.headers.get("transfer-encoding") is not None:
                    return self._say(403, "refused", violation="path")
                if not self._budget():
                    return self._say(429, "budget")
                try:
                    self._forward("GET", None)
                except (OSError, http.client.HTTPException, ssl.SSLError):
                    try:
                        self._say(502, "upstream")
                    except OSError:
                        pass

        class Server(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
            daemon_threads = True

        old = os.umask(0o177)
        try:
            self.server = Server(self.path, Handler)
        finally:
            os.umask(old)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def start(self):
        self.thread.start()
        return self

    def stop(self):
        self.server.shutdown()
        self.server.server_close()
        try:
            os.unlink(self.path)
        except OSError:
            pass


def main():
    """`broker.py`: one JSON line on stdin names everything; EOF on stdin ends the broker."""
    try:
        cfg = json.loads(sys.stdin.readline())
        if not isinstance(cfg, dict) or cfg.get("v") != 1 or set(cfg) - {"v", "dir", "token", "origin", "profile", "api_key"}:
            raise Refused("frame")
        broker = Broker(directory=cfg["dir"], token=cfg["token"], origin=cfg["origin"],
                        profile=cfg["profile"], api_key=cfg.get("api_key")).start()
        del cfg
    except (Refused, ValueError, KeyError, OSError) as e:
        word = e.args[0] if isinstance(e, Refused) else type(e).__name__[:40]
        print(json.dumps({"e": "broker_fatal", "code": word}), flush=True)
        return 3
    print(json.dumps({"e": "broker_ready"}), flush=True)
    sys.stdin.read()
    broker.stop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
