import pathlib,json,subprocess,time
r=pathlib.Path('/tmp/wyss-f8v-sb_mv5xg'); pid=json.loads((r/'pids.json').read_text())['gui']; p=pathlib.Path(__file__).with_suffix('.js')
p.write_text('for (const w of workspace.windowList()) { if (w.pid === '+str(pid)+' && w.caption === "Wyss Console") { workspace.activeWindow = w; callDBus("org.saintwyss.F8VisualProbe","/Probe","org.saintwyss.F8Visual","Window",JSON.stringify({pid:w.pid,id:w.internalId.toString(),active:w.active})); }}')
s=subprocess.check_output(['qdbus6','org.kde.KWin','/Scripting','org.kde.kwin.Scripting.loadScript',str(p.resolve()),'wyss-f8-visual-focus'],text=True).strip()
try:
 subprocess.run(['qdbus6','org.kde.KWin','/Scripting/Script'+s,'org.kde.kwin.Script.run'],check=True);time.sleep(.2)
finally:subprocess.run(['qdbus6','org.kde.KWin','/Scripting','org.kde.kwin.Scripting.unloadScript','wyss-f8-visual-focus'],check=True)
v=json.loads((r/'windows.jsonl').read_text().splitlines()[-1]);assert v['pid']==pid and v.get('active'),v
print(v)
