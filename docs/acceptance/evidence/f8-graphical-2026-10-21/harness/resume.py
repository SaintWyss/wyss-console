import os,json,pathlib,subprocess,time,tempfile,shutil,signal
repo=pathlib.Path.cwd();r=pathlib.Path(pathlib.Path('/tmp/wyss-f8-visual-root').read_text());(r/'stop').unlink(missing_ok=True)
installed=pathlib.Path.home()/'.local/opt/wyss-console';gui=installed/'bin/wyss-console';authority=installed/'libexec/wyss-console/wyss-control'
e=json.loads((r/'env.json').read_text());e['GDK_BACKEND']='x11';e['DISPLAY']=os.environ['DISPLAY']
(r/'env.json').write_text(json.dumps(e));a=g=None

def start_a():
 return subprocess.Popen([str(authority),'--protocol','2','--runtime-root',str(r/'ar'),'--state-root',str(r/'as'),*(['--create'] if not (r/'as/wyss-console/authority.sqlite3').exists() else []),'--codex',str(r/'bin/codex'),'--codex-home',str(r/'u'),'--allow-credential'],env=e,stdout=open(r/'authority.log','a'),stderr=subprocess.STDOUT)
def start_g():
 return subprocess.Popen([str(gui),'--f8-authority',str(r/'ar')],env=e,stdout=open(r/'gui.log','a'),stderr=subprocess.STDOUT)
def save():
 (r/'pids.json').write_text(json.dumps({'authority':a.pid if a else None,'gui':g.pid if g else None}))
try:
 a=start_a()
 for _ in range(200):
  if (r/'ar/wyss-console/control.sock').exists():break
  if a.poll() is not None:raise RuntimeError('authority startup failed')
  time.sleep(.05)
 g=start_g();save();print('ROOT',r,'OWNED',a.pid,g.pid,flush=True)
 while not (r/'stop').exists():
  c=r/'command.json'
  if c.exists():
   v=json.loads(c.read_text());c.unlink();action=v['action']
   if action=='crash_authority':a.kill();a.wait();a=None
   elif action=='start_authority':a=start_a()
   elif action in ['restart_gui','restart_gui_x11','restart_gui_wayland']:
    if action=='restart_gui_x11':
     e['GDK_BACKEND']='x11'
     e['XAUTHORITY']=os.environ['XAUTHORITY']
    if action=='restart_gui_wayland':e['GDK_BACKEND']='wayland'
    g.terminate();g.wait(timeout=15);g=start_g()
   save();print('command complete',action,flush=True)
  time.sleep(.2)
finally:
 if g and g.poll() is None:g.terminate();g.wait(timeout=15)
 subprocess.run([str(gui),'--wyss-host-stop'],env=e,timeout=15,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 if a and a.poll() is None:a.terminate();a.wait(timeout=15)
 print('owned GUI, host and authority stopped',flush=True)
