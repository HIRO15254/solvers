"""Small source/metadata checks only; no build, solve, perf, API or archive read."""
import ast
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess
import time

HERE = Path(__file__).resolve().parent


def pin(path):
    raw = path.read_bytes()
    return {'path': path.name, 'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def main():
    started = time.monotonic()
    parsed = []
    for path in sorted(HERE.glob('*.py')):
        ast.parse(path.read_text(), filename=str(path))
        parsed.append(pin(path))
    shell = []
    for path in sorted(HERE.glob('*.sh')):
        command = ['C:/Program Files/Git/bin/bash.exe', '-n', path.as_posix()]
        done = subprocess.run(command, capture_output=True, timeout=10)
        shell.append({'argv': command, 'exit_code': done.returncode,
                      'stdout': done.stdout.decode(errors='replace'), 'stderr': done.stderr.decode(errors='replace'),
                      'source': pin(path)})
        if done.returncode:
            raise ValueError('Shell syntax check failed: ' + path.name)
    result = {'at_utc': dt.datetime.now(dt.timezone.utc).isoformat(), 'python_ast': parsed,
              'shell_syntax': shell, 'elapsed_seconds': time.monotonic() - started,
              'native_build_or_solver': False, 'cloud_api': False, 'archive_read': False}
    with (HERE / 'control-checks.json').open('x') as stream:
        json.dump(result, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'status': 'passed', 'python_sources': len(parsed), 'shell_sources': len(shell),
                      'elapsed_seconds': result['elapsed_seconds']}))


if __name__ == '__main__':
    main()
