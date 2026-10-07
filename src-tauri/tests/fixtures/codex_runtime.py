#!/usr/bin/python3
"""Deterministic zero-turn App Server fixture; records method names, never frames."""
import json
import os
import sys
import time
from pathlib import Path

root = Path(os.environ['WYSS_RUNTIME_FIXTURE'])
scenario = os.environ.get('WYSS_RUNTIME_SCENARIO', 'happy')
if sys.argv[1:] == ['--version']:
    print('codex-cli 0.159.0')
    sys.exit(0)
assert sys.argv[-1] == 'app-server'
argv = sys.argv[1:-1]
assert len(argv) % 2 == 0 and all(argv[i] == '-c' for i in range(0, len(argv), 2))
flags = dict(a.split('=', 1) for a in argv[1::2])
assert len(flags) == len(argv) // 2
assert not argv or flags.get('features.hooks') == 'false' and flags.get('features.shell_tool') == 'false'
assert not argv or flags.get('features.unified_exec') == 'false' and flags.get('web_search') == '"disabled"'

with (root / 'pids').open('a') as record:
    record.write(str(os.getpid()) + '\n')


def emit(value):
    print(json.dumps(value), flush=True)


def notification():
    emit({'method': 'remoteControl/status/changed', 'params': {'fixture': 'x' * 4096}})


threads_file = root / "threads.json"
threads = json.loads(threads_file.read_text()) if threads_file.exists() else {}
initialized = False
initialize_id = None
for line in sys.stdin:
    message = json.loads(line)
    method = message.get('method')
    with (root / 'methods').open('a') as record:
        record.write((method or 'RESPONSE') + '\n')
    if method == 'initialize':
        assert message['params']['capabilities']['experimentalApi'] is False
        initialize_id = message['id']
        if scenario == 'timeout':
            continue
        emit({'method': 'remoteControl/status/changed', 'params': {}})
        emit({'id': message['id'], 'result': {
            'userAgent': 'fixture', 'codexHome': '/synthetic',
            'platformFamily': 'unix', 'platformOs': 'linux'}})
    elif method == 'initialized':
        initialized = True
    elif method == 'model/list':
        assert initialized
        assert message['params'] == {'cursor': None, 'limit': 50, 'includeHidden': False}
        if scenario == 'catalog_error':
            emit({'id': message['id'], 'error': {'code': -32603, 'message': 'synthetic private diagnostic'}})
            continue
        count = 51 if scenario == 'too_many_models' else 45
        models = [{'id': f'model-{index}', 'model': f'model-{index}',
                   'displayName': 'Synthetic model', 'description': 'Fixture',
                   'hidden': False, 'isDefault': index == 0,
                   'defaultReasoningEffort': 'medium',
                   'supportedReasoningEfforts': [{'reasoningEffort': 'medium', 'description': 'Fixture'}]}
                  for index in range(count)]
        emit({'id': message['id'], 'result': {'data': models, 'nextCursor': None}})
        if scenario == 'malformed':
            print('{not-json}', flush=True)
        elif scenario == 'oversized':
            print('x' * (262144 + 1), flush=True)
        elif scenario == 'invalid_notification':
            emit({'method': 'thread/started', 'params': {}})
        elif scenario == 'duplicate':
            emit({'id': initialize_id, 'result': {}})
        elif scenario == 'server_request':
            emit({'id': 'unanswered', 'method': 'future/action', 'params': {}})
            responses = 0
            for possible_response in sys.stdin:
                responses += int("method" not in json.loads(possible_response))
            (root / 'responses').write_text(str(responses))
            break
        elif scenario == 'exit':
            time.sleep(0.1)
            sys.exit(7)
        elif scenario == 'flood':
            # Pipe capacity is far below either burst: both drains must keep running.
            for index in range(512):
                os.write(2, b'x' * 4096)
                notification()
            (root / 'drained').touch()

    elif method == 'config/read':
        assert message['params']['includeLayers'] is False
        counter=root/'config-count'
        calls=int(counter.read_text()) if counter.exists() else 0
        counter.write_text(str(calls+1))
        names=['fixture_mcp']
        if scenario=='mcp_drift' and calls>=1:names.append('new_mcp')
        servers = {n: {'enabled': flags.get('mcp_servers.' + n + '.enabled') != 'false'} for n in names}
        features = {k.split('.',1)[1]: False for k in flags if k.startswith('features.')}
        emit({'id':message['id'],'result':{'config':{'mcp_servers':servers,'features':features,'cloud':{'skills':{'enabled':False}},'skills':{'include_instructions':False},'web_search':'disabled'},'layers':None}})
    elif method == 'hooks/list':
        hooks=[{'enabled':True}] if scenario=='hooks_enabled' or (root/'restore-hooks').exists() else []
        emit({'id':message['id'],'result':{'data':[{'hooks':hooks}]}})
    elif method in ('thread/start', 'thread/resume'):
        assert initialized
        params = message['params']
        if method == 'thread/start':
            assert set(params) == {'cwd', 'ephemeral'} and params['ephemeral'] is False
            thread_id = 'opaque-' + str(len((root / 'methods').read_text().splitlines()))
        else:
            assert set(params) == {'cwd', 'threadId', 'excludeTurns', 'config', 'sandbox', 'approvalPolicy'} and params['excludeTurns'] is True
            assert params['sandbox'] == 'read-only' and params['approvalPolicy'] == 'never'
            assert params['config']['features.hooks'] is False and params['config']['mcp_servers']['fixture_mcp']['enabled'] is False
            thread_id = params['threadId']
        if method == 'thread/resume':
            assert threads[thread_id]['durable'] and not threads[thread_id]['archived']
        assert Path(params['cwd']).is_absolute()
        (root / 'dispatched').touch()
        if scenario == 'thread_timeout':
            continue
        if scenario == 'thread_hold':
            while not (root / 'release').exists():
                time.sleep(0.005)
        if scenario == 'thread_error' or (scenario == 'resume_error' and method == 'thread/resume' and (root / 'methods').read_text().splitlines().count('thread/resume') > 1):
            emit({'id': message['id'], 'error': {'code': -32603, 'message': 'private provider error'}})
            continue
        if scenario == 'thread_server_request':
            emit({'id': 'approval-not-answered', 'method': 'future/action', 'params': {}})
            (root / 'responses').write_text(str(sum(1 for _ in sys.stdin)))
            break
        thread = {'id': thread_id, 'cwd': params['cwd'], 'cliVersion': '0.159.0',
                  'modelProvider': 'synthetic', 'preview': '', 'sessionId': 'synthetic',
                  'createdAt': 0, 'updatedAt': 0, 'ephemeral': False, 'projectId': None,
                  'source': 'appServer', 'status': {'type': 'idle'}, 'turns': []}
        if method == 'thread/start':
            threads[thread_id] = {'thread': dict(thread), 'durable': False, 'archived': False}
        if scenario == 'thread_wrong_id' and method == 'thread/resume':
            thread['id'] = 'substituted-provider-identity'
        if scenario == 'thread_wrong_cwd':
            thread['cwd'] = '/'
        if scenario == 'thread_malformed':
            del thread['id']
        result = {'thread': thread, 'cwd': params['cwd'], 'model': 'synthetic',
                  'modelProvider': 'synthetic', 'approvalPolicy': 'never',
                  'approvalsReviewer': 'user', 'sandbox': {'type': 'readOnly', 'networkAccess': False}}
        if scenario == 'thread_lock_response':
            import sqlite3
            lock = sqlite3.connect(Path(os.environ['XDG_STATE_HOME']) / 'wyss-console/control.sqlite3')
            lock.execute('BEGIN IMMEDIATE')
        emit({'id': 'wrong-response-id' if scenario == 'thread_wrong_response' else message['id'], 'result': result})
        (root / 'responded').touch()

    elif method in ('thread/archive', 'thread/unarchive'):
        params=message['params']
        assert set(params)=={'threadId'} and params['threadId'] in threads
        key='archive' if method=='thread/archive' else 'unarchive'
        tid=params['threadId']; entry=threads[tid]
        (root / (key+'-written')).touch()
        if scenario == key+'_hold':
            while not (root / 'release').exists(): time.sleep(.005)
        if scenario == key+'_timeout': continue
        if scenario == key+'_error':
            emit({'id':message['id'],'error':{'code':-32603,'message':'private persistence error'}})
            continue
        if scenario == key+'_server_request':
            emit({'id':'unanswered','method':'future/action','params':{}})
            (root/'responses').write_text(str(sum(1 for _ in sys.stdin)))
            break
        if key=='archive':
            entry['durable']=True;entry['archived']=True;result={}
        else:
            assert entry['durable'] and entry['archived']
            entry['archived']=False;result={'thread':dict(entry['thread'])}
            if scenario=='unarchive_wrong_id': result['thread']['id']='substituted-id'
            if scenario=='unarchive_wrong_cwd': result['thread']['cwd']='/'
            if scenario=='unarchive_relative_cwd': result['thread']['cwd']='.'
            if scenario=='unarchive_malformed': del result['thread']['id']
        threads_file.write_text(json.dumps(threads))
        if scenario == key+'_lock_response':
            import sqlite3
            lock=sqlite3.connect(Path(os.environ['XDG_STATE_HOME'])/'wyss-console/control.sqlite3')
            lock.execute('BEGIN IMMEDIATE')
        if scenario==key+'_extra': result['unexpected']='forbidden'
        emit({'method':'thread/archived' if key=='archive' else 'thread/unarchived','params':{'threadId':tid}})
        emit({'id':'unrelated-id' if scenario==key+'_wrong_response' else message['id'],'result':result})
        (root/(key+'-responded')).touch()

    elif method == 'turn/start':
        p=message['params'];assert set(p)=={'threadId','input'}
        assert len(p['input'])==1 and set(p['input'][0])=={'type','text','textElements'}
        assert p['input'][0]['type']=='text' and p['input'][0]['textElements']==[]
        tid=p['threadId'];turn_id='provider-turn-'+str(len((root/'methods').read_text().splitlines()))
        current_turn={'id':turn_id,'items':[],'status':'inProgress','error':None}
        (root/'turn-written').touch()
        if scenario=='turn_timeout':continue
        if scenario=='turn_hold':
            while not (root/'release').exists():time.sleep(.005)
        def notice(method, **payload):emit({'method':method,'params':{'threadId':tid,'turnId':turn_id,**payload}})
        if scenario=='turn_response_first':
            import threading
            def deferred_start():
                while not (root/'release-started').exists():time.sleep(.005)
                emit({'method':'turn/started','params':{'threadId':tid,'turn':current_turn}})
                (root/'provider-started').touch()
            emit({'id':message['id'],'result':{'turn':current_turn}})
            threading.Thread(target=deferred_start,daemon=True).start()
            continue
        emit({'method':'turn/started','params':{'threadId':tid,'turn':current_turn}})
        notice('item/agentMessage/delta',itemId='agent-item',delta='ASSISTANT_SENTINEL')
        emit({'id':message['id'],'result':{'turn':current_turn}})
        if scenario.startswith('isolation_'):
            kind=scenario[len('isolation_'):]
            if kind=='hook':notice('hook/started',run={'eventName':'sessionStart','handlerType':'command'})
            elif kind in ['approval','permission','input']:emit({'id':'unanswered','method':{'approval':'item/commandExecution/requestApproval','permission':'item/permissions/requestApproval','input':'item/tool/requestUserInput'}[kind],'params':{}})
            elif kind=='diff':notice('turn/diff/updated',diff='forbidden')
            else:notice('item/started',item={'type':kind,'id':'external-item'})
            (root/'responses').write_text(str(sum(1 for line in sys.stdin if '"method"' not in line)))
            break
        if scenario in ['turn_long','interrupt_timeout']:continue
        notice('item/reasoning/summaryTextDelta',itemId='reasoning-item',summaryIndex=0,delta='SUMMARY_SENTINEL')
        notice('item/reasoning/textDelta',itemId='reasoning-item',contentIndex=0,delta='RAW_PRIVATE_REASONING_SENTINEL')
        notice('item/completed',completedAtMs=0,item={'type':'agentMessage','id':'agent-item','text':'ASSISTANT_SENTINEL','phase':'final_answer'})
        terminal=dict(current_turn,status='completed')
        emit({'method':'turn/completed','params':{'threadId':tid,'turn':terminal}})
        (root/'turn-completed').touch()
    elif method == 'turn/interrupt':
        if scenario=='turn_response_first' and not (root/'provider-started').exists():
            emit({'id':message['id'],'error':{'code':-32600,'message':'no active turn to interrupt'}})
            continue
        assert set(message['params'])=={'threadId','turnId'}
        assert message['params']=={'threadId':tid,'turnId':turn_id}
        emit({'id':message['id'],'result':{}})
        if scenario!='interrupt_timeout':
            emit({'method':'turn/completed','params':{'threadId':tid,'turn':dict(current_turn,status='interrupted')}})
