#!/usr/bin/env python3
"""One idle run of a GUI binary on an isolated X display the caller made (DISPLAY :77).

    WYSS_AB_OUT=dir python3 -I benchmarks/idle_ab.py BINARY LABEL SECONDS

Replaces `idle-ab.sh`. The only process it ever ends is the one it launched (see `owned.py`); its
web process is adopted by descriptor to be measured, never searched for by name to be ended.
"""
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from owned import Owned, NotOurs


def main():
    binary, label, secs = os.path.realpath(sys.argv[1]), sys.argv[2], sys.argv[3]
    out = os.environ["WYSS_AB_OUT"]
    os.makedirs(out, exist_ok=True)
    env = {k: v for k, v in os.environ.items() if k != "WAYLAND_DISPLAY"}
    env.update(DISPLAY=":77", GDK_BACKEND="x11", NO_AT_BRIDGE="1")
    here = os.path.dirname(os.path.abspath(__file__))
    with open(f"{out}/{label}.app.log", "wb") as log:
        try:
            app = Owned([binary], binary, env=env, log=log)
        except NotOurs as why:
            print(f"ABORT: {why}; only the launched process was ended", file=sys.stderr)
            return 1
        with app:
            time.sleep(23)
            web = app.adopt_child("WebKitWebProces")
            if web is None:
                print(f"ABORT: no web process under {app.pid}", file=sys.stderr)
                return 1
            head = f"label={label} app={app.pid} web={web} loadavg={' '.join(open('/proc/loadavg').read().split()[:3])}"
            print(head)
            jobs = []
            for who, pid in (("web", web), ("app", app.pid)):
                path = f"{out}/{label}.{who}.txt"
                jobs.append((who, path, subprocess.Popen(
                    [sys.executable, "-I", f"{here}/threads.py", str(pid), secs],
                    stdout=open(path, "w"))))
            for _, _, job in jobs:
                job.wait()
            lines = [head]
            for who, path, _ in jobs:
                last = open(path).read().strip().splitlines()[-1]
                print(f"{who}:  {last}")
                lines.append(f"{who}:  {last}")
            open(f"{out}/{label}.txt", "w").write("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
