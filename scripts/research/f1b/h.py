import json, os, subprocess, sys, tempfile, time, select, shutil, pathlib

class App:
    def __init__(self, home, codex_home, cwd, extra=(), prefix=()):
        env = {k: v for k, v in os.environ.items() if not k.startswith(('CLAUDE', 'CODEX', 'DBUS', 'XDG_RUNTIME', 'SSH_', 'GNOME', 'KDE'))}
        env.pop('WARP_HONOR_PS1', None)
        env['HOME'] = str(home); env['CODEX_HOME'] = str(codex_home)
        env['XDG_CONFIG_HOME'] = str(home / '.config'); env['XDG_DATA_HOME'] = str(home / '.local/share')
        env['XDG_STATE_HOME'] = str(home / '.local/state')
        self.p = subprocess.Popen([*prefix, 'codex', *extra, 'app-server'], cwd=cwd, env=env, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
        self.n = 0
        self.notes = []
        r = self.call('initialize', {'clientInfo': {'name': 'wyss-f1b', 'version': '0'}})
        self.send({'method': 'initialized'})
        self.init = r
    def send(self, o):
        self.p.stdin.write(json.dumps(o) + '\n'); self.p.stdin.flush()
    def call(self, method, params=None, timeout=20):
        self.n += 1; i = self.n
        self.send({'id': i, 'method': method, 'params': params if params is not None else {}})
        end = time.time() + timeout
        while time.time() < end:
            r, _, _ = select.select([self.p.stdout], [], [], 0.2)
            if not r: continue
            line = self.p.stdout.readline()
            if not line: break
            try: m = json.loads(line)
            except Exception: continue
            if m.get('id') == i: return m.get('result', {'error': m.get('error')})
            self.notes.append(m)
        return {'timeout': True}
    def close(self):
        try: self.p.stdin.close()
        except Exception: pass
        try: self.p.wait(timeout=5)
        except Exception: self.p.kill()

def sandbox():
    root = pathlib.Path(tempfile.mkdtemp(prefix='f1b-'))
    (root / 'home').mkdir()
    return root

def hooks_of(res):
    out = []
    for d in res.get('data', []):
        for h in d.get('hooks', []):
            out.append(h)
    return out
