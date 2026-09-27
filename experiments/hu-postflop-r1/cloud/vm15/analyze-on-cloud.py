"""Bounded post-workload verification only, on the cloud host."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

PACKAGE = Path('/opt/r1/flop-worker-cloud32-package')
CONTROLS = PACKAGE / 'experiments/hu-postflop-r1/flop-scaling/worker-scratch/cloud32'
PROOF = Path('/opt/r1/flop-worker-cloud32-proof01')
OUT = Path('/tmp/flop-worker-cloud32-analysis01')
CASE = Path('/tmp/case_analyze.py')


def pin(path):
    data = path.read_bytes()
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def main():
    state = subprocess.run(['systemctl', 'show', 'solvers-r1-vm15-worker32', '-p', 'ActiveState', '-p', 'MainPID'],
                           capture_output=True, text=True, timeout=10, check=True)
    values = dict(line.split('=', 1) for line in state.stdout.splitlines())
    if values.get('ActiveState') not in {'inactive', 'failed'} or values.get('MainPID') != '0':
        raise ValueError('Workload must be quiescent before verification')
    if OUT.exists() or OUT.is_symlink():
        raise ValueError('Fresh verification output required')
    if pin(CONTROLS / 'analyze.py')['sha256'] != '221ec03585ff20e560482f364690bc0e63cfb84c8c83e8fdd5297aefe4ee98ce':
        raise ValueError('Packaged full reader changed')
    if pin(CASE)['sha256'] != '40b88fa36ae6055b60b5089f1aab1f42548df0f5024eb29e057b96aa108a7329':
        raise ValueError('Case reader changed')
    OUT.mkdir()
    env = {**os.environ, 'PYTHONDONTWRITEBYTECODE': '1', 'PYTHONPATH': str(CONTROLS)}
    commands = [('full', [str(CONTROLS / 'analyze.py'), '--proof', str(PROOF)])]
    commands += [(case, [str(CASE), '--proof', str(PROOF), '--case', case]) for case in ('narrow', 'expanded')]
    runs = []
    for name, argv in commands:
        command = [sys.executable, '-B', *argv, '--out', str(OUT / (name + '.json'))]
        try:
            result = subprocess.run(command, env=env, capture_output=True, timeout=90)
            code, stdout, stderr = result.returncode, result.stdout, result.stderr
        except subprocess.TimeoutExpired as error:
            code, stdout, stderr = None, error.stdout or b'', error.stderr or b''
        (OUT / (name + '.stdout.log')).write_bytes(stdout)
        (OUT / (name + '.stderr.log')).write_bytes(stderr)
        runs.append({'name': name, 'command': command, 'exit_code': code})
    text = (PROOF / 'tests-worker/supervisor.stdout.log').read_text()
    totals = [tuple(map(int, row)) for row in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', text)]
    summary = {'runs': runs, 'readers': {'full': pin(CONTROLS / 'analyze.py'), 'case': pin(CASE)},
               'tests': {'result_groups': len(totals), 'passed': sum(r[0] for r in totals),
                         'failed': sum(r[1] for r in totals), 'ignored': sum(r[2] for r in totals),
                         'scope': 'candidate engine/holdem/cfr-ref unit and integration, release; not full workspace'},
               'files': {p.name: pin(p) for p in sorted(OUT.iterdir()) if p.is_file()}}
    (OUT / 'receipt.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary))
    return 0 if all(r['exit_code'] == 0 for r in runs) else 1


if __name__ == '__main__':
    raise SystemExit(main())
