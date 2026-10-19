import sys, json, os, pathlib; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h import *
res = {}
def mk(hook=True, trust_proj=None):
    root = sandbox(); home = root / 'home'; user = home / '.codex'; user.mkdir()
    proj = root / 'proj'; (proj / '.codex').mkdir(parents=True)
    if hook:
        (user / 'hooks.json').write_text(json.dumps({"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "/bin/true"}]}]}}))
    (proj / '.codex' / 'hooks.json').write_text(json.dumps({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "/bin/true"}]}]}}))
    return root, home, user, proj
def summ(r):
    return [(h['source'], h['eventName'], h['enabled'], h['trustStatus']) for h in hooks_of(r)]
def errs(r):
    return [(d['errors'], d['warnings']) for d in r.get('data', [])]

# E1
root, home, user, proj = mk()
a = App(home, user, proj); c = summ(a.call('hooks/list', {'cwds': [str(proj)]})); a.close()
priv = home / 'private'; priv.mkdir()
a = App(home, priv, proj); t = summ(a.call('hooks/list', {'cwds': [str(proj)]})); a.close()
res['E1'] = {'control_user_home': c, 'private_home': t, 'pass': len(t) == 0 and len(c) >= 1}

# E2: system policy visible; engine pinned on
a = App(home, priv, proj)
req = a.call('configRequirements/read'); cfg = a.call('config/read', {'includeLayers': False})
hl = a.call('hooks/list', {'cwds': [str(proj)]})
a.close()
a = App(home, priv, proj, extra=['-c', 'features.hooks=false'])
cfg2 = a.call('config/read', {'includeLayers': False}); a.close()
feat = lambda c: (c.get('config') or {}).get('features', {}).get('hooks') if isinstance(c, dict) else None
res['E2'] = {'requirements': req.get('requirements', req), 'private_hooks': summ(hl), 'features.hooks_effective': feat(cfg),
             'features.hooks_effective_with_override_false': feat(cfg2), 'enabled_definitions_seen_by_check': sum(1 for h in hooks_of(hl) if h['enabled'])}

# E4: trust
root, home, user, proj = mk(hook=False)
(user / 'config.toml').write_text(f'[projects."{proj}"]\ntrust_level = "trusted"\n')
a = App(home, user, proj); user_home = summ(a.call('hooks/list', {'cwds': [str(proj)]})); a.close()
priv = home / 'private'; priv.mkdir()
a = App(home, priv, proj); private = summ(a.call('hooks/list', {'cwds': [str(proj)]})); a.close()
(priv / 'config.toml').write_text(f'[projects."{proj}"]\ntrust_level = "trusted"\n')
a = App(home, priv, proj); control = summ(a.call('hooks/list', {'cwds': [str(proj)]})); a.close()
res['E4'] = {'trusted_in_user_home': user_home, 'private_home_untrusted': private, 'control_trusted_in_private': control}

# E3: credential through a link; local API-key login writes auth.json
root, home, user, proj = mk(hook=False)
real = root / 'user-auth.json'; real.write_text('{}')
priv = home / 'private'; priv.mkdir(); (priv / 'auth.json').symlink_to(real)
a = App(home, priv, proj, extra=['-c', 'cli_auth_credentials_store="file"'])
login = a.call('account/login/start', {'type': 'apiKey', 'apiKey': 'sk-test-SYNTHETIC-f1b-0000'})
a.close()
link = (priv / 'auth.json').is_symlink()
res['E3'] = {'login_reply': {k: (v if k != 'apiKey' else '...') for k, v in login.items()} if isinstance(login, dict) else login,
             'link_survived': link, 'target_size_after': real.stat().st_size,
             'target_has_synthetic_key': 'SYNTHETIC' in real.read_text() if real.exists() else None,
             'files_in_private_home': sorted(p.name for p in priv.iterdir())}

# E7: where threads live
root, home, user, proj = mk(hook=False)
priv = home / 'private'; priv.mkdir()
a = App(home, priv, proj)
t = a.call('thread/start', {'cwd': str(proj)})
tid = (t.get('thread') or {}).get('id')
a.close()
files = sorted(str(p.relative_to(priv)) for p in priv.rglob('*') if p.is_file())
priv2 = home / 'private2'; priv2.mkdir()
a = App(home, priv2, proj); r2 = a.call('thread/resume', {'threadId': tid}) if tid else None; a.close()
a = App(home, priv, proj); r3 = a.call('thread/resume', {'threadId': tid}) if tid else None; a.close()
res['E7'] = {'thread_started': bool(tid), 'files_created_under_codex_home': files[:20],
             'resume_from_other_home': 'error' if (r2 or {}).get('error') or (r2 or {}).get('timeout') else 'ok',
             'resume_from_same_home': 'error' if (r3 or {}).get('error') or (r3 or {}).get('timeout') else 'ok',
             'other_home_reply': json.dumps(r2)[:200] if r2 else None, 'same_home_reply': json.dumps(r3)[:120] if r3 else None}
print(json.dumps(res, indent=1, default=str))
