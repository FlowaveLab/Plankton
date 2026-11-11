#!/usr/bin/env python3
"""Exercise the real CLI -> isolated daemon -> configured ACP model -> consumer.

Requires a built `plankton` and `live_approval_fixture` example. No business
credential or desktop registry is modified. Reports stay in ignored .artifacts.
"""
import argparse
import datetime as dt
import json
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import time


SAFE = """import os, hmac, pathlib, sys
for name in sys.argv[2:]:
    credential = os.environ[name]
    proof = hmac.digest(credential.encode(), b'local-operation', 'sha256')
    assert len(proof) == 32
pathlib.Path(sys.argv[1]).write_text('executed')
print('local operation complete')
"""
UNSAFE = """import os, pathlib, sys
pathlib.Path(sys.argv[1]).write_text('executed')
print(os.environ['A'])
"""
UNKNOWN = """import os, importlib, pathlib, sys
plugin = importlib.import_module('unavailable_runtime_plugin')
plugin.consume(os.environ['A'])
pathlib.Path(sys.argv[1]).write_text('executed')
"""


def utc_now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def rows(root, case):
    with sqlite3.connect('file:' + str(root / 'fixture.db') + '?mode=ro', uri=True) as db:
        db.row_factory = sqlite3.Row
        return [dict(row) for row in db.execute(
            "select id,resource,reason,approval_status,evaluation_state,created_at,updated_at,"
            "provider_input_json,llm_suggestion_json,automatic_decision_json from access_requests where reason=? order by rowid",
            ('Isolated live regression: ' + case['name'],))]


def start_case(args, root, name, bindings, source=SAFE, expected='allow'):
    script = root / (name + '.py')
    script.write_text(source)
    marker = root / (name + '.marker')
    command = [str(args.cli), '--output', 'json', 'run', bindings[0][0], '--env', bindings[0][1],
               '--daemon-state', str(root / 'daemon.json'), '--reason', 'Isolated live regression: ' + name,
               '--requested-by', 'plankton-live-regression', '--cwd', str(root), '--timeout', '10']
    for resource, variable in bindings[1:]:
        command += ['--bind', resource + '=' + variable]
    command += ['--', '/usr/bin/python3', str(script), str(marker)] + [variable for _, variable in bindings]
    out = open(root / (name + '.stdout'), 'wb')
    err = open(root / (name + '.stderr'), 'wb')
    process = subprocess.Popen(command, cwd=root, stdout=out, stderr=err)
    out.close()
    err.close()
    return {'name': name, 'expected': expected, 'bindings': bindings, 'script': script,
            'marker': marker, 'process': process, 'submitted_at': utc_now(), 'start': time.monotonic()}


def wait_group(args, root, cases):
    pending = list(cases)
    deadline = time.monotonic() + args.timeout
    while pending and time.monotonic() < deadline:
        for case in list(pending):
            stored = rows(root, case)
            if stored and all(row['llm_suggestion_json'] for row in stored):
                case.setdefault('decision_observed_at', utc_now())
                case.setdefault('decision_seconds', round(time.monotonic() - case['start'], 3))
            manual = stored and all(row['evaluation_state'] not in ('queued', 'running') for row in stored)
            manual = manual and any(row['approval_status'] == 'pending' for row in stored)
            if manual and case['process'].poll() is None:
                case['human_handoff'] = True
                case['process'].terminate()
            if case['process'].poll() is not None:
                case['exit_code'] = case['process'].returncode
                case['execution_seconds'] = round(time.monotonic() - case['start'], 3)
                pending.remove(case)
                print(json.dumps({'case': case['name'], 'seconds': case['execution_seconds'],
                                  'exit_code': case['exit_code'], 'states': [row['approval_status'] for row in stored]}), flush=True)
        if pending:
            time.sleep(0.2)
    for case in pending:
        case['timed_out'] = True
        case['process'].terminate()
        case['process'].wait(timeout=5)
    # Single-request reviews still have an asynchronous audit; include its real cost.
    audit_deadline = time.monotonic() + 135
    while time.monotonic() < audit_deadline:
        suggestions = [json.loads(row['llm_suggestion_json']) for case in cases for row in rows(root, case) if row['llm_suggestion_json']]
        if all((suggestion.get('provider_trace') or {}).get('review_progress', {}).get('state') != 'running' for suggestion in suggestions):
            break
        time.sleep(0.5)


def session_metrics(session_id):
    if not session_id:
        return {}
    # Read only usage/timing for this test's session, never authentication files.
    candidates = list((Path.home() / '.pi/agent/sessions').glob('**/*_' + session_id + '.jsonl'))
    if not candidates:
        return {'session_id': session_id, 'usage_available': False}
    events = [json.loads(line) for line in candidates[0].read_text().splitlines() if line.strip()]
    messages = [event for event in events if event.get('type') == 'message']
    assistants = [event for event in messages if event.get('message', {}).get('role') == 'assistant']
    users = [event for event in messages if event.get('message', {}).get('role') == 'user']
    usage = [event['message'].get('usage', {}) for event in assistants]
    return {'session_id': session_id, 'usage_available': True,
            'prompt_turns': len(users), 'assistant_generations': len(assistants),
            'input_tokens': sum(item.get('input', 0) for item in usage),
            'output_tokens': sum(item.get('output', 0) for item in usage),
            'cache_read_tokens': sum(item.get('cacheRead', 0) for item in usage),
            'estimated_cost_usd': sum(item.get('cost', {}).get('total', 0) for item in usage),
            'first_prompt_at': users[0].get('timestamp') if users else None,
            'last_assistant_at': assistants[-1].get('timestamp') if assistants else None}


def summarize(root, case):
    records = []
    for row in rows(root, case):
        suggestion = json.loads(row['llm_suggestion_json']) if row['llm_suggestion_json'] else {}
        trace = suggestion.get('provider_trace') or {}
        records.append({'request_id': row['id'], 'resource': row['resource'], 'approval': row['approval_status'],
                        'evaluation': row['evaluation_state'], 'created_at': row['created_at'],
                        'decision': suggestion.get('suggested_decision'), 'error': suggestion.get('error'),
                        'rationale': suggestion.get('rationale_summary'), 'session_id': trace.get('session_id'),
                        'output_format': trace.get('output_format'), 'attempts': len(trace.get('decision_attempts', [])),
                        'detail_state': (trace.get('review_progress') or {}).get('state'),
                        'surfaces': (suggestion.get('exposure_report') or {}).get('surfaces', [])})
    marker = case['marker'].exists()
    no_errors = records and all(not row['error'] for row in records)
    decisions = [row['approval'] for row in records]
    if case['expected'] == 'allow':
        passed = no_errors and all(item == 'approved' for item in decisions) and marker and case.get('exit_code') == 0
    elif case['expected'] == 'mixed':
        passed = no_errors and decisions == ['approved', 'rejected'] and not marker and case.get('exit_code') != 0
    elif case['expected'] == 'changed':
        output = ''.join((root / (case['name'] + suffix)).read_text(errors='replace') for suffix in ['.stdout', '.stderr']).lower()
        passed = no_errors and not marker and case.get('exit_code') != 0 and ('chang' in output or 'hash' in output)
    else:
        passed = no_errors and all(item != 'approved' for item in decisions) and not marker
    return {key: value for key, value in case.items() if key not in ('script', 'marker', 'process', 'start')} | {
        'passed': bool(passed and len(records) == len(case['bindings']) and not case.get('timed_out')), 'consumer_executed': marker, 'records': records}


def batch_assertions(report):
    cases = {case['name']: case for case in report['cases']}
    sessions = {session['session_id']: session for session in report.get('sessions', [])}
    checks = {}
    for name in ['multi', 'policy']:
        if name in cases:
            ids = {record['session_id'] for record in cases[name]['records']}
            checks[name + '_one_model_turn'] = len(ids) == 1 and None not in ids and sessions[next(iter(ids))].get('prompt_turns') == 1
    if all(name in cases for name in ['early-0', 'early-1', 'late-0', 'late-1']):
        early = {row['session_id'] for name in ['early-0', 'early-1'] for row in cases[name]['records']}
        late = {row['session_id'] for name in ['late-0', 'late-1'] for row in cases[name]['records']}
        checks['late_requests_have_separate_batch'] = len(early) == len(late) == 1 and early.isdisjoint(late) and None not in early | late
        if checks['late_requests_have_separate_batch']:
            first, second = sessions[next(iter(early))], sessions[next(iter(late))]
            checks['model_batches_overlap'] = bool(first.get('last_assistant_at') and second.get('first_prompt_at') and second['first_prompt_at'] < first['last_assistant_at'])
            checks['parallel_batches_one_turn_each'] = first.get('prompt_turns') == second.get('prompt_turns') == 1
    if 'future' in cases and 'multi' in cases:
        before = {row['request_id'] for row in cases['multi']['records']}
        after = {row['request_id'] for row in cases['future']['records']}
        checks['future_call_requires_new_review'] = bool(after) and before.isdisjoint(after)
    if 'single' in cases:
        checks['single_audit_completed'] = bool(cases['single']['records']) and all(row['detail_state'] == 'complete' for row in cases['single']['records'])
    return checks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--cases', default='multi,policy,parallel,mutation,future,unknown,single')
    parser.add_argument('--timeout', type=int, default=240)
    args = parser.parse_args()
    args.cli, args.fixture = args.cli.resolve(), args.fixture.resolve()
    root = args.artifacts.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    log = open(root / 'fixture.log', 'wb')
    server = subprocess.Popen([str(args.fixture), str(root)], cwd=root, stdout=log, stderr=log)
    log.close()
    cases = []
    report = {'started_at': utc_now(), 'synthetic_only': True, 'cases': [], 'requested_groups': args.cases.split(','), 'completed_groups': []}
    try:
        deadline = time.monotonic() + 20
        while not (root / 'ready.json').exists():
            if server.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError('fixture did not start; inspect fixture.log')
            time.sleep(0.1)
        report['provider'] = json.loads((root / 'ready.json').read_text())
        for group in args.cases.split(','):
            print('Starting real-provider group: ' + group, flush=True)
            if group == 'multi':
                current = [start_case(args, root, group, [('secret/live-a', 'A'), ('secret/live-b', 'B')])]
            elif group == 'policy':
                current = [start_case(args, root, group, [('secret/live-a', 'A'), ('secret/live-blocked', 'B')], expected='mixed')]
            elif group == 'parallel':
                current = [start_case(args, root, 'early-' + str(i), [('secret/live-a', 'A')]) for i in range(2)]
                time.sleep(1.25)
                current += [start_case(args, root, 'late-' + str(i), [('secret/live-b', 'A')]) for i in range(2)]
            elif group == 'mutation':
                current = [start_case(args, root, 'changed-file', [('secret/live-a', 'A')], expected='changed'),
                           start_case(args, root, 'unchanged-file', [('secret/live-b', 'A')])]
                deadline = time.monotonic() + 5
                while not rows(root, current[0]):
                    if time.monotonic() > deadline:
                        raise RuntimeError('mutation request was not admitted')
                    time.sleep(0.05)
                current[0]['script'].write_text(SAFE + "\nprint('changed after admission')\n")
            elif group == 'future':
                current = [start_case(args, root, group, [('secret/live-a', 'A')], UNSAFE, 'deny')]
            elif group == 'unknown':
                current = [start_case(args, root, group, [('secret/live-a', 'A')], UNKNOWN, 'deny')]
            elif group == 'single':
                current = [start_case(args, root, group, [('secret/live-a', 'A')])]
            else:
                raise ValueError('unknown case group ' + group)
            cases += current
            wait_group(args, root, current)
            report['cases'] = [summarize(root, case) for case in cases]
            session_ids = {record['session_id'] for case in report['cases'] for record in case['records'] if record['session_id']}
            report['sessions'] = [session_metrics(session_id) for session_id in sorted(session_ids)]
            report['completed_groups'].append(group)
            report['batch_assertions'] = batch_assertions(report)
            (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2))
    except Exception as error:
        report['harness_error'] = str(error)
        raise
    finally:
        for case in cases:
            if case['process'].poll() is None:
                case['process'].terminate()
                case['process'].wait(timeout=5)
        if server.poll() is None:
            server.send_signal(signal.SIGINT)
            server.wait(timeout=30)
        report['finished_at'] = utc_now()
        report['passed'] = (bool(report['cases']) and all(case['passed'] for case in report['cases'])
                            and report['completed_groups'] == report['requested_groups']
                            and all(report.get('batch_assertions', {}).values()) and 'harness_error' not in report)
        (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2))
    print(json.dumps({'passed': report['passed'], 'report': str(root / 'report.json')}), flush=True)
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
