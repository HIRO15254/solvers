"""After cleanup only: two bounded VM21 Monitoring GETs and compact receipts.

Adapted from the VM20 collector. No VM operation, ledger write, query job,
archive access, or native solver. Existing SDK credentials remain in memory.
"""
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = Path(__file__).resolve().parent
VM = HERE.parent
CLOUD = VM.parent
PROJECT = 'solvers-abstraction-20260723'
ZONE = 'us-central1-b'
INSTANCE_ID = '715936786015339093'
INSTANCE_NAME = 'solvers-r1-20260928-21'
LAUNCH = '2026-09-28T05:35:51.441617+00:00'
ORIGINAL_STOP = '2026-09-28T06:20:51Z'
SDK = r'C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\lib\gcloud.py'
METRICS = {'sent': 'compute.googleapis.com/instance/network/sent_bytes_count',
           'uptime': 'compute.googleapis.com/instance/uptime'}
MAX_FILE = 2 * 1024**2
MAX_INPUTS = 8 * 1024**2


def need(value, message):
    if not value:
        raise ValueError(message)


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def stamp(value):
    result = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    need(result.tzinfo is not None and result.utcoffset() is not None,
         'Explicit timestamp offset required')
    # Compute operation/disk timestamps can use -07:00; compare UTC instants.
    return result.astimezone(dt.timezone.utc)


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def compact(path):
    need(path.is_file() and not path.is_symlink() and path.stat().st_size <= MAX_FILE,
         'Regular compact evidence required: ' + str(path))
    return path.read_bytes()


def save(name, raw):
    path = HERE / name
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as stream:
        stream.write(raw)
    return {'path': name, **pin(raw)}


def collect_inputs(record):
    names = {'budget.json', 'launch-r1-20260928-21.json', 'create-result-r1-20260928-21.json',
             'vm21/create.receipt.json', 'vm21/create.stderr.log', 'vm21/reservation.json', 'vm21/reservation-check.json',
             'vm21/budget-proposal.json', 'vm21/phase32-plan.json',
             'preflight-vm20/pricing-sources.json'}
    names.update(p.relative_to(CLOUD).as_posix() for pattern in
                 ('transfer-*.intent.json', 'transfer-*.result.json', 'transfer-*.completed.json', 'download-check.json')
                 for p in VM.glob(pattern))
    commands = []
    for path in sorted(VM.glob('*.result.json')):
        value = json.loads(compact(path))
        if not {'argv', 'stdout', 'stderr', 'started_utc', 'ended_utc'} <= set(value):
            continue
        command = path.relative_to(CLOUD).as_posix()
        commands.append(command)
        names.add(command)
        for channel in ('stdout', 'stderr'):
            stream = path.with_name(path.name.removesuffix('.result.json') + '.' + channel + '.log')
            need(pin(compact(stream)) == value[channel], 'SDK stream identity differs')
            names.add(stream.relative_to(CLOUD).as_posix())
    pricing = json.loads(compact(CLOUD / 'preflight-vm20/pricing-sources.json'))
    for attempt in pricing['attempts']:
        for ref in attempt['retained']:
            name = 'preflight-vm20/' + ref['path']
            need(pin(compact(CLOUD / name)) == {k: ref[k] for k in ('bytes', 'sha256')}, 'Price fragment differs')
            names.add(name)
    payload = {name: compact(CLOUD / name) for name in sorted(names)}
    need(sum(map(len, payload.values())) <= MAX_INPUTS, 'Compact input total exceeds bound')
    create = json.loads(payload['vm21/create.receipt.json'])
    need(create['exit_code'] == 0
         and pin(payload['create-result-r1-20260928-21.json']) == create['stdout']
         and pin(payload['vm21/create.stderr.log']) == create['stderr'], 'Create receipt identity differs')
    for name, data in payload.items():
        record['inputs'][name] = save('inputs/' + name, data)
    record['captured_sdk_commands'] = commands


def verify_lifecycle(raw, commands):
    launch = raw('launch-r1-20260928-21.json')
    need(launch['reservation_id'] == 'r1-20260928-21' and stamp(launch['attempted_at']) == stamp(LAUNCH)
         and stamp(launch['termination_time']) == stamp(ORIGINAL_STOP), 'Original launch differs')
    need(launch['argv'][:4] == ['compute', 'instances', 'create', INSTANCE_NAME], 'Create target differs')
    for key, value in {'project': PROJECT, 'zone': ZONE, 'machine-type': 'e2-standard-2',
                       'provisioning-model': 'SPOT', 'instance-termination-action': 'STOP',
                       'termination-time': ORIGINAL_STOP, 'boot-disk-size': '20GB'}.items():
        need([a for a in launch['argv'] if a.startswith('--' + key + '=')] == ['--' + key + '=' + value],
             'Create envelope differs')
    reservation = raw('vm21/reservation.json')
    for key, value in {'id': 'r1-20260928-21', 'reserved_usd': 1.35, 'maximum_runtime_seconds': 2700,
                       'maximum_starts': 3, 'maximum_large_phase_seconds': 480, 'disk_gib': 20,
                       'maximum_download_gib': .3125, 'tax_price_delay_and_other_reserve_usd': 1,
                       'billing_rounding_slack_seconds': 120, 'billed_usd': None,
                       'reservation_released': False}.items():
        need(reservation.get(key) == value, 'Original reservation differs: ' + key)
    creation, = raw('create-result-r1-20260928-21.json')
    need(creation['id'] == INSTANCE_ID and creation['name'] == INSTANCE_NAME
         and stamp(creation['scheduling']['terminationTime']) == stamp(ORIGINAL_STOP), 'Created instance differs')
    base = f'https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}'
    target, disk_uri = base + '/instances/' + INSTANCE_NAME, base + '/disks/' + INSTANCE_NAME
    boot, = creation['disks']
    need(boot['source'] == disk_uri and boot['autoDelete'] is True and int(boot['diskSizeGb']) == 20,
         'Created disk differs')
    phase = raw('vm21/phase32-plan.json')
    need(phase['instance_id'] == INSTANCE_ID and stamp(phase['original_stop_utc']) == stamp(ORIGINAL_STOP),
         'Large phase identity differs')
    need(479 < (stamp(phase['phase_stop_utc']) - stamp(phase['armed_at_utc'])).total_seconds() <= 480
         and stamp(LAUNCH) < stamp(phase['armed_at_utc']) < stamp(phase['phase_stop_utc']) < stamp(ORIGINAL_STOP),
         'Fixed highCPU deadline differs')
    receipts = [(name, raw(name)) for name in commands]

    def matching(resource, verb, exact_filter=None):
        result = []
        for name, receipt in receipts:
            args = receipt['argv'][1:]
            if args[:3] != ['compute', resource, verb] or '--project=' + PROJECT not in args:
                continue
            if exact_filter is not None and '--filter=' + exact_filter not in args:
                continue
            if verb in ('describe', 'start') and (args[3:4] != [INSTANCE_NAME] or '--zone=' + ZONE not in args):
                continue
            need(receipt['exit_code'] == 0 and stamp(receipt['started_utc']) <= stamp(receipt['ended_utc']),
                 'Lifecycle command failed or time reversed')
            value = raw(name.removesuffix('.result.json') + '.stdout.log')
            result.append((name, receipt, value))
        return result

    ops_records = matching('operations', 'list', 'targetId=' + INSTANCE_ID)
    need(ops_records, 'Cleanup operation list absent')
    operation_name, operation_receipt, operations = max(ops_records, key=lambda r: stamp(r[1]['ended_utc']))
    need(operations and all(o['targetId'] == INSTANCE_ID and o['targetLink'] == target
         and o['status'] == 'DONE' and not o.get('error') for o in operations), 'Operation result/target differs')
    allowed = {'insert', 'setMetadata', 'start', 'stop', 'setMachineType', 'setScheduling', 'delete'}
    need(all(o['operationType'] in allowed for o in operations), 'Unreviewed lifecycle operation')
    need(sum(o['operationType'] == 'start' for o in operations) == 2
         and sum(o['operationType'] == 'setMachineType' for o in operations) == 2, 'Unexpected restart/resize count')
    deletion, = [o for o in operations if o['operationType'] == 'delete']
    need(stamp(LAUNCH) < stamp(deletion['endTime']) < stamp(operation_receipt['started_utc']), 'Cleanup ordering differs')
    absent = {}
    for resource in ('instances', 'disks', 'addresses'):
        candidates = [row for row in matching(resource, 'list', 'name~solvers-r1-')
                      if stamp(row[1]['started_utc']) > stamp(deletion['endTime'])]
        need(candidates, 'Post-delete absence receipt missing: ' + resource)
        name, receipt, value = max(candidates, key=lambda row: stamp(row[1]['ended_utc']))
        need(value == [], 'Campaign resource remains: ' + resource)
        absent[resource] = {'receipt': name, 'ended_utc': receipt['ended_utc']}
    disk_records = matching('disks', 'describe')
    need(disk_records, 'Disk lifetime evidence missing')
    disk_name, disk_receipt, disk = max(disk_records, key=lambda row: stamp(row[1]['ended_utc']))
    need(disk['id'].isdecimal() and disk['name'] == INSTANCE_NAME and disk['sizeGb'] == '20'
         and disk['type'] == base + '/diskTypes/pd-balanced' and disk['users'] == [target]
         and stamp(LAUNCH) <= stamp(disk['creationTimestamp']) < stamp(deletion['endTime']), 'Disk lifetime/shape differs')
    states = []
    state_records = [(name, receipt, state, 'describe')
                     for name, receipt, state in matching('instances', 'describe')]
    for name, receipt, response in matching('instances', 'start'):
        need(isinstance(response, list) and len(response) == 1,
             'Start response must contain exactly one instance')
        state, = response
        need(state['status'] == 'RUNNING', 'Successful start response is not RUNNING')
        state_records.append((name, receipt, state, 'start_response'))
    for name, receipt, state, evidence_kind in state_records:
        need(state['id'] == INSTANCE_ID and state['name'] == INSTANCE_NAME, 'Observed instance changed')
        need(state['machineType'] in [base + '/machineTypes/' + m for m in ('e2-standard-2', 'e2-highcpu-32')],
             'Observed non-E2 machine')
        scheduling = state['scheduling']
        need(scheduling['provisioningModel'] == 'SPOT' and scheduling['automaticRestart'] is False
             and scheduling['instanceTerminationAction'] == 'STOP'
             and stamp(scheduling['terminationTime']) in (stamp(ORIGINAL_STOP), stamp(phase['phase_stop_utc'])),
             'Observed scheduling differs')
        states.append({'receipt': name, 'evidence_kind': evidence_kind,
                       'observed_at': receipt['ended_utc'], 'state': state})
    need(any(s['state']['status'] == 'RUNNING' and s['state']['machineType'].endswith('/e2-standard-2') for s in states),
         'Observed small RUNNING state absent')
    large_running = [s for s in states if s['state']['status'] == 'RUNNING'
                     and s['state']['machineType'].endswith('/e2-highcpu-32')]
    need(large_running and all(stamp(s['state']['scheduling']['terminationTime']) == stamp(phase['phase_stop_utc'])
                              for s in large_running), 'Observed large RUNNING state/deadline absent')
    need(all(stamp(phase['armed_at_utc']) < stamp(s['observed_at']) < stamp(phase['phase_stop_utc'])
             for s in large_running), 'Large RUNNING evidence outside fixed phase')
    return {'launch': LAUNCH, 'original_stop': ORIGINAL_STOP, 'phase': phase,
            'operations_receipt': operation_name, 'deletion': deletion,
            'absence': max((r['ended_utc'] for r in absent.values()), key=stamp), 'absence_receipts': absent,
            'disk': disk, 'disk_receipt': disk_name, 'disk_absence': absent['disks']['ended_utc'],
            'observed_states': states,
            'highcpu_time_basis': 'Retain hard phase window and raw starts/stops; cost interpretation is a separate reviewed offline step.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--acquire-after-cleanup', action='store_true')
    args = parser.parse_args()
    need(args.acquire_after_cleanup, 'Explicit post-cleanup invocation required; preparation never acquires')
    need(not (HERE / 'acquisition.json').exists(), 'Fresh acquisition required')
    record = {'schema': 'r1.vm21-usage-acquisition/v1', 'started_at': now(), 'project': PROJECT,
              'instance_id': INSTANCE_ID, 'instance_name': INSTANCE_NAME, 'source': pin(Path(__file__).read_bytes()),
              'status': 'running', 'inputs': {}, 'requests': [], 'maximum_monitoring_gets': 2,
              'maximum_response_bytes_per_get': MAX_FILE, 'credential_material_retained': False,
              'cloud_mutations': False, 'budget_mutations': False, 'billed_usd': None,
              'usage_coverage': 'Unknown; missing observations are not zero usage.'}
    token, deadline = None, time.monotonic() + 100
    try:
        collect_inputs(record)
        raw = lambda name: json.loads((HERE / record['inputs'][name]['path']).read_bytes())
        lifecycle = verify_lifecycle(raw, record['captured_sdk_commands'])
        record['lifecycle'] = lifecycle
        record['interval_start'], record['interval_end'] = LAUNCH, lifecycle['absence']
        need(stamp(record['interval_end']) < stamp(record['started_at']), 'Acquisition predates cleanup')
        auth = subprocess.run([sys.executable, SDK, 'auth', 'print-access-token', '--quiet'],
                              capture_output=True, timeout=30)
        record['authentication'] = {'returncode': auth.returncode, 'stderr_bytes': len(auth.stderr)}
        need(auth.returncode == 0, 'Existing authentication unavailable')
        token = auth.stdout.decode('ascii').strip()
        need(token and not any(c.isspace() for c in token), 'Unexpected authentication response')
        for label, metric in METRICS.items():
            remaining = deadline - time.monotonic()
            need(remaining > 0, 'Acquisition deadline elapsed')
            params = {'filter': f'metric.type = "{metric}" AND resource.type = "gce_instance" AND resource.labels.project_id = "{PROJECT}" AND resource.labels.instance_id = "{INSTANCE_ID}"',
                      'interval.startTime': LAUNCH, 'interval.endTime': lifecycle['absence'], 'view': 'FULL', 'pageSize': '1000'}
            url = f'https://monitoring.googleapis.com/v3/projects/{PROJECT}/timeSeries?' + urllib.parse.urlencode(params)
            entry = {'label': label, 'metric': metric, 'page': 0, 'method': 'GET', 'url': url, 'requested_at': now()}
            record['requests'].append(entry)
            request = urllib.request.Request(url, headers={'Authorization': 'Bearer ' + token})
            try:
                response = urllib.request.urlopen(request, timeout=min(20, remaining))
            except urllib.error.HTTPError as error:
                response = error
            with response:
                body = response.read(MAX_FILE + 1)
                need(len(body) <= MAX_FILE, 'Response cap exceeded')
                entry.update(http_status=response.status, completed_at=now(), response=save(f'vm21-{label}-00.json', body))
            need(entry['http_status'] == 200, 'Monitoring unavailable')
            data = json.loads(body)
            need(not data.get('nextPageToken'), 'Single page bound exceeded')
            entry['point_count'] = sum(len(s.get('points', [])) for s in data.get('timeSeries', []))
        record['status'] = 'completed'
    except Exception as error:
        record.update(status='unavailable_or_incomplete', error_type=type(error).__name__, error=str(error))
    finally:
        token = None
        record['ended_at'] = now()
        save('acquisition.json', (json.dumps(record, indent=2) + '\n').encode())
    print(json.dumps({'status': record['status'], 'http_statuses': [r.get('http_status') for r in record['requests']],
                      'point_counts': [r.get('point_count') for r in record['requests']]}))
    return int(record['status'] != 'completed')


if __name__ == '__main__':
    raise SystemExit(main())
