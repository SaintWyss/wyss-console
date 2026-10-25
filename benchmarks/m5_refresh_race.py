#!/usr/bin/env python3
"""A synthetic model of the credential-refresh race that blocks concurrent provider runtimes (M5).

What it models: N runtime processes that share ONE credential file holding a rotating refresh
token. Each process, when its access token is stale, does load -> exchange the refresh token at the
token service -> save, with no lock between processes (the shape of the Codex 0.159.0 source read
in docs/research/f8-codex-shared-state-2026-10-20.md: in-process semaphores only, in-place save).
The token service rotates strictly: a refresh token works once; presenting a spent one is terminal
(`refresh_token_reused`) for that lineage.

What it is NOT: evidence about any real provider. The service is a local stand-in, the tokens are
random strings made here, nothing touches a network, a home directory or a real credential. It
shows which *protocols* are safe under the strictest rotation rule, so that the real question
(PROVIDER_EVIDENCE_REQUIRED: what the pinned provider build actually does) is asked with a
candidate answer in hand.

Protocols compared, N = 2 and 4, with a barrier so the stale reads overlap:
  shared-unlocked   one file, no cross-process lock          -> expected: reuse failures
  shared-flock      one file, exclusive flock around the     -> expected: none, serialized
                    whole load-exchange-save
  private-copies    each process its own copy of the file    -> expected: reuse failures (a copy of
                    (what "a private home per runtime" would  a rotating token is the same lineage)
                    need), and copying is forbidden anyway
  broker-refresh    one process refreshes, others ask it     -> expected: none; the workers hold
                    (the access token only, never the         no refresh token at all
                    refresh token)

    python3 -I benchmarks/m5_refresh_race.py [--rounds 20]
"""
import argparse
import fcntl
import json
import multiprocessing as mp
from multiprocessing.managers import BaseManager
import os
import secrets
import shutil
import sys
import tempfile
import time


class TokenService:
    """Strict rotation, kept in a manager process so every worker talks to the same one."""

    def __init__(self):
        self.valid = None
        self.reused = 0
        self.issued = 0

    def start(self):
        self.valid = secrets.token_hex(8)
        return self.valid

    def exchange(self, refresh):
        if refresh != self.valid:
            self.reused += 1
            return None
        self.valid = secrets.token_hex(8)
        self.issued += 1
        return {"access": secrets.token_hex(8), "refresh": self.valid}

    def stats(self):
        return {"reused": self.reused, "issued": self.issued}


class Manager(BaseManager):
    pass


Manager.register("TokenService", TokenService)


def refresh_unlocked(service, path, barrier, results):
    with open(path) as f:
        creds = json.load(f)
    barrier.wait()  # every process has read the same stale refresh token
    got = service.exchange(creds["refresh"])
    if got is None:
        results.put("reused")
        return
    time.sleep(0.001)
    tmp = path + ".tmp." + str(os.getpid())
    with open(tmp, "w") as f:
        json.dump(got, f)
    os.replace(tmp, path)
    results.put("ok")


def refresh_flock(service, path, barrier, results):
    with open(path) as f:
        seen = json.load(f)["refresh"]  # the stale read every process starts from
    barrier.wait()
    with open(path + ".lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        with open(path) as f:
            creds = json.load(f)
        # Double-checked under the lock: if the file moved on while this process waited, someone
        # else already refreshed and their result is used instead of spending the token again.
        if creds["refresh"] != seen:
            results.put("skipped")
            return
        got = service.exchange(creds["refresh"])
        if got is None:
            results.put("reused")
            return
        with open(path, "w") as f:
            json.dump(got, f)
        results.put("ok")


def refresh_private(service, path, barrier, results):
    # path is this process's own copy, taken before the race from the same lineage.
    with open(path) as f:
        creds = json.load(f)
    barrier.wait()
    got = service.exchange(creds["refresh"])
    results.put("reused" if got is None else "ok")


def one_round(kind, n, service, workdir):
    shared = os.path.join(workdir, "auth.json")
    refresh = service.start()
    with open(shared, "w") as f:
        json.dump({"access": "stale", "refresh": refresh}, f)
    barrier = mp.Barrier(n)
    results = mp.Queue()
    procs = []
    if kind == "broker-refresh":
        # The broker is the only holder of the refresh token; workers receive access tokens.
        got = service.exchange(refresh)
        outcome = ["ok" if got else "reused"] + ["ok"] * (n - 1)
        return outcome
    for i in range(n):
        path = shared
        target = refresh_unlocked
        if kind == "shared-flock":
            target = refresh_flock
        if kind == "private-copies":
            path = os.path.join(workdir, f"copy-{i}.json")
            shutil.copy(shared, path)
            target = refresh_private
        p = mp.Process(target=target, args=(service, path, barrier, results))
        p.start()
        procs.append(p)
    for p in procs:
        p.join(10)
    out = []
    while not results.empty():
        out.append(results.get())
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--rounds", type=int, default=20)
    args = ap.parse_args()
    manager = Manager()
    manager.start()
    rows = []
    for kind in ("shared-unlocked", "shared-flock", "private-copies", "broker-refresh"):
        for n in (2, 4):
            service = manager.TokenService()
            failed_rounds = 0
            total_reused = 0
            for _ in range(args.rounds):
                with tempfile.TemporaryDirectory(prefix="m5-refresh-") as workdir:
                    before = service.stats()["reused"]
                    one_round(kind, n, service, workdir)
                    delta = service.stats()["reused"] - before
                    total_reused += delta
                    failed_rounds += 1 if delta else 0
            rows.append({"protocol": kind, "processes": n, "rounds": args.rounds,
                         "rounds_with_a_reuse_failure": failed_rounds, "reuse_failures": total_reused})
    for r in rows:
        print(json.dumps(r))
    # The claims the model is allowed to make, checked here so the output cannot drift from them.
    by = {(r["protocol"], r["processes"]): r for r in rows}
    assert by[("shared-unlocked", 4)]["reuse_failures"] > 0, "the unlocked race did not show"
    assert by[("private-copies", 2)]["reuse_failures"] > 0, "copies of a rotating token do not help"
    assert by[("broker-refresh", 4)]["reuse_failures"] == 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
