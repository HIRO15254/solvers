"""Local-only QRE gate and plateau solve. Run from the repository root.

python experiments/p2-method-2026-10/gtow-fixed/qre_local.py gate
python experiments/p2-method-2026-10/gtow-fixed/qre_local.py full

The full solve uses 16 threads, 1000-iteration snapshots, and the existing
loss(t-2000)-loss(t) < 0.02 rule; 6000 iterations is the hard cap. No cloud APIs.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
CACHE = ROOT / '.cache/p2-trunk/gtow-fixed/qre-genuine'
EVIDENCE = HERE / 'results/qre-genuine'
SPEC = importlib.util.spec_from_file_location('compare', HERE.parent / 'b4s-8h/compare.py')
COMPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARE)
REFERENCE = json.loads(COMPARE.REFERENCE.read_text(encoding='utf-8'))


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8', newline='\n')


def command(iterations, folder, extra):
    tables = Path(os.environ['LOCALAPPDATA']) / 'solvers/ehs2'
    binary = ROOT / 'target/release/examples/trunk_solve.exe'
    return [str(binary), '--config', 'experiments/p2-method-2026-10/b4s-8h/b4s-gtow-raise5.toml',
            '--leaf-model', 'l1', '--ehs2-cache', str(tables / 'v2-f32-t32-r32.postcard'),
            '--l1-relative', f'8x2:{tables / "v2-f128-t128-r128.postcard"}',
            '--l1-relative-prior', '10', '--l1-flop-groups', str(HERE / 'subsets/k25-groups.txt'),
            '--l1-hand', 'draws', '--tables-dir', '../_goal/gtowtree/tables',
            '--solver-k4-samples', '256', '--solver-k4-min-samples', '16',
            '--threads', '16', '--iterations', str(iterations), '--print-every', '100',
            '--l1-seed', '0', '--l1-eval-seed', '0',
            '--output', str(folder / 'solve.json'), '--output-profile', str(folder / 'profile.json'),
            *extra]


def score(path):
    result = COMPARE.compare(REFERENCE, COMPARE.load(path))
    summary = COMPARE.summary(result)
    return {'loss_bb100': summary['all']['loss_bb100'],
            'fourbet_loss_bb100': summary['vs 4bet+']['loss_bb100']}


def gate(iterations, inverse_temperature):
    profiles = {}
    for name, extra in [('baseline', []), ('baseline-no-regression', ['--l1-train-regression', 'false']),
                        ('qre', ['--l1-qre-prior', inverse_temperature])]:
        folder = CACHE / 'gate' / name
        folder.mkdir(parents=True, exist_ok=True)
        args = command(iterations, folder, ['--eval-every', '0', *extra])
        save(folder / 'command.json', args)
        with (folder / 'solve.log').open('w', encoding='utf-8') as log:
            subprocess.run(args, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True)
        path = folder / 'profile.json'
        result = json.loads((folder / 'solve.json').read_text(encoding='utf-8'))
        profiles[name] = {'sha256': hashlib.sha256(path.read_bytes()).hexdigest(), **score(path),
                          'solve': result}
    baseline = COMPARE.load(CACHE / 'gate/baseline/profile.json')
    no_regression = COMPARE.load(CACHE / 'gate/baseline-no-regression/profile.json')
    qre = COMPARE.load(CACHE / 'gate/qre/profile.json')
    differences = {}
    for name, nodes in [('baseline', baseline), ('baseline-no-regression', no_regression)]:
        changed = []
        for path, row in nodes.items():
            other = qre[path]
            delta = max(abs(a-b) for x, y in zip(row['probabilities'], other['probabilities']) for a, b in zip(x, y))
            if delta:
                changed.append({'path': list(path), 'max_action_probability_delta': delta})
        differences[name] = {'changed_nodes': len(changed), 'total_nodes': len(nodes),
                             'nodes': sorted(changed, key=lambda n: -n['max_action_probability_delta'])}
        if not changed:
            raise RuntimeError(f'Inert QRE profile relative to {name}; full solve prohibited')
    save(EVIDENCE / 'gate.json', {'iterations': iterations, 'lambda': inverse_temperature,
                                 'profiles': profiles, 'differences': differences})
    print(json.dumps({name: {k: v for k, v in p.items() if k != 'solve'} for name, p in profiles.items()}, indent=2), flush=True)


def full(inverse_temperature):
    gate_result = json.loads((EVIDENCE / 'gate.json').read_text(encoding='utf-8'))
    if gate_result['lambda'] != inverse_temperature or any(not d['changed_nodes'] for d in gate_result['differences'].values()):
        raise RuntimeError('A non-inert gate for this lambda is required first')
    # NashConv is not the plateau metric. Finished-iteration resume lets us save
    # class profiles every 1000 iterations without retaining evaluator scratch.
    folder = CACHE / f'lambda-{inverse_temperature}-profile-only'
    snapshots = folder / 'snapshots'
    snapshots.mkdir(parents=True, exist_ok=True)
    trajectory = {}
    reason = '6000-iteration cap'
    for iteration in range(1000, 6001, 1000):
        snapshot = snapshots / f'it{iteration:06}.profile.json'
        if not snapshot.exists():
            args = command(iteration, folder, ['--l1-qre-prior', inverse_temperature,
                '--eval-every', '0', '--state', str(folder / 'solve.state'),
                '--state-every', '120', '--output-postflop', str(folder / 'postflop.bin')])
            save(folder / f'command-{iteration}.json', args)
            with (folder / 'solve.log').open('a', encoding='utf-8') as log:
                subprocess.run(args, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True)
            snapshot.write_bytes((folder / 'profile.json').read_bytes())
            save(EVIDENCE / f'solve-{iteration}.json',
                 json.loads((folder / 'solve.json').read_text(encoding='utf-8')))
        current = score(snapshot)
        trajectory[iteration] = current
        print(f'iteration {iteration}: {json.dumps(current)}', flush=True)
        previous = trajectory.get(iteration - 2000)
        if previous and previous['loss_bb100'] - current['loss_bb100'] < 0.02:
            reason = f'plateau at snapshot {iteration}'
        save(EVIDENCE / 'trajectory.json', {'lambda': inverse_temperature,
            'threads': 16, 'cost_usd': 0, 'eval_every': 0, 'state_every_seconds': 120,
            'stop_reason': reason, 'snapshots': trajectory})
        if reason.startswith('plateau'):
            break
    result = json.loads((folder / 'solve.json').read_text(encoding='utf-8'))
    save(EVIDENCE / 'full.json', {'lambda': inverse_temperature, 'threads': 16,
        'cost_usd': 0, 'stop_reason': reason, 'score': score(folder / 'profile.json'), 'solve': result})
    print(f'finished: {reason}; {json.dumps(score(folder / "profile.json"))}', flush=True)
    retained = []
    for path in sorted(CACHE.rglob('*')):
        if not path.is_file():
            continue
        digest = hashlib.sha256()
        with path.open('rb') as source:
            for block in iter(lambda: source.read(1024 * 1024), b''):
                digest.update(block)
        retained.append(f'{digest.hexdigest()}  {path.relative_to(ROOT).as_posix()}')
    (EVIDENCE / 'retained.sha256').write_text('\n'.join(retained) + '\n', encoding='utf-8', newline='\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['gate', 'full'])
    parser.add_argument('--lambda', dest='inverse_temperature', default='0.5')
    parser.add_argument('--gate-iterations', type=int, default=50)
    options = parser.parse_args()
    if options.mode == 'gate':
        gate(options.gate_iterations, options.inverse_temperature)
    else:
        full(options.inverse_temperature)
