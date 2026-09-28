"""Record two small offline metadata checks once; no cloud/native/archive work."""
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent


def pin(path):
    data = path.read_bytes()
    return {'path': path.name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def main():
    if (HERE / 'checks01.json').exists() or (HERE / 'report.json').exists():
        raise ValueError('Checks require fresh immutable output names')
    record = {'schema': 'r1.vm19-usage-checks/v1', 'at_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
              'commands': [], 'native_or_cloud_work': False, 'local_archive_decompression': False}
    commands = [('tests', [sys.executable, '-X', 'utf8', '-B', '-m', 'unittest', 'discover', '-s', str(HERE), '-p', 'test_analyze.py', '-v']),
                ('generate', [sys.executable, '-X', 'utf8', '-B', str(HERE / 'analyze.py'), '--report', str(HERE / 'report.json')])]
    for label, argv in commands:
        start = time.monotonic()
        completed = subprocess.run(argv, capture_output=True, timeout=30)
        step = {'label': label, 'argv': argv, 'exit_code': completed.returncode, 'elapsed_seconds': time.monotonic() - start}
        for stream in ('stdout', 'stderr'):
            path = HERE / (label + '01.' + stream + '.log')
            with path.open('xb') as file:
                file.write(getattr(completed, stream))
            step[stream] = pin(path)
        record['commands'].append(step)
        if completed.returncode:
            break
    record['status'] = 'passed' if len(record['commands']) == 2 and all(c['exit_code'] == 0 for c in record['commands']) else 'failed'
    record['files'] = [pin(HERE / name) for name in ('collect.py', 'analyze.py', 'test_analyze.py', 'check.py', 'acquisition.json', 'report.json', 'README.jp.md') if (HERE / name).exists()]
    with (HERE / 'checks01.json').open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'status': record['status'], 'commands': [{'label': c['label'], 'elapsed_seconds': c['elapsed_seconds']} for c in record['commands']], 'report': pin(HERE / 'report.json') if (HERE / 'report.json').exists() else None}))
    return int(record['status'] != 'passed')


if __name__ == '__main__':
    raise SystemExit(main())
