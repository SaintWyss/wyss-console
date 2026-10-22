import gi,json,pathlib,sys,time
gi.require_version('Atspi','2.0')
from gi.repository import Atspi
r=pathlib.Path(pathlib.Path('/tmp/wyss-f8-visual-root').read_text());pid=json.loads((r/'pids.json').read_text())['gui']
d=Atspi.get_desktop(0);apps=[]
for i in range(d.get_child_count()):
 a=d.get_child_at_index(i)
 try:
  if a.get_process_id()==pid:apps.append(a)
 except Exception:pass
nodes=[]
def visit(a,path,depth=0):
 if not a or depth>30 or len(nodes)>1500:return
 try:
  role=a.get_role_name();name=a.get_name();states=a.get_state_set()
  showing=states.contains(Atspi.StateType.SHOWING)
  text=''
  if role not in ['password text']:
   try:text=Atspi.Text.get_text(a,0,-1)[:200]
   except Exception:pass
  nodes.append((path,a,role,name,showing,text))
  for i in range(a.get_child_count()):visit(a.get_child_at_index(i),path+'.'+str(i),depth+1)
 except Exception:pass
for i,a in enumerate(apps):visit(a,str(i))
if sys.argv[1:] and sys.argv[1]=='info':
 for n in nodes:
  if sys.argv[2] in n[3]:print(n[0],n[2],repr(n[3]),'states',n[1].get_state_set().get_states())
elif sys.argv[1:] and sys.argv[1]=='end':
 matches=[n for n in nodes if n[0]==sys.argv[2]];assert len(matches)==1
 a=matches[0][1];n=Atspi.Text.get_character_count(a);print('last character',n,Atspi.Text.scroll_substring_to(a,n-1,n,Atspi.ScrollType.BOTTOM_EDGE))
elif sys.argv[1:] and sys.argv[1]=='scroll':
 matches=[n for n in nodes if n[0]==sys.argv[2] or n[3]==sys.argv[2]];assert len(matches)==1
 print(Atspi.Component.scroll_to(matches[0][1],Atspi.ScrollType.TOP_EDGE))
elif sys.argv[1:] and sys.argv[1] in ['click','clickpath']:
 matches=[n for n in nodes if (n[0] if sys.argv[1]=='clickpath' else n[3])==sys.argv[2] and n[4]]
 if len(matches)!=1:raise RuntimeError('expected one visible target: '+str([(n[0],n[2],n[3]) for n in matches]))
 a=matches[0][1];iface=a.get_action_iface();print('actions',[iface.get_action_name(i) for i in range(iface.get_n_actions())]);print('result',iface.do_action(int(sys.argv[3]) if len(sys.argv)>3 else 0))
elif sys.argv[1:] and sys.argv[1]=='focus':
 matches=[n for n in nodes if n[0]==sys.argv[2] or n[3]==sys.argv[2]];assert len(matches)==1
 print(Atspi.Component.grab_focus(matches[0][1]))
elif sys.argv[1:] and sys.argv[1]=='set':
 matches=[n for n in nodes if n[0]==sys.argv[2]];assert len(matches)==1
 print(matches[0][1].get_editable_text_iface().set_text_contents(sys.argv[3]))
else:
 print('own accessible applications:',len(apps),'PID',pid)
 for path,a,role,name,showing,text in nodes:
  if showing:print(path,role,repr(name[:140]),repr(text))
