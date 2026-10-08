from pathlib import Path
import hashlib, json, os, shutil, struct, subprocess, time

ROOT = Path.cwd().resolve()
AREA = ROOT / 'runs/p1-t9'
OLD = ROOT / 'target/p1-t9-old/release/solvers.exe'
NEW = ROOT / 'target/release/solvers.exe'
VERIFY = ROOT / 'target/release/examples/verify_save.exe'
BASE = (ROOT / 'experiments/p1-perf-2026-10/sol-strategy-stream-20261007/turn6.toml').read_text(encoding='utf-8')
RESULT = AREA / 'comparison-results.jsonl'

def record(**data):
    with RESULT.open('a', encoding='utf-8') as f:
        f.write(json.dumps(data, ensure_ascii=False) + '\n')
    print(json.dumps(data, ensure_ascii=False), flush=True)

def free():
    gb = shutil.disk_usage(ROOT).free / 1024**3
    if gb < 3.1:
        raise RuntimeError(f'Disk threshold: {gb:.3f} GiB')

def run(binary, args, name, threads=1, expected=0):
    free()
    env = os.environ.copy()
    env['RAYON_NUM_THREADS'] = str(threads)
    env['CARGO_BUILD_JOBS'] = '4'
    for key in ['SOLVERS_P1_KERNEL', 'SOLVERS_P1_NORM']:
        env.pop(key, None)
    log = AREA / f'{name}.log'
    with log.open('wb') as f:
        process = subprocess.run([str(binary), *map(str, args)], stdout=f, stderr=subprocess.STDOUT, env=env)
    if process.returncode != expected:
        raise RuntimeError(f'{name}: exit {process.returncode}; {log.read_text(encoding="utf-8", errors="replace")[-2000:]}')
    return log

def clean(path):
    path = path.resolve()
    assert path.is_relative_to(AREA) and path.name.startswith('compare-')
    shutil.rmtree(path)

def config(storage, precision, label, base=BASE):
    setting = f'[solver]\nstorage = "{storage}"\n'
    if precision:
        setting += f'cfr_precision = "{precision}"\n'
    path = AREA / f'{label}.toml'
    path.write_text(base.replace('[solver.stop]', setting + '\n[solver.stop]'), encoding='utf-8')
    return path

def solve(binary, cfg, label, threads):
    out = AREA / f'compare-{label}'
    run(binary, ['solve', cfg, '--out', out, '--threads', threads], label, threads)
    record(step='solve', label=label, threads=threads, summary=json.loads((out / 'run.json').read_text()))
    return out

def compare(a, b, label, ignore=False, different=False, ignore_run=False):
    args = ['solution', a / 'solution.sol', b / 'solution.sol']
    if ignore:
        args += ['--ignore-cfr-precision']
    if ignore_run:
        args += ['--ignore-run']
    log = run(VERIFY, args, label, expected=1 if different else 0)
    record(step='payload', label=label, expected_difference=different, result=log.read_text(encoding='utf-8').strip())

def progress(path):
    return [(row['iteration'], struct.pack('<d', row['nash_conv'])) for row in map(json.loads, (path / 'progress.jsonl').read_text().splitlines())]

def main():
    RESULT.write_text('', encoding='utf-8')
    for binary in [OLD, NEW, VERIFY]:
        record(step='binary', path=str(binary.relative_to(ROOT)), sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
    for storage in ['f32', 'i16', 'i16-f32avg']:
        cfg_old = config(storage, None, f'{storage}-old')
        cfg64 = config(storage, 'f64', f'{storage}-f64')
        for threads in [1, 8]:
            label = f'{storage}-t{threads}'
            old = solve(OLD, cfg_old, label+'-old', threads)
            new = solve(NEW, cfg64, label+'-new64', threads)
            compare(old, new, label+'-legacy-payload', ignore=True)
            assert progress(old) == progress(new)
            record(step='nashconv', label=label, bit_equal=True)
            for view in ['strategy', 'ev']:
                a = AREA / f'{label}-old-{view}.json'
                b = AREA / f'{label}-new-{view}.json'
                run(OLD, ['export', old / 'solution.sol', view, '--node', 'root', '--output', a], label+'-old-'+view, threads)
                run(NEW, ['export', new / 'solution.sol', view, '--node', 'root', '--output', b], label+'-new-'+view, threads)
                assert a.read_bytes() == b.read_bytes(), f'{label} {view}'
                record(step='export', label=label, view=view, byte_equal=True, sha256=hashlib.sha256(a.read_bytes()).hexdigest())
            if threads == 1:
                cfg32 = config(storage, 'f32', f'{storage}-f32')
                f32 = solve(NEW, cfg32, label+'-new32', 1)
                f32t8 = solve(NEW, cfg32, f'{storage}-t8-new32', 8)
                unspecified = solve(NEW, cfg_old, label+'-unspecified', 1)
                compare(f32, unspecified, label+'-default')
                compare(f32, f32t8, label+'-f32-threads', ignore_run=True)
                compare(f32, new, label+'-precision-diff', ignore=True, different=True)
                # Check the entire raw state through fingerprints too (all arena scales included).
                left = run(VERIFY, ['checkpoint', f32 / 'checkpoint.ckpt'], label+'-state1')
                right = run(VERIFY, ['checkpoint', f32t8 / 'checkpoint.ckpt'], label+'-state8')
                assert left.read_bytes() == right.read_bytes()
                record(step='state', label=label, bit_equal=True, result=left.read_text().strip())
                for path in [f32, f32t8, unspecified]:
                    clean(path)
                if storage == 'f32':
                    global OLD_WALL
                    OLD_WALL = json.loads((old / 'run.json').read_text())['wallSecs']
            clean(old)
            clean(new)
    cfg = config('f32', None, 'resume-old', BASE.replace('max_iterations = 6', 'max_iterations = 20').replace('check_every = 3', 'check_every = 4') + f'\n[run]\nmax_time = "{max(0.001, OLD_WALL * 0.7):.6f}s"\ncheckpoint_interval = "0.001s"\n')
    old = solve(OLD, cfg, 'resume-old', 1)
    summary = json.loads((old / 'run.json').read_text())
    before = summary['iterations']
    assert 0 < before < 20, before
    assert 'cfr_precision' not in (old / 'run.toml').read_text()
    new = AREA / 'compare-resume-new'
    run(NEW, ['resume', old, '--out', new, '--max-time', '1h', '--threads', 1], 'resume-new')
    assert json.loads((new / 'run.json').read_text())['iterations'] == 20
    assert 'cfr_precision = "f32"' in (new / 'run.toml').read_text()
    record(step='resume', old_iterations=before, new_iterations=20, effective_f32=True)
    clean(old)
    clean(new)
    log = run(NEW, ['validate', AREA/'f32-old.toml', '--show-effective'], 'validate-default')
    assert 'cfr_precision = "f32"' in log.read_text(encoding='utf-8')
    record(step='validate', effective_f32=True)
    record(step='complete', free_gib=shutil.disk_usage(ROOT).free / 1024**3)

if __name__ == '__main__':
    main()
