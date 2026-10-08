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
# The local-tools generation differs from inference by exactly this one enabled feature.
local = flags.get('features.shell_tool') == 'true'
assert not argv or flags.get('features.hooks') == 'false' and flags.get('features.shell_tool') in ('false', 'true')
assert not local or flags.get('features.unified_exec_tty') == 'false'
WORKSPACE_ONLY = {'sandbox_workspace_write.exclude_slash_tmp': 'true', 'sandbox_workspace_write.exclude_tmpdir_env_var': 'true',
                  'sandbox_workspace_write.network_access': 'false'}
assert {k: v for k, v in flags.items() if k.startswith('sandbox_workspace_write.')} == (WORKSPACE_ONLY if local else {})
assert local or 'features.unified_exec_tty' not in flags
assert not argv or flags.get('features.unified_exec') == 'false' and flags.get('web_search') == '"disabled"'
assert not argv or flags.get('agents.enabled') == 'false'
assert not argv or flags.get('features.code_mode_host') == 'false' and flags.get('features.sleep_tool') == 'false'
catalog = flags.get('model_catalog_json', '').strip('"') or None
if catalog:
    # The authority presentation is in place before the thread-capable generation starts.
    presented = json.loads(Path(catalog).read_text())
    assert presented['models'] and all(
        m['tool_mode'] == 'direct' and m['experimental_supported_tools'] == []
        and m['multi_agent_version'] is None
        and (m['apply_patch_tool_type'] == 'freeform' if local else m['apply_patch_tool_type'] is None)
        for m in presented['models'])
    assert Path(catalog).stat().st_mode & 0o777 == 0o600
    (root / 'catalog-launches').open('a').write('1\n')

with (root / 'pids').open('a') as record:
    record.write(str(os.getpid()) + '\n')
if local:
    # What this process can actually see from inside its namespace. Never credential bytes
    # beyond the fixture marker the test wrote itself.
    home = Path(os.environ['CODEX_HOME'])
    user = Path(os.environ['WYSS_FIXTURE_USER_HOME'])
    layers = json.loads(os.environ.get('WYSS_FIXTURE_LAYERS', '[]'))
    (root / 'local.json').write_text(json.dumps({
        'codex_home': str(home),
        'requirements': Path('/etc/codex/requirements.toml').read_text(),
        'managed_entries': sorted(os.listdir('/etc/codex')),
        'user_home_entries': sorted(os.listdir(user)),
        'credential': (home / 'auth.json').read_text(),
        'credential_writable': os.access(home / 'auth.json', os.W_OK),
        'inherited': sorted(k for k in os.environ if k.startswith(('CODEX_', 'OPENAI'))),
        'layers': {p: sorted(os.listdir(p)) for p in layers},
        'layers_writable': [p for p in layers if os.access(p, os.W_OK)],
    }))
    with (root / 'local-launches').open('a') as record:
        record.write(('final' if catalog else 'discovery') + '\n')


def emit(value):
    print(json.dumps(value), flush=True)


def notification():
    emit({'method': 'remoteControl/status/changed', 'params': {'fixture': 'x' * 4096}})


threads_file = root / "threads.json"
threads = json.loads(threads_file.read_text()) if threads_file.exists() else {}
initialized = False
initialize_id = None
# The one approval request this provider is waiting on, and what it would act on.
waiting = None


def tally(name):
    with (root / name).open('a') as record:
        record.write('1\n')


def command_item(workspace, status, **extra):
    return {'type': 'commandExecution', 'id': 'call-1', 'command': 'printf WYSS_FIXTURE_COMMAND', 'cwd': workspace,
            'commandActions': [{'type': 'unknown', 'command': 'printf WYSS_FIXTURE_COMMAND'}], 'status': status,
            'source': 'unifiedExecStartup', 'processId': '1', **extra}


def file_item(workspace, status):
    return {'type': 'fileChange', 'id': 'patch-1', 'status': status, 'changes': [
        {'path': workspace + '/wyss-fixture-probe.txt', 'kind': {'type': 'add'}, 'diff': '+WYSS_FIXTURE_PATCH\n'}]}


def resolve(tid, decision):
    """What the real runtime does once its one request is answered."""
    global waiting
    kind, workspace, turn = waiting['kind'], waiting['workspace'], waiting['turn']
    request_id = waiting['id']
    waiting = None
    item = command_item if kind == 'command' else file_item
    emit({'method': 'serverRequest/resolved', 'params': {'threadId': tid, 'requestId': request_id}})
    finished = lambda status, **extra: emit({'method': 'item/completed', 'params': {
        'threadId': tid, 'turnId': turn['id'], 'completedAtMs': 2, 'item': item(workspace, status, **extra)}})
    if decision != 'accept':
        finished('declined')
        # `decline` keeps the turn; `cancel` interrupts it.
        emit({'method': 'turn/completed', 'params': {'threadId': tid, 'turn': dict(
            turn, status='completed' if decision == 'decline' else 'interrupted')}})
        return
    if scenario.endswith('_slow'):
        # Accepted and running: the outcome is never reported.
        tally('executions')
        return
    if kind == 'command':
        tally('executions')
        emit({'method': 'item/commandExecution/outputDelta', 'params': {
            'threadId': tid, 'turnId': turn['id'], 'itemId': 'call-1', 'delta': 'WYSS_FIXTURE_OUTPUT'}})
        finished('completed', exitCode=0, aggregatedOutput='WYSS_FIXTURE_OUTPUT')
    else:
        Path(workspace + '/wyss-fixture-probe.txt').write_text('WYSS_FIXTURE_PATCH\n')
        tally('mutations')
        emit({'method': 'turn/diff/updated', 'params': {'threadId': tid, 'turnId': turn['id'], 'diff': '+WYSS_FIXTURE_PATCH\n'}})
        finished('completed')
    emit({'method': 'turn/completed', 'params': {'threadId': tid, 'turn': dict(turn, status='completed')}})


for line in sys.stdin:
    message = json.loads(line)
    method = message.get('method')
    with (root / 'methods').open('a') as record:
        record.write((method or 'RESPONSE') + '\n')
    if method is None:
        # Every response frame the authority ever writes, and the decision it carries.
        with (root / 'decisions').open('a') as record:
            record.write(json.dumps(message.get('result', {}).get('decision')) + '\n')
        if waiting and message.get('id') == waiting['id']:
            resolve(tid, message['result']['decision'])
        continue
    if method == 'initialize':
        assert message['params']['capabilities']['experimentalApi'] is False
        initialize_id = message['id']
        if scenario == 'timeout':
            continue
        emit({'method': 'remoteControl/status/changed', 'params': {}})
        emit({'id': message['id'], 'result': {
            'userAgent': 'fixture',
            'codexHome': os.environ['CODEX_HOME'] if local and scenario != 'local_wrong_home' else '/synthetic',
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
        if local:
            # The real runtime refreshes the backend catalog cache of ITS home here.
            (Path(os.environ['CODEX_HOME']) / 'models_cache.json').write_text(json.dumps({'models': [{
                'slug': 'fixture-model', 'tool_mode': 'code_mode_only', 'apply_patch_tool_type': 'freeform',
                'multi_agent_version': 'v2', 'experimental_supported_tools': ['clock']}]}))
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
        names=[] if local and scenario != 'local_mcp' else ['fixture_mcp']
        if scenario=='mcp_drift' and calls>=1:names.append('new_mcp')
        servers = {n: {'enabled': flags.get('mcp_servers.' + n + '.enabled') != 'false'} for n in names}
        features = {k.split('.',1)[1]: v == 'true' for k, v in flags.items() if k.startswith('features.')}
        emit({'id':message['id'],'result':{'config':{'mcp_servers':servers,'features':features,'cloud':{'skills':{'enabled':False}},'skills':{'include_instructions':False},'web_search':'disabled','agents':{'enabled':flags.get('agents.enabled') != 'false'},'model_catalog_json':catalog,
            **({'sandbox_workspace_write':{'writable_roots':[],'network_access':False,'exclude_tmpdir_env_var':True,'exclude_slash_tmp':scenario!='local_tmp_writable'}} if local else {})},'layers':None}})
    elif method == 'configRequirements/read':
        assert local and message['params'] == {}
        managed = Path('/etc/codex/requirements.toml').read_text()
        required = {'unified_exec': False} if 'unified_exec = false' in managed else {}
        if scenario == 'local_no_requirement':
            required = {'hooks': True}
        emit({'id': message['id'], 'result': {'requirements': {'featureRequirements': required, 'hooks': None}}})
    elif method == 'hooks/list':
        hooks=[{'enabled':True}] if scenario=='hooks_enabled' or (root/'restore-hooks').exists() else []
        emit({'id':message['id'],'result':{'data':[{'hooks':hooks}]}})
        if scenario.endswith('_deaf') and waiting:
            # The provider proves its profile for the Accept and is gone before reading it.
            os.close(0)
            time.sleep(0.3)
            os._exit(0)
    elif method in ('thread/start', 'thread/resume'):
        assert initialized
        params = message['params']
        posture = ('workspace-write', 'untrusted') if local else ('read-only', 'never')
        if method == 'thread/start' and not local:
            assert set(params) == {'cwd', 'ephemeral'} and params['ephemeral'] is False
            thread_id = 'opaque-' + str(len((root / 'methods').read_text().splitlines()))
        elif method == 'thread/start':
            assert set(params) == {'cwd', 'ephemeral', 'config', 'sandbox', 'approvalPolicy'} and params['ephemeral'] is False
            thread_id = 'opaque-' + str(len((root / 'methods').read_text().splitlines()))
        else:
            assert set(params) == {'cwd', 'threadId', 'excludeTurns', 'config', 'sandbox', 'approvalPolicy'} and params['excludeTurns'] is True
            thread_id = params['threadId']
        if 'config' in params:
            assert (params['sandbox'], params['approvalPolicy']) == posture
            assert params['config']['features.hooks'] is False and params['config']['features.unified_exec'] is False
            assert params['config']['features.shell_tool'] is local and params['config']['agents.enabled'] is False
            if local:
                projects = params['config']['projects']
                assert params['cwd'] in projects or len(projects) == 1
                assert projects and all(v == {'trust_level': 'untrusted'} for v in projects.values())
                assert params['config']['mcp_servers'] == {} and 'default_permissions' not in params['config']
                assert all(params['config'][k] is (v == 'true') for k, v in WORKSPACE_ONLY.items())
                (root / 'trust-roots').write_text(json.dumps(sorted(projects)))
            else:
                assert params['config']['mcp_servers']['fixture_mcp']['enabled'] is False
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
        if local:
            result.update(approvalPolicy='untrusted', activePermissionProfile=None,
                          sandbox={'type': 'workspaceWrite', 'writableRoots': [], 'networkAccess': False,
                                   'excludeTmpdirEnvVar': True, 'excludeSlashTmp': True})
            if scenario == 'local_weak_thread':
                result['approvalPolicy'] = 'never'
            if scenario == 'local_network_thread':
                result['sandbox']['networkAccess'] = True
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
        if scenario.startswith('approval_'):
            # The mediated surface of the local-tools generation, as 0.159.0 emits it:
            # the action is announced, then asked about, and only an answer makes it run.
            variant = scenario[len('approval_'):]
            workspace = threads[tid]['thread']['cwd']
            kind = 'file' if variant.startswith('file') else 'command'
            started = lambda item: emit({'method': 'item/started', 'params': {
                'threadId': tid, 'turnId': turn_id, 'startedAtMs': 1, 'item': item}})
            ask = {'threadId': tid, 'turnId': turn_id, 'itemId': 'call-1' if kind == 'command' else 'patch-1', 'startedAtMs': 1}
            if kind == 'command':
                ask.update(command='printf WYSS_FIXTURE_COMMAND', cwd=workspace, kind='command', environmentId='local',
                           commandActions=[{'type': 'unknown', 'command': 'printf WYSS_FIXTURE_COMMAND'}],
                           proposedExecpolicyAmendment=['printf', 'WYSS_FIXTURE_COMMAND'],
                           availableDecisions=['accept', {'acceptWithExecpolicyAmendment': {'execpolicy_amendment': ['printf']}}, 'cancel'])
            if variant == 'command_nocancel':
                ask['availableDecisions'] = ['accept']
            if variant == 'command_stdin':
                ask['kind'] = 'writeStdin'
            if variant == 'command_permissions':
                ask['additionalPermissions'] = {'network': {'enabled': True}}
            if variant == 'command_network':
                ask['networkApprovalContext'] = {'host': 'example.com', 'protocol': 'https'}
            if variant == 'file_grant':
                ask['grantRoot'] = '/'
            if variant == 'command_outside':
                ask['cwd'] = '/'
            if variant == 'command_remote':
                ask['environmentId'] = 'remote'
            request = {'id': 'approval-1', 'method': 'item/%s/requestApproval' % ('commandExecution' if kind == 'command' else 'fileChange'), 'params': ask}
            if variant == 'command_unasked':
                # The runtime ran a command on its own: nothing was ever asked.
                started(command_item(workspace, 'inProgress'))
                emit({'method': 'item/completed', 'params': {'threadId': tid, 'turnId': turn_id, 'completedAtMs': 2,
                                                              'item': command_item(workspace, 'completed', exitCode=0)}})
                continue
            if variant != 'command_outside':
                started(command_item(workspace, 'inProgress') if kind == 'command' else file_item(workspace, 'inProgress'))
            waiting = {'id': 'approval-1', 'kind': kind, 'workspace': workspace, 'turn': current_turn}
            emit(request)
            (root / 'asked').touch()
            if variant == 'command_double':
                emit(dict(request, id='approval-2'))
            if variant == 'command_early':
                # Output before any answer: an effect the authority never accepted.
                emit({'method': 'item/commandExecution/outputDelta', 'params': {
                    'threadId': tid, 'turnId': turn_id, 'itemId': 'call-1', 'delta': 'early'}})
            continue
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
        if waiting:
            # An interrupted turn withdraws the request it was waiting on.
            emit({'method': 'serverRequest/resolved', 'params': {'threadId': tid, 'requestId': waiting['id']}})
            waiting = None
        if scenario!='interrupt_timeout':
            emit({'method':'turn/completed','params':{'threadId':tid,'turn':dict(current_turn,status='interrupted')}})
