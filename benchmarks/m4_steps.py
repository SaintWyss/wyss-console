"""Checks about the terminal host and its parser helpers that a desktop scenario makes through the
host's own read-only protocol (the pictures say what the window shows; these say what the host
holds). A check returns `(ok, detail)`; it never raises for a failed expectation.

The one thing here that acts is `kill-helper`, which ends a parser helper to show that the window
and the terminal go on: it first proves from /proc that the process is this run's own helper (the
executable is `wyss-screen` and its parent is the run's own host) and refuses otherwise.
"""
import base64
import json
import os
import signal
import socket
import struct
import time
from owned import signal_pinned


def call(socket_path, op, params=None):
    body = json.dumps({"op": op, "params": params}).encode()
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(15)
        sock.connect(socket_path)
        sock.sendall(struct.pack(">I", len(body)) + body)
        head = sock.recv(4)
        size = struct.unpack(">I", head)[0]
        data = b""
        while len(data) < size:
            data += sock.recv(size - len(data))
    return json.loads(data)


def _first(socket_path):
    rows = call(socket_path, "term_list")["ok"]
    return rows[0] if rows else None


def _wait(predicate, seconds):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if predicate():
            return True
        time.sleep(0.2)
    return predicate()


def check(step, ctx):
    kind, args = step[0], step[1:]
    sp = ctx["socket"]
    state = ctx.setdefault("m4", {})
    if kind == "record":
        rows = call(sp, "term_list")["ok"]
        state["terminals"] = [(r["terminal"]["terminal_id"], r["terminal"]["observed_pid"]) for r in rows]
        return bool(rows), f"{len(rows)} terminals: {state['terminals']}"
    if kind == "assert_same":
        rows = call(sp, "term_list")["ok"]
        now = [(r["terminal"]["terminal_id"], r["terminal"]["observed_pid"]) for r in rows]
        return now == state.get("terminals"), f"was {state.get('terminals')} now {now}"
    if kind == "assert_parser":
        key, want = args[0], args[1]
        timeout = args[2] if len(args) > 2 else 10

        def holds():
            row = _first(sp)
            return row is not None and row["parser"].get(key) == want
        ok = _wait(holds, timeout)
        row = _first(sp)
        return ok, f"{key}={row['parser'].get(key) if row else None!r} (want {want!r})"
    if kind == "assert_restarts":
        row = _first(sp)
        n = row["parser"]["restarts"] if row else None
        return n is not None and n >= args[0], f"restarts={n}"
    if kind == "assert_screen":
        tid = _first(sp)["terminal"]["terminal_id"]
        answer = call(sp, "term_screen", {"terminal_id": tid})["ok"]
        if not answer.get("available"):
            return False, f"no screen: {answer.get('reason')}"
        repaint = base64.b64decode(answer["repaint"])
        return args[0].encode() in repaint, f"{len(repaint)} bytes, fidelity {answer['parser']['fidelity']}"
    if kind == "host_write":
        # Setup, never the thing observed: input typed through the host's own protocol, to leave a
        # full-screen program that the window's keys did not reach in time.
        tid = _first(sp)["terminal"]["terminal_id"]
        data = args[0].encode().decode("unicode_escape").encode()
        answer = call(sp, "term_write", {"terminal_id": tid, "data": list(data)})
        return "ok" in answer, f"{len(data)} bytes"
    if kind == "assert_output":
        tid = _first(sp)["terminal"]["terminal_id"]
        seen, frm = b"", 0
        end = time.monotonic() + 10
        while time.monotonic() < end:
            out = call(sp, "term_read", {"terminal_id": tid, "from": frm, "wait_ms": 200})["ok"]
            seen += bytes(out["data"])
            if out["next"] == frm:
                break
            frm = out["next"]
        return args[0].encode() in seen, f"{len(seen)} bytes of output kept"
    if kind == "kill_helper":
        row = _first(sp)
        pid = row["parser"].get("helper_pid") if row else None
        if not pid:
            return False, "no helper pid reported"
        fd = os.pidfd_open(pid)
        try:
            # The helper is not dumpable (its /proc/<pid>/exe is not readable by anyone but root),
            # so the proof is its command name (it names itself: the host runs it from a verified
            # descriptor) and its parent: the run's own host.
            with open(f"/proc/{pid}/comm") as f:
                exe = f.read().strip()
            with open(f"/proc/{pid}/stat") as f:
                ppid = int(f.read().rsplit(")", 1)[1].split()[1])
            if exe != "wyss-screen" or ppid != ctx["host_pid"]:
                return False, f"refusing: comm={exe} ppid={ppid} host={ctx['host_pid']}"
            signal_pinned(fd, signal.SIGKILL)
            return True, f"killed helper {pid}"
        except OSError as e:
            return False, f"cannot prove or signal the helper: {e}"
        finally:
            os.close(fd)
    return False, f"unknown check {kind}"
