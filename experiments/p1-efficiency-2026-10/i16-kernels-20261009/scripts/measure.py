"""Sequential, alternating old/new P1 measurements; run from the repo root."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[4]
EXP = Path(__file__).resolve().parents[1]
MAIN_CONFIGS = Path('C:/Users/PC_User/orca/workspaces/solvers/cisco/experiments/p1-perf-2026-10/lane-fold-20261008/configs')


def execute(cmd, stem):
    start = time.time()
    with (EXP / 'raw' / f'{stem}.log').open('w', encoding='utf-8') as log:
        result = subprocess.run(list(map(str, cmd)), cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
    record = {'argv': list(map(str, cmd)), 'startUnix': start,
              'wallSecs': time.time() - start, 'exitCode': result.returncode}
    (EXP / 'raw' / f'{stem}.command.json').write_text(json.dumps(record, indent=2) + '\n')
    if result.returncode:
        raise RuntimeError(f'{stem} failed: see raw log')
    print(f'{stem}: {record["wallSecs"]:.2f}s', flush=True)


def binary(version, name):
    subdir = 'examples' if name == 'p1_bench' else ''
    return ROOT / 'target' / version / 'release' / subdir / f'{name}.exe'


def speed(reps, turn_reps):
    for case, iters in [('c_turn2', 30), ('c_flop1', 15)]:
        for storage in ['i16', 'i16-f32avg', 'f32']:
            for rep in range(1, (turn_reps if case == 'c_turn2' else reps) + 1):
                for version in ['old', 'new']:
                    stem = f'bench_{case}_{storage}_{rep}_{version}'
                    execute([binary(version, 'p1_bench'), MAIN_CONFIGS / f'{case}.toml',
                             '--threads', 8, '--warmup', 5, '--iters', iters, '--evals', 0,
                             '--storage', storage, '--json', EXP / 'raw' / f'{stem}.json'], stem)


def convergence(run_group):
    for case, storage in [('c_turn2', 'i16'), ('c_turn2', 'i16-f32avg'), ('c_flop1', 'i16')]:
        config = EXP / 'configs' / f'{case}_{storage}.toml'
        raw = (MAIN_CONFIGS / f'{case}.toml').read_text(encoding='utf-8')
        raw = raw.replace('storage = "f32"', f'storage = "{storage}"')
        raw = raw.replace('check_every = 25', 'check_every = 10')
        raw = raw.replace('[run]', '[run]\nfinal_checkpoint = false')
        config.write_text(raw, encoding='utf-8')
        for version in ['old', 'new']:
            stem = f'solve_{case}_{storage}_{version}'
            out = ROOT / 'runs' / 'i16-kernels' / run_group / stem
            execute([binary(version, 'solvers'), 'solve', config, '--out', out, '--threads', 8], stem)
            for name in ['progress.jsonl', 'events.jsonl', 'run.json', 'manifest.json', 'run.toml']:
                (EXP / 'raw' / f'{stem}.{name}').write_bytes((out / name).read_bytes())


def summarize():
    speed_results = []
    for case in ['c_turn2', 'c_flop1']:
        for storage in ['i16', 'i16-f32avg', 'f32']:
            row = {'case': case, 'storage': storage}
            for version in ['old', 'new']:
                files = sorted((EXP / 'raw').glob(f'bench_{case}_{storage}_*_{version}.json'))
                values = [json.loads(p.read_text())['secsPerIter'] for p in files]
                if not values:
                    break
                row[version] = {'samples': values, 'median': statistics.median(values)}
            else:
                row['ratio'] = row['new']['median'] / row['old']['median']
                speed_results.append(row)
    quality = []
    for case, storage, pot in [('c_turn2', 'i16', 22.5), ('c_turn2', 'i16-f32avg', 22.5), ('c_flop1', 'i16', 5.5)]:
        row = {'case': case, 'storage': storage, 'potBB': pot}
        for version in ['old', 'new']:
            path = EXP / 'raw' / f'solve_{case}_{storage}_{version}.progress.jsonl'
            if not path.exists():
                break
            progress = [json.loads(line) for line in path.read_text().splitlines()]
            crossing = next((p for p in progress if p['nash_conv'] / 2 / pot <= 0.001), None)
            row[version] = crossing
        else:
            row['ratio'] = row['new']['iteration'] / row['old']['iteration'] if row['new'] and row['old'] else None
            quality.append(row)
    hashes = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
              for p in [ROOT / 'Cargo.lock', ROOT / '.cargo/config.toml',
                        *[binary(v, n) for v in ['old', 'new'] for n in ['solvers', 'p1_bench']]]}
    result = {'speed': speed_results, 'convergence': quality, 'sha256': hashes}
    (EXP / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('mode', choices=['speed', 'convergence', 'summarize'])
    parser.add_argument('--reps', type=int, default=3)
    parser.add_argument('--turn-reps', type=int, default=9)
    parser.add_argument('--run-group', default='final')
    parser.add_argument('--config-dir', type=Path, default=MAIN_CONFIGS if MAIN_CONFIGS.exists() else EXP / 'configs')
    args = parser.parse_args()
    MAIN_CONFIGS = args.config_dir
    if args.mode == 'speed':
        speed(args.reps, args.turn_reps)
    elif args.mode == 'convergence':
        convergence(args.run_group)
    else:
        summarize()
