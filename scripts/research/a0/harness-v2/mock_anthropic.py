"""Synthetic Anthropic Messages API on loopback, for isolated SDK experiments.

It exists so the real Claude Code CLI can be driven with a synthetic credential and no
network: every request body the CLI sends is appended to a JSONL log, which is the ground
truth of what the model would be offered (tool definitions, system prompt, model, effort),
independent of anything the SDK reports about itself.

Responses are scripted: each `/v1/messages` call consumes the next entry of the script
(a list of dicts); the last entry repeats. An entry is `{"text": str}` or
`{"tool_use": {"name": str, "input": dict}}`, plus optional `usage` and `model`.
"""

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DEFAULT_USAGE = {
    "input_tokens": 11,
    "output_tokens": 7,
    "cache_creation_input_tokens": 3,
    "cache_read_input_tokens": 5,
}


class MockAnthropic:
    def __init__(self, log_path, script=None, host="127.0.0.1", port=0, on_record=None):
        self.log_path = log_path
        self.on_record = on_record  # v2: live digest of every recorded line (tamper evidence)
        self.script = list(script or [{"text": "ok from mock"}])
        self.calls = 0
        self.lock = threading.Lock()
        outer = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_):
                pass

            def _body(self):
                n = int(self.headers.get("content-length") or 0)
                return self.rfile.read(n) if n else b""

            def _record(self, body):
                try:
                    parsed = json.loads(body) if body else None
                except ValueError:
                    parsed = {"_unparsed_bytes": len(body)}
                entry = {
                    "method": self.command,
                    "path": self.path,
                    "headers": {k.lower(): v for k, v in self.headers.items()
                                if k.lower() not in ("x-api-key", "authorization")},
                    "has_key": bool(self.headers.get("x-api-key")),
                    "key_sha256": __import__("hashlib").sha256((self.headers.get("x-api-key") or "").encode()).hexdigest(),
                    "body": parsed,
                }
                line = json.dumps(entry) + "\n"
                with outer.lock:
                    with open(outer.log_path, "a") as f:
                        f.write(line)
                    if outer.on_record is not None:
                        outer.on_record(line.encode())
                return parsed

            def _json(self, code, value):
                raw = json.dumps(value).encode()
                self.send_response(code)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            def do_GET(self):
                self._record(b"")
                if self.path.startswith("/v1/models"):
                    self._json(200, {"data": [], "has_more": False, "first_id": None, "last_id": None})
                else:
                    self._json(404, {"type": "error", "error": {"type": "not_found_error", "message": "mock"}})

            def do_POST(self):
                body = self._body()
                parsed = self._record(body)
                if self.path.startswith("/v1/messages/count_tokens"):
                    return self._json(200, {"input_tokens": 7})
                if not self.path.startswith("/v1/messages"):
                    return self._json(404, {"type": "error", "error": {"type": "not_found_error", "message": "mock"}})
                with outer.lock:
                    idx = min(outer.calls, len(outer.script) - 1)
                    outer.calls += 1
                    step = outer.script[idx]
                model = step.get("model") or (parsed or {}).get("model") or "mock-model"
                if step.get("http_error"):
                    return self._json(step["http_error"], {"type": "error", "error": {"type": "api_error", "message": "mock failure"}})
                usage = dict(step.get("usage") or DEFAULT_USAGE)
                events = []
                msg = {"id": "msg_mock_%d" % idx, "type": "message", "role": "assistant", "model": model,
                       "content": [], "stop_reason": None, "stop_sequence": None,
                       "usage": {**usage, "output_tokens": 1}}
                events.append(("message_start", {"type": "message_start", "message": msg}))
                if "tool_use" in step:
                    tu = step["tool_use"]
                    events.append(("content_block_start", {"type": "content_block_start", "index": 0,
                        "content_block": {"type": "tool_use", "id": "toolu_mock_%d" % idx, "name": tu["name"], "input": {}}}))
                    events.append(("content_block_delta", {"type": "content_block_delta", "index": 0,
                        "delta": {"type": "input_json_delta", "partial_json": json.dumps(tu["input"])}}))
                    stop = "tool_use"
                else:
                    events.append(("content_block_start", {"type": "content_block_start", "index": 0,
                        "content_block": {"type": "text", "text": ""}}))
                    text = step.get("text", "ok from mock")
                    for i in range(0, len(text), 16):
                        events.append(("content_block_delta", {"type": "content_block_delta", "index": 0,
                            "delta": {"type": "text_delta", "text": text[i:i + 16]}}))
                    stop = "end_turn"
                events.append(("content_block_stop", {"type": "content_block_stop", "index": 0}))
                events.append(("message_delta", {"type": "message_delta",
                    "delta": {"stop_reason": stop, "stop_sequence": None},
                    "usage": {"output_tokens": usage["output_tokens"]}}))
                events.append(("message_stop", {"type": "message_stop"}))
                if (parsed or {}).get("stream"):
                    self.send_response(200)
                    self.send_header("content-type", "text/event-stream")
                    self.send_header("transfer-encoding", "chunked")
                    self.end_headers()
                    for name, data in events:
                        chunk = ("event: %s\ndata: %s\n\n" % (name, json.dumps(data))).encode()
                        self.wfile.write(b"%x\r\n%s\r\n" % (len(chunk), chunk))
                        self.wfile.flush()
                        if step.get("delay"):
                            import time
                            time.sleep(step["delay"])
                    self.wfile.write(b"0\r\n\r\n")
                else:
                    msg["content"] = [{"type": "text", "text": step.get("text", "ok from mock")}]
                    msg["stop_reason"] = stop
                    msg["usage"] = usage
                    self._json(200, msg)

        self.server = ThreadingHTTPServer((host, port), Handler)
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


class ConnectLogger:
    """A forward proxy that refuses everything and records what was asked for, to learn which
    destinations the CLI tries besides its configured API base URL."""

    def __init__(self, log_path, host="127.0.0.1"):
        import socketserver

        outer = self

        class H(socketserver.StreamRequestHandler):
            def handle(self):
                line = self.rfile.readline(4096).decode("latin-1", "replace").strip()
                with open(outer.log_path, "a") as f:
                    f.write(line + "\n")
                self.wfile.write(b"HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\n\r\n")

        self.log_path = log_path
        self.server = socketserver.ThreadingTCPServer((host, 0), H)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def url(self):
        h, p = self.server.server_address
        return "http://%s:%d" % (h, p)

    def start(self):
        self.thread.start()
        return self

    def stop(self):
        self.server.shutdown()
        self.server.server_close()
