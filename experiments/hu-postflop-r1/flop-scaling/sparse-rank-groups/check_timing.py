"""Retain one bounded source-only runner check; no build, solver or archive read."""
import ast
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent


def pin(path):
    raw = path.read_bytes()
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def main():
    out = HERE / 'timing-checks01'
    out.mkdir()
    for name in ('run.py', 'test_run.py', 'check_timing.py'):
        ast.parse((HERE / name).read_text(encoding='utf-8'), filename=name)
    argv = [sys.executable, '-X', 'utf8', '-B', '-m', 'unittest', 'discover', '-s', str(HERE), '-p', 'test_run.py', '-v']
    started = time.monotonic()
    result = subprocess.run(argv, capture_output=True, timeout=20)
    record = {'schema': 'r1.sparse-rank-groups-runner-check/v1', 'status': 'passed' if result.returncode == 0 else 'failed',
              'argv': argv, 'exit_code': result.returncode, 'elapsed_seconds': time.monotonic() - started,
              'files': {n: pin(HERE / n) for n in ('run.py', 'protocol.jp.md', 'test_run.py', 'check_timing.py')},
              'local_native_execution': False, 'external_api': False, 'archive_access': False}
    for stream in ('stdout', 'stderr'):
        path = out / (stream + '.log')
        with path.open('xb') as file:
            file.write(getattr(result, stream))
        record[stream] = pin(path)
    with (out / 'receipt.json').open('x', encoding='utf-8') as file:
        json.dump(record, file, indent=2)
        file.write('\n')
    print(json.dumps({'status': record['status'], 'elapsed_seconds': record['elapsed_seconds'],
                      'files': record['files'], 'receipt': pin(out / 'receipt.json')}))
    return int(result.returncode != 0)


if __name__ == '__main__':
    raise SystemExit(main())
