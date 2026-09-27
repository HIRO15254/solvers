"""Capture one explicit VM15 SDK operation, without retries."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import subprocess

HERE = Path(__file__).resolve().parent
GCLOUD = 'C:/Program Files (x86)/Google/Cloud SDK/google-cloud-sdk/bin/gcloud.cmd'


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--label', required=True)
    p.add_argument('--timeout', type=int, default=240)
    p.add_argument('arguments', nargs=argparse.REMAINDER)
    a = p.parse_args()
    assert a.label and all(c.isalnum() or c in '-_' for c in a.label)
    args = a.arguments[1:] if a.arguments[:1] == ['--'] else a.arguments
    assert args and 0 < a.timeout <= 600
    env = os.environ.copy()
    env.update(CLOUDSDK_PYTHON='C:/Python313/python.exe', CLOUDSDK_ENCODING='utf-8', PYTHONIOENCODING='utf-8')
    record = {'argv': [GCLOUD, *args], 'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'timeout_seconds': a.timeout}
    with (HERE / (a.label + '.intent.json')).open('x') as f:
        json.dump(record, f, indent=2)
    try:
        done = subprocess.run(record['argv'], capture_output=True, env=env, timeout=a.timeout)
        code, stdout, stderr = done.returncode, done.stdout, done.stderr
    except subprocess.TimeoutExpired as error:
        code, stdout, stderr = None, error.stdout or b'', error.stderr or b''
        record['error'] = 'Timeout; inspect state before any further mutation, never automatically retry.'
    for kind, raw in [('stdout', stdout), ('stderr', stderr)]:
        with (HERE / (a.label + '.' + kind + '.log')).open('xb') as f:
            f.write(raw)
        record[kind] = {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}
    record.update(exit_code=code, ended_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
    with (HERE / (a.label + '.result.json')).open('x') as f:
        json.dump(record, f, indent=2)
    print(json.dumps({'label': a.label, 'exit_code': code, 'stdout_bytes': len(stdout), 'stderr_bytes': len(stderr)}))
    raise SystemExit(0 if code == 0 else 1)


if __name__ == '__main__':
    main()
