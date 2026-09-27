"""One explicit creation attempt with fixed STOP and a captured timeout; no retry."""
import datetime as dt
from decimal import Decimal as D
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
GCLOUD = 'C:/Program Files (x86)/Google/Cloud SDK/google-cloud-sdk/bin/gcloud.cmd'


def main():
    ledger = json.loads((CLOUD / 'budget.json').read_bytes())
    total = sum(D(str(r['reserved_usd'])) if not r['reservation_released'] else D(str(r['billed_usd'])) for r in ledger['reservations'])
    row, = [r for r in ledger['reservations'] if r['id'] == 'r1-20260927-18']
    if total > 40 or ledger['authorized_limit'] != 40 or row['reservation_released'] or row['reserved_usd'] != 2.5 or row['maximum_runtime_seconds'] != 3600:
        raise ValueError('Resource authorization differs')
    started = dt.datetime.now(dt.timezone.utc)
    stop = (started + dt.timedelta(seconds=3600)).strftime('%Y-%m-%dT%H:%M:%SZ')
    argv = ['compute', 'instances', 'create', row['instance'], '--project=' + row['project'], '--zone=' + row['zone'],
            '--machine-type=e2-standard-2', '--provisioning-model=SPOT', '--instance-termination-action=STOP',
            '--termination-time=' + stop, '--no-restart-on-failure', '--maintenance-policy=TERMINATE',
            '--image-family=ubuntu-2404-lts-amd64', '--image-project=ubuntu-os-cloud', '--boot-disk-type=pd-balanced',
            '--boot-disk-size=40GB', '--boot-disk-auto-delete', '--no-service-account', '--no-scopes',
            '--labels=campaign=hu-postflop-r1,reservation=' + row['id'],
            '--metadata-from-file=startup-script=' + str(HERE / 'bootstrap.sh'),
            '--format=json(id,name,status,creationTimestamp,scheduling,disks)', '--quiet']
    record = {'reservation_id': row['id'], 'attempted_at': started.isoformat(), 'termination_time': stop,
              'build_deadline_utc': (dt.datetime.fromisoformat(stop.replace('Z', '+00:00')) - dt.timedelta(seconds=2400)).isoformat(),
              'argv': argv, 'client_timeout_seconds': 180}
    with (CLOUD / ('launch-' + row['id'] + '.json')).open('x') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
    env = dict(os.environ, CLOUDSDK_PYTHON='C:/Python313/python.exe', CLOUDSDK_ENCODING='utf-8', PYTHONIOENCODING='utf-8')
    try:
        result = subprocess.run([GCLOUD, *argv], env=env, capture_output=True, timeout=180)
        code, stdout, stderr = result.returncode, result.stdout, result.stderr
    except subprocess.TimeoutExpired as error:
        code, stdout, stderr = None, error.stdout or b'', error.stderr or b''
        record['error'] = 'Client timeout; inspect named resource before any next mutation'
    with (CLOUD / ('create-result-' + row['id'] + '.json')).open('xb') as stream:
        stream.write(stdout)
    with (HERE / 'create.stderr.log').open('xb') as stream:
        stream.write(stderr)
    record.update(exit_code=code, ended_utc=dt.datetime.now(dt.timezone.utc).isoformat())
    with (HERE / 'create.receipt.json').open('x') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'exit_code': code, 'stop_deadline_utc': stop, 'build_deadline_utc': record['build_deadline_utc']}))
    raise SystemExit(0 if code == 0 else 1)


if __name__ == '__main__':
    main()
