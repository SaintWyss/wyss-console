import gi,json,pathlib
from gi.repository import Gio,GLib
r=pathlib.Path(pathlib.Path('/tmp/wyss-f8-visual-root').read_text())
xml='<node><interface name="org.saintwyss.F8Visual"><method name="Window"><arg type="s" direction="in"/></method></interface></node>'
info=Gio.DBusNodeInfo.new_for_xml(xml);bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
def call(conn,sender,path,interface,method,params,invocation):
 value=json.loads(params.unpack()[0]);pid=json.loads((r/'pids.json').read_text())['gui']
 if value['pid']!=pid:invocation.return_dbus_error('org.saintwyss.Refused','wrong pid');return
 with (r/'windows.jsonl').open('a') as f:f.write(json.dumps(value)+'\n')
 invocation.return_value(GLib.Variant('()',()))
bus.register_object('/Probe',info.interfaces[0],call,None,None)
Gio.bus_own_name_on_connection(bus,'org.saintwyss.F8VisualProbe',Gio.BusNameOwnerFlags.NONE,None,None)
print('window probe ready',flush=True);GLib.MainLoop().run()
