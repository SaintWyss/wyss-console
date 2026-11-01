"""Test-side driver for the bridge: the Python twin of what the Rust worker does."""
import json, os, queue, subprocess, tempfile, threading, shutil, time, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import sandbox, matrix

BRIDGE_DIR = os.path.dirname(HERE)


class BridgeSession:
    def __init__(self, venv, *, script=None, managed=False, seed=True, extra_env=None, root=None):
        self.keep = root is not None
        self.root = root or tempfile.mkdtemp(prefix="wyss-bridge-")
        if root is not None:
            seed = False
        self.home, self.ws, self.markers, self.etc = matrix.seed(self.root) if seed else [self.root + "/" + d for d in ("home", "ws", "markers", "etc")]
        for d in (self.home, self.ws, self.markers):
            os.makedirs(d, exist_ok=True)
        env = sandbox.base_env("http://127.0.0.1:1", {"WYSS_BRIDGE": BRIDGE_DIR + "/bridge.py", "WYSS_MOCK_SCRIPT": json.dumps(script), **(extra_env or {})})
        # The bridge gets only what a production launch gives it: no ANTHROPIC_* in its environment.
        for k in ("ANTHROPIC_API_KEY", "ANTHROPIC_BASE_URL"):
            env.pop(k, None)
        extra = ["--bind", self.ws, "/work/ws", "--bind", self.markers, "/markers"]
        if managed:
            extra += ["--ro-bind", self.etc + "/managed-settings.json", "/etc/claude-code/managed-settings.json"]
        cmd = sandbox.argv(home=self.home, workdir="/work/ws", ro=[venv, BRIDGE_DIR, HERE], env=env,
                           command=[venv + "/bin/python", "-I", HERE + "/in_sandbox.py"], extra=extra)
        self.proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env={})
        self.q = queue.Queue()
        self.stderr = []
        threading.Thread(target=self._pump, daemon=True).start()
        threading.Thread(target=lambda: [self.stderr.append(l) for l in self.proc.stderr], daemon=True).start()

    def _pump(self):
        for line in self.proc.stdout:
            self.q.put(json.loads(line))
        self.q.put(None)

    def send(self, **cmd):
        self.proc.stdin.write((json.dumps({"v": 1, **cmd}) + "\n").encode())
        self.proc.stdin.flush()

    def raw(self, data: bytes):
        self.proc.stdin.write(data)
        self.proc.stdin.flush()

    def recv(self, timeout=60):
        return self.q.get(timeout=timeout)

    def until(self, pred, timeout=90, collect=None):
        end = time.monotonic() + timeout
        seen = collect if collect is not None else []
        while time.monotonic() < end:
            ev = self.recv(max(0.1, end - time.monotonic()))
            if ev is None:
                return None
            seen.append(ev)
            if pred(ev):
                return ev
        raise TimeoutError("no matching event; saw %r" % [e.get("e") for e in seen][-10:])

    def close(self):
        try:
            self.proc.stdin.close()
        except OSError:
            pass
        try:
            rc = self.proc.wait(10)
        except subprocess.TimeoutExpired:
            self.proc.kill(); rc = self.proc.wait()
        if not self.keep:
            shutil.rmtree(self.root, ignore_errors=True)
        return rc
