"""Finite read-only SDK capture for two historical IDs and campaign absence.

One selected command per invocation. Existing authentication only; no resources,
ledger, billing settings, queries, archives or native solver are changed/read.
"""
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
SDK = r'C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\lib\gcloud.py'
PROJECT = 'solvers-abstraction-20260723'
TARGETS = {'vm02': '7774326211091312507', 'vm05': '1627891813360280286'}


def pin(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def save(name, data):
    with (HERE / name).open('xb') as stream:
        stream.write(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('kind', choices=[*TARGETS, 'instances', 'disks', 'addresses'])
    kind = parser.parse_args().kind
    if kind in TARGETS:
        args = ['compute', 'operations', 'list', '--filter=targetId=' + TARGETS[kind]]
    else:
        args = ['compute', kind, 'list', '--filter=name~solvers-r1-']
    argv = [sys.executable, SDK, *args, '--project=' + PROJECT, '--limit=100', '--format=json', '--quiet']
    stem = kind + '-01'
    record = {'schema': 'r1.early-delete-sdk-capture/v1', 'kind': kind, 'project': PROJECT,
              'argv': argv, 'started_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
              'timeout_seconds': 60, 'maximum_returned_items': 100, 'read_only': True}
    save(stem + '.intent.json', (json.dumps(record, indent=2) + '\n').encode())
    try:
        result = subprocess.run(argv, capture_output=True, timeout=60)
        stdout, stderr = result.stdout, result.stderr
        record['exit_code'] = result.returncode
    except subprocess.TimeoutExpired as error:
        stdout, stderr = error.stdout or b'', error.stderr or b''
        record.update(exit_code=None, error='SDK timeout; no retry')
    save(stem + '.stdout.log', stdout)
    save(stem + '.stderr.log', stderr)
    record.update(stdout=pin(stdout), stderr=pin(stderr), ended_utc=dt.datetime.now(dt.timezone.utc).isoformat())
    save(stem + '.result.json', (json.dumps(record, indent=2) + '\n').encode())
    print(json.dumps({'kind': kind, 'exit_code': record['exit_code'], 'stdout': record['stdout'], 'stderr': record['stderr']}))
    return int(record['exit_code'] != 0)


if __name__ == '__main__':
    raise SystemExit(main())
