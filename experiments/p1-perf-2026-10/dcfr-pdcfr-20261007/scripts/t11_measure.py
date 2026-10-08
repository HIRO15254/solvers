import json
import os
from pathlib import Path
import shutil
import subprocess
import time

ROOT = Path.cwd().resolve()
SCRATCH = ROOT / 'runs/p1-t11'
MIN_FREE = 3 * 1024**3
minimum_free = shutil.disk_usage(ROOT).free


def run(name, args, prototype=None, expected=0):
    global minimum_free
    env = os.environ.copy()
    env.pop('SOLVERS_P1_PDCFR', None)
    if prototype is not None:
        env['SOLVERS_P1_PDCFR'] = prototype
    free = shutil.disk_usage(ROOT).free
    if free < MIN_FREE + 512 * 1024**2:
        raise RuntimeError(f'Disk headroom insufficient: {free} bytes')
    start = time.perf_counter()
    with (SCRATCH / (name + '.log')).open('wb') as log:
        child = subprocess.Popen([str(a) for a in args], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        while child.poll() is None:
            free = shutil.disk_usage(ROOT).free
            minimum_free = min(minimum_free, free)
            if free < MIN_FREE:
                child.terminate()
                child.wait()
                raise RuntimeError(f'Disk free below 3 GiB: {free} bytes; stopped {name}')
            try:
                child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                pass
    elapsed = time.perf_counter() - start
    record = {'name': name, 'argv': [str(a) for a in args], 'prototype': prototype,
              'exit': child.returncode, 'processWallSecs': elapsed, 'minimumFreeBytes': minimum_free}
    (SCRATCH / (name + '.execution.json')).write_text(json.dumps(record, indent=2), encoding='utf-8')
    print(json.dumps(record), flush=True)
    if child.returncode != expected:
        raise RuntimeError(f'{name} exited {child.returncode}; expected {expected}')
    return record


def discard_large(directory):
    # Only named generated artifacts beneath this task's scratch directory.
    directory = directory.resolve()
    assert directory.is_relative_to(SCRATCH.resolve())
    for name in ['checkpoint.ckpt', 'solution.sol']:
        path = directory / name
        if path.exists():
            path.unlink()


new = ROOT / 'target/release/solvers.exe'
old = ROOT / 'target/p1-t11-old/release/solvers.exe'
verify = ROOT / 'target/release/examples/verify_save.exe'
bench = ROOT / 'target/release/examples/p1_bench.exe'
turn6 = 'experiments/p1-perf-2026-10/sol-strategy-stream-20261007/turn6.toml'
config = 'experiments/p1-perf-2026-10/cfr-precision-20261007/configs/c_turn.toml'

if not (SCRATCH/'solution-compare.execution.json').exists():
    for name, binary in [('old-turn6', old), ('new-turn6', new)]:
        run(name, [binary, 'solve', turn6, '--out', SCRATCH/name, '--threads', '4'])
    run('solution-compare', [verify, 'solution', SCRATCH/'old-turn6/solution.sol', SCRATCH/'new-turn6/solution.sol'])
assert json.loads((SCRATCH/'solution-compare.execution.json').read_text(encoding='utf-8'))['exit'] == 0
for name in ['old-turn6', 'new-turn6']:
    run(name+'-checkpoint-hash', [verify, 'checkpoint', SCRATCH/name/'checkpoint.ckpt'])
a = (SCRATCH/'old-turn6-checkpoint-hash.log').read_text(encoding='utf-8').strip()
b = (SCRATCH/'new-turn6-checkpoint-hash.log').read_text(encoding='utf-8').strip()
assert a == b, (a, b)
assert (SCRATCH/'old-turn6/run.toml').read_bytes() == (SCRATCH/'new-turn6/run.toml').read_bytes()
print('checkpoint state hashes and effective configs equal: ' + a, flush=True)
for name in ['old-turn6', 'new-turn6']:
    discard_large(SCRATCH/name)

rows = []
for name, prototype in [('dcfr', None), ('pdcfr-2.3-5', '2.3,5'), ('pcfr-inf-2', 'inf,2')]:
    directory = SCRATCH/name
    execution = run(name, [new, 'solve', config, '--out', directory, '--threads', '8'], prototype)
    result = json.loads((directory/'run.json').read_text(encoding='utf-8'))
    row = {'algorithm': name, 'prototype': prototype, 'iterations': result['iterations'],
           'solverWallSecs': result['wallSecs'], 'processWallSecs': execution['processWallSecs'],
           'nashConv': result['nashConv'],
           'targetReached': '"target-reached"' in (directory/'events.jsonl').read_text(encoding='utf-8')}
    rows.append(row)
    (SCRATCH/'convergence.json').write_text(json.dumps(rows, indent=2), encoding='utf-8')
    print(json.dumps(row), flush=True)
    discard_large(directory)

run('bench-prototype', [bench, turn6, '--threads', '4', '--warmup', '0', '--iters', '2', '--evals', '1'], '2.3,5')
run('bench-i16-rejected', [bench, turn6, '--storage', 'i16', '--iters', '0', '--evals', '0'], '2.3,5', expected=1)
run('bench-mixed-rejected', [bench, turn6, '--storage', 'i16-f32avg', '--iters', '0', '--evals', '0'], '2.3,5', expected=1)
run('bench-malformed-rejected', [bench, turn6, '--iters', '0', '--evals', '0'], 'NaN,5', expected=1)
