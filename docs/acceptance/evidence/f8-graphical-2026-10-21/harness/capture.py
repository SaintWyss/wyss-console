import gi,json,pathlib,os,threading,sys,cairo
from gi.repository import Gio,GLib
r=pathlib.Path(pathlib.Path('/tmp/wyss-f8-visual-root').read_text());pid=json.loads((r/'pids.json').read_text())['gui']
w=json.loads((r/'windows.jsonl').read_text().splitlines()[-1]);assert w['pid']==pid
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
info=bus.call_sync('org.kde.KWin','/KWin','org.kde.KWin','getWindowInfo',GLib.Variant('(s)',(w['id'],)),None,Gio.DBusCallFlags.NONE,3000,None).unpack()[0]
assert info['pid']==pid,info
read,write=os.pipe();chunks=[]
def drain():
 while True:
  b=os.read(read,65536)
  if not b:break
  chunks.append(b)
 os.close(read)
t=threading.Thread(target=drain);t.start();fds=Gio.UnixFDList.new();index=fds.append(write);os.close(write)
try:
 result,_=bus.call_with_unix_fd_list_sync('org.kde.KWin','/org/kde/KWin/ScreenShot2','org.kde.KWin.ScreenShot2','CaptureWindow',GLib.Variant('(sa{sv}h)',(w['id'],{'include-decoration':GLib.Variant('b',True)},index)),None,Gio.DBusCallFlags.NONE,10000,fds,None)
 meta=result.unpack()[0];del fds;t.join(timeout=10);raw=bytearray(b''.join(chunks));print('metadata',meta,'bytes',len(raw))
 surface=cairo.ImageSurface.create_for_data(raw,cairo.FORMAT_ARGB32,meta['width'],meta['height'],meta['stride']);surface.write_to_png(str(r/sys.argv[1]));print(r/sys.argv[1])
except Exception as e:
 print(type(e).__name__,str(e));del fds;t.join(timeout=1);sys.exit(1)
