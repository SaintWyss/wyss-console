"""Hardening shared by the two local HTTP endpoints (the bridge's tap and the credential broker).

What the endpoints say is a closed set of words: no byte of a request ever reaches a response
(not a method, a version, a header or a tool name), so nothing a client controls can inject a
header or forge a line of protocol. What they accept is closed too: one length per request, no
chunking, no folded or control-bearing headers. What they spend is bounded: connections,
bytes being read in flight (a bound on what is held at once, not on the process's memory), idle
time and total time to read a request, requests, response sizes. Standard library only.
"""

import re
import threading
import time
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler

_CODE = re.compile(r"[a-z0-9_]{1,40}")
_TOKEN = re.compile(rb"[!#$%&'*+\-.^_`|~0-9A-Za-z]+")
_BAD_VALUE = ("\r", "\n", "\x00")


def closed_code(text, default="violation"):
    """`text` if it is already a closed word, else `default`: never echoes what it was given."""
    return text if isinstance(text, str) and _CODE.fullmatch(text) else default


@dataclass(frozen=True)
class Limits:
    max_connections: int = 32
    # Idle time on a socket, and the total time a request may take to arrive (request line, headers
    # and body together): a peer that drips a byte before each idle timeout still runs out of the total.
    read_timeout: float = 30.0
    request_deadline: float = 30.0
    # The total time of one exchange with the other side (the provider, or the broker).
    exchange_deadline: float = 1800.0
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


class _Tee:
    """The request file: remembers the raw lines the parser read, and enforces the request's total
    deadline on every read."""

    def __init__(self, raw, handler):
        self._raw = raw
        self._handler = handler
        self.lines = []

    def readline(self, limit=-1):
        # One socket read at a time, the deadline checked before each: a peer that drips a byte per
        # idle period cannot stretch a line past the request's total time.
        out = b""
        while limit < 0 or len(out) < limit:
            self._handler.tick()
            buf = self._raw.peek(1)
            if not buf:
                break
            room = len(buf) if limit < 0 else min(len(buf), limit - len(out))
            cut = buf.find(b"\n", 0, room)
            take = cut + 1 if cut >= 0 else room
            out += self._raw.read(take)
            if cut >= 0:
                break
        self.lines.append(out)
        return out

    def read(self, n=-1):
        out = b""
        while n < 0 or len(out) < n:
            self._handler.tick()
            chunk = self._raw.read1(4096 if n < 0 else min(4096, n - len(out)))
            if not chunk:
                break
            out += chunk
        return out

    def __getattr__(self, name):
        return getattr(self._raw, name)


class GuardedHandler(BaseHTTPRequestHandler):
    """A handler that answers in fixed words (`_say`, supplied by the subclass) and refuses an
    ambiguous request before reading anything of it."""

    protocol_version = "HTTP/1.1"
    limits = Limits()

    deadline = None

    def log_message(self, *_):
        pass

    def setup(self):
        super().setup()
        self.rfile = _Tee(self.rfile, self)

    def tick(self):
        """Called before every read of a request: the total deadline, and the socket's own timeout
        never longer than what is left of it."""
        if self.deadline is None:
            return
        left = self.deadline - time.monotonic()
        if left <= 0:
            raise TimeoutError("request deadline")
        self.connection.settimeout(max(0.05, min(self.limits.read_timeout, left)))

    def parse_request(self):
        self.rfile.lines = []
        if not super().parse_request():
            return False
        # The parser drops a header line it cannot read and everything after it; what it kept must be
        # every line that was sent, each one `token: value` with no folding and no control bytes.
        kept = 0
        for line in self.rfile.lines:
            if line in (b"\r\n", b"\n"):
                break
            name, colon, value = line.partition(b":")
            body = value.rstrip(b"\r\n")
            if (line[:1] in (b" ", b"\t") or not colon or not _TOKEN.fullmatch(name)
                    or any((b < 0x20 and b != 0x09) or b >= 0x7F for b in body)):
                self.send_error(400)
                return False
            kept += 1
        if kept != len(self.headers):
            self.send_error(400)
            return False
        return True

    def handle_one_request(self):
        self.deadline = time.monotonic() + self.limits.request_deadline
        try:
            super().handle_one_request()
        finally:
            self.deadline = None

    def exchange_started(self):
        """The moment the other side is spoken to: the exchange's own total deadline starts."""
        return time.monotonic() + self.limits.exchange_deadline

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
        """The `n` bytes of the body, within the bytes-in-flight budget and the request's total deadline; or
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
