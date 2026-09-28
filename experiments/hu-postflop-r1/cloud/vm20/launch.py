"""One explicit creation attempt with fixed STOP and a captured timeout; no retry."""
import datetime as dt
from decimal import Decimal as D
import json
import hashlib
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
TEMPLATE = HERE.parent / 'vm19'
GCLOUD = 'C:/Program Files (x86)/Google/Cloud SDK/google-cloud-sdk/bin/gcloud.cmd'


def main():
    ledger = json.loads((CLOUD / 'budget.json').read_bytes())
    total = sum(D(str(r['reserved_usd'])) if not r['reservation_released'] else D(str(r['billed_usd'])) for r in ledger['reservations'])
    row, = [r for r in ledger['reservations'] if r['id'] == 'r1-20260928-20']
    if (row['instance'], row['project'], row['zone']) != ('solvers-r1-20260928-20', 'solvers-abstraction-20260723', 'us-central1-b'):
        raise ValueError('New resource identity differs')
    if total > 40 or ledger['authorized_limit'] != 40 or row['reservation_released'] or row['reserved_usd'] != 1.85 or row['maximum_runtime_seconds'] != 2700:
        raise ValueError('Resource authorization differs')
    check = json.loads((HERE / 'reservation-check.json').read_bytes())
    raw = (CLOUD / 'budget.json').read_bytes()
    if check['budget_after'] != {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}:
        raise ValueError('Reserved ledger changed before launch')
    if row['maximum_starts'] != 3 or row['disk_gib'] != 20 or row['capacity_or_quota_fallback'] is not None:
        raise ValueError('Finite E2-only envelope differs')
    pack = json.loads((TEMPLATE / 'pack-receipt.json').read_bytes())
    if pack['archive'] != {'path': 'deployment01.tar.gz', 'bytes': 1373074, 'sha256': 'ad156105b5323c91c4c9e715525e9efdf75ec07acb193db3eb3106581fec466f'}:
        raise ValueError('Fixed deployment template differs')
    if pack['manifest'] != {'bytes': 75417, 'sha256': 'c376fe25a9ba2fbf2d61f5fca71bf366223fb374b7a1ef6a20cbe2b11403d04a'}:
        raise ValueError('Fixed deployment manifest differs')
    raw = (TEMPLATE / pack['archive']['path']).read_bytes()
    if {k: pack['archive'][k] for k in ('bytes', 'sha256')} != {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}:
        raise ValueError('Frozen deployment changed')
    raw = (TEMPLATE / 'source-manifest.json').read_bytes()
    if pack['manifest'] != {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}:
        raise ValueError('Frozen manifest changed')
    manifest = json.loads(raw)
    raw = (TEMPLATE / 'bootstrap.sh').read_bytes()
    if manifest['files']['experiments/hu-postflop-r1/cloud/vm19/bootstrap.sh'] != {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}:
        raise ValueError('Actual launch bootstrap differs from frozen package')
    started = dt.datetime.now(dt.timezone.utc)
    if (started - dt.datetime.fromisoformat(check['at_utc'])).total_seconds() > 1800:
        raise ValueError('Reservation/preflight older than30min')
    stop = (started + dt.timedelta(seconds=2700)).strftime('%Y-%m-%dT%H:%M:%SZ')
    argv = ['compute', 'instances', 'create', row['instance'], '--project=' + row['project'], '--zone=' + row['zone'],
            '--machine-type=e2-standard-2', '--provisioning-model=SPOT', '--instance-termination-action=STOP',
            '--termination-time=' + stop, '--no-restart-on-failure', '--maintenance-policy=TERMINATE',
            '--image-family=ubuntu-2404-lts-amd64', '--image-project=ubuntu-os-cloud', '--boot-disk-type=pd-balanced',
            '--boot-disk-size=20GB', '--boot-disk-auto-delete', '--no-service-account', '--no-scopes',
            '--labels=campaign=hu-postflop-r1,reservation=' + row['id'],
            '--metadata-from-file=startup-script=' + str(TEMPLATE / 'bootstrap.sh'),
            '--format=json(id,name,status,creationTimestamp,scheduling,disks)', '--quiet']
    record = {'template': '../vm19', 'fresh_instance_and_build': True, 'reservation_id': row['id'], 'attempted_at': started.isoformat(), 'termination_time': stop,
              'build_deadline_utc': (dt.datetime.fromisoformat(stop.replace('Z', '+00:00')) - dt.timedelta(seconds=1500)).isoformat(),
              'argv': argv, 'client_timeout_seconds': 180}
    with (CLOUD / ('launch-' + row['id'] + '.json')).open('x') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
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
