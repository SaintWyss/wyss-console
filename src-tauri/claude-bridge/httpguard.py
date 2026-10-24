"""Hardening shared by the two local HTTP endpoints (the bridge's tap and the credential broker).

What the endpoints say is a closed set of words: no byte of a request ever reaches a response
(not a method, a version, a header or a tool name), so nothing a client controls can inject a
header or forge a line of protocol. What they accept is closed too: one length per request, no
chunking, no folded or control-bearing headers. What they spend is bounded: connections,
bytes in flight, time to read, requests, response sizes. Standard library only.
"""

import re
import threading
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler

_CODE = re.compile(r"^[a-z0-9_]{1,40}$")
_BAD_VALUE = ("\r", "\n", "\x00")


def closed_code(text, default="violation"):
    """`text` if it is already a closed word, else `default`: never echoes what it was given."""
    return text if isinstance(text, str) and _CODE.match(text) else default


@dataclass(frozen=True)
class Limits:
    max_connections: int = 32
    read_timeout: float = 30.0
    max_body: int = 8 * 1024 * 1024
    max_inflight_bytes: int = 64 * 1024 * 1024
    max_requests: int = 512
    max_response: int = 32 * 1024 * 1024
    max_stream: int = 64 * 1024 * 1024


class Gauge:
    """Bytes being read right now, across every connection."""

    def __init__(self, cap):
        self.cap, self.used, self.lock = cap, 0, threading.Lock()

    def reserve(self, n):
        with self.lock:
            if self.used + n > self.cap:
                return False
            self.used += n
            return True

    def release(self, n):
        with self.lock:
            self.used -= n


class BoundedServerMixin:
    """For `socketserver.ThreadingMixIn` servers: a connection beyond the limit is closed unread,
    and an error inside a handler prints nothing (a traceback is not a protocol)."""

    def bound(self, limits):
        self._slots = threading.BoundedSemaphore(limits.max_connections)

    def process_request(self, request, client_address):
        if not self._slots.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self._slots.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self._slots.release()

    def handle_error(self, request, client_address):
        pass


class GuardedHandler(BaseHTTPRequestHandler):
    """A handler that answers in fixed words (`_say`, supplied by the subclass) and refuses an
    ambiguous request before reading anything of it."""

    protocol_version = "HTTP/1.1"
    limits = Limits()

    def log_message(self, *_):
        pass

    def send_error(self, code, message=None, explain=None):
        # The parser's own errors quote the method, the version or the header; these do not.
        self.close_connection = True
        try:
            status = int(code)
            self._say(status if 400 <= status < 600 else 400, "http")
        except (OSError, ValueError):
            pass

    def refuse(self, status, word):
        self.close_connection = True
        self._say(status, word)

    def length(self):
        """The body length this request declares, or `None` after refusing it. Nothing is read."""
        for key, value in self.headers.items():
            if any(c in value for c in _BAD_VALUE) or any(c in key for c in ("\r", "\n", "\x00", " ", "\t")):
                self.refuse(400, "framing")
                return None
        if self.headers.get_all("transfer-encoding") is not None:
            self.refuse(400, "framing")
            return None
        lengths = self.headers.get_all("content-length") or []
        if len(lengths) > 1:
            self.refuse(400, "framing")
            return None
        text = lengths[0] if lengths else "0"
        if not text.isascii() or not text.isdigit() or len(text) > 9:
            self.refuse(400, "framing")
            return None
        n = int(text)
        if n > self.limits.max_body:
            self.refuse(413, "body")
            return None
        return n

    def body(self, n, gauge):
        """The `n` bytes of the body, within the bytes-in-flight budget and the read deadline; or
        `None` after refusing."""
        if not gauge.reserve(n):
            self.refuse(503, "busy")
            return None
        try:
            raw = self.rfile.read(n) if n else b""
        except OSError:
            self.close_connection = True
            return None
        finally:
            gauge.release(n)
        if len(raw) != n:
            self.refuse(400, "framing")
            return None
        return raw
