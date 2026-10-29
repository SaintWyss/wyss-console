"""Tamper-evident evidence stream (v2).

v1 defect (independent review): `/markers` is writable by the system under test and the
evidence carried no binding digests, so captures could be altered after the fact.

Design: the host generates a random per-case key and writes it on the sandbox harness's stdin
(never in argv or env). The in-sandbox harness reads it, replaces stdin with /dev/null and keeps
the key only in memory (Yama ptrace_scope=1 stops the provider, a descendant of nobody here,
from attaching to read it). Every evidence record is emitted live on the harness's stdout as one
line `A0REC <seq> <json> <hmac>` where hmac = HMAC-SHA256(key, prev_hmac || seq || json).
The provider never holds the harness's stdout (its own stdio is a pipe or a PTY). A forged or
reordered line breaks the chain; a missing END record means incomplete evidence.
"""
import hashlib
import hmac
import json

PREFIX = "A0REC "


class Emitter:
    def __init__(self, key: bytes, out):
        self.key, self.out = key, out
        self.prev = b"\x00" * 32
        self.seq = 0

    def emit(self, kind, **fields):
        body = json.dumps({"kind": kind, **fields}, sort_keys=True, separators=(",", ":"))
        mac = hmac.new(self.key, self.prev + str(self.seq).encode() + body.encode(), hashlib.sha256).digest()
        self.out.write(f"{PREFIX}{self.seq} {body} {mac.hex()}\n")
        self.out.flush()
        self.prev = mac
        self.seq += 1


def verify(key: bytes, text: str):
    """Return (records, problems). Non-record lines are ignored but counted."""
    prev = b"\x00" * 32
    expected = 0
    records, problems, foreign = [], [], 0
    for line in text.splitlines():
        if not line.startswith(PREFIX):
            foreign += 1
            continue
        try:
            _, seq, rest = line.split(" ", 2)
            body, mac_hex = rest.rsplit(" ", 1)
            seq = int(seq)
        except ValueError:
            problems.append("unparseable_record")
            continue
        mac = hmac.new(key, prev + str(seq).encode() + body.encode(), hashlib.sha256).digest()
        if seq != expected or not hmac.compare_digest(mac.hex(), mac_hex):
            problems.append(f"bad_record_at_seq:{seq}")
            continue
        records.append(json.loads(body))
        prev = mac
        expected += 1
    if not records or records[-1].get("kind") != "END":
        problems.append("missing_END")
    return records, problems, foreign
