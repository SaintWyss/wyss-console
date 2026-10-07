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
assert sys.argv[1:] == ['app-server']
with (root / 'pids').open('a') as record:
    record.write(str(os.getpid()) + '\n')


def emit(value):
    print(json.dumps(value), flush=True)


def notification():
    emit({'method': 'future/notice', 'params': {'fixture': 'x' * 4096}})


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
        emit({'method': 'future/notice', 'params': {}})
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
                responses += 1
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

    elif method in ('thread/start', 'thread/resume'):
        assert initialized
        params = message['params']
        if method == 'thread/start':
            assert set(params) == {'cwd', 'ephemeral'} and params['ephemeral'] is False
            thread_id = 'opaque-' + str(len((root / 'methods').read_text().splitlines()))
        else:
            assert set(params) == {'cwd', 'threadId', 'excludeTurns'} and params['excludeTurns'] is True
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
                  'approvalsReviewer': 'user', 'sandbox': {'type': 'read-only'}}
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
