#!/usr/bin/python3
"""Deterministic read-only App Server fixture; records method names, never frames."""
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
