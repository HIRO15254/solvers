"""Record source-only tests and rustfmt checks once, without Cargo or solver work."""
import json
from pathlib import Path
import subprocess
import sys
import time
import hashlib

HERE = Path(__file__).resolve().parent


def pin(path):
    raw = path.read_bytes()
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def main():
    out = HERE / 'checks03'
    out.mkdir()
    commands = [('source-tests', [sys.executable, '-X', 'utf8', '-B', '-m', 'unittest', 'discover', '-s', str(HERE), '-p', 'test_prepare.py', '-v']),
                ('replay', [sys.executable, '-X', 'utf8', '-B', str(HERE / 'prepare.py'), '--check']),
                ('rustfmt', ['rustfmt', '--edition', '2024', '--config', 'skip_children=true', '--check', str(HERE / 'kernel.rs')])]
    rows = []
    for name, argv in commands:
        start = time.monotonic()
        result = subprocess.run(argv, capture_output=True, timeout=30)
        row = {'name': name, 'argv': argv, 'exit_code': result.returncode, 'elapsed_seconds': time.monotonic() - start}
        for stream in ('stdout', 'stderr'):
            path = out / (name + '.' + stream + '.log')
            with path.open('xb') as file:
                file.write(getattr(result, stream))
            row[stream] = pin(path)
        rows.append(row)
        if result.returncode:
            break
    record = {'schema': 'r1.sparse-rank-groups-source-check/v1', 'status': 'passed' if len(rows) == 3 and all(row['exit_code'] == 0 for row in rows) else 'failed',
              'commands': rows, 'files': {name: pin(HERE / name) for name in ('prepare.py', 'tests.rs.in', 'kernel.rs', 'candidate.patch', 'provenance.json', 'test_prepare.py', 'check.py', 'README.jp.md')},
              'rust_tests_compiled_or_executed': False, 'cargo_or_solver_or_archive_work': False}
    with (out / 'receipt.json').open('x', encoding='utf-8') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'status': record['status'], 'commands': [{k: row[k] for k in ('name', 'exit_code', 'elapsed_seconds')} for row in rows], 'receipt': pin(out / 'receipt.json')}))
    return int(record['status'] != 'passed')


if __name__ == '__main__':
    raise SystemExit(main())
