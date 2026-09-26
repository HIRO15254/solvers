"""Finite Linux validation of one immutable range/scaling source snapshot."""
import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import time

sys.dont_write_bytecode = True


def pin(path):
    path = Path(path)
    data = path.read_bytes()
    return {'path': str(path.resolve()), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def save(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + '\n')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--target', type=Path, required=True)
    parser.add_argument('--deadline-utc', required=True)
    parser.add_argument('--name', default='validation')
    parser.add_argument('--build-only', action='store_true',
        help='fresh native release build after changing measurement CPU; not workspace validation')
    args = parser.parse_args()
    root, target = args.root.resolve(), args.target.resolve()
    assert re.fullmatch(r'[a-z][a-z0-9-]*', args.name)
    source, out = root / 'source', root / args.name
    manifest_path = root / 'source-candidate-manifest.json'
    manifest = json.loads(manifest_path.read_text())
    expected = {r['path']: {k: r[k] for k in ('bytes', 'sha256')} for r in manifest['files']}
    def verify_source():
        actual = {p.relative_to(source).as_posix(): {k: v for k, v in pin(p).items() if k != 'path'}
                  for p in source.rglob('*') if p.is_file()}
        assert actual == expected, 'source snapshot changed'
    verify_source()
    assert not target.exists(), 'fresh native build target required'
    target.mkdir(parents=True)
    out.mkdir()
    deadline = dt.datetime.fromisoformat(args.deadline_utc.replace('Z', '+00:00')).timestamp()
    assert 0 < deadline - time.time() < 6 * 3600
    toolchain = Path('/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin')
    os.environ.update(CARGO_HOME='/opt/r1/cargo', RUSTUP_HOME='/opt/r1/rustup',
        RUSTUP_TOOLCHAIN='1.97.0', CARGO_BUILD_JOBS='2', RAYON_NUM_THREADS='1',
        RUST_TEST_THREADS='2', CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0',
        RUSTC=str(toolchain / 'rustc'), RUSTDOC=str(toolchain / 'rustdoc'),
        PATH=str(toolchain) + ':' + os.environ['PATH'])
    for name in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER'):
        assert name not in os.environ, 'unexpected build override'
    boot = Path('/proc/sys/kernel/random/boot_id').read_text().strip()
    cgroup_path = Path('/sys/fs/cgroup') / Path('/proc/self/cgroup').read_text().strip().split('::', 1)[1].lstrip('/')
    memory_max = cgroup_path.joinpath('memory.max').read_text().strip()
    assert memory_max != 'max' and int(memory_max) <= 12 * 1024**3, '12 GiB outer memory limit required'
    state = {'schema': 'r1.range-scaling-validation/v1', 'status': 'running',
        'mode': 'release-build-only' if args.build_only else 'full-validation',
        'source_manifest': pin(manifest_path), 'source_archive': pin(root / 'source-candidate.tar.gz'),
        'source_root': str(source), 'target': str(target), 'deadline_utc': args.deadline_utc,
        'boot_id': boot, 'lscpu': json.loads(subprocess.check_output(['lscpu', '-J'])),
        'cgroup': {'path': str(cgroup_path), 'memory_max': memory_max,
                   'cpu_limits': {str(p): (p / 'cpu.max').read_text().strip()
                       for p in (cgroup_path, *cgroup_path.parents)
                       if p.is_relative_to('/sys/fs/cgroup') and (p / 'cpu.max').is_file()},
                   'allowed_cpus': sorted(os.sched_getaffinity(0))},
        'environment': {key: os.environ[key] for key in ('CARGO_HOME','RUSTUP_HOME','RUSTUP_TOOLCHAIN','CARGO_BUILD_JOBS','RAYON_NUM_THREADS','RUST_TEST_THREADS','CARGO_INCREMENTAL','CARGO_PROFILE_DEV_DEBUG','CARGO_PROFILE_TEST_DEBUG','RUSTC','RUSTDOC')},
        'tools': {name: pin(toolchain / name) for name in ('cargo','rustc','rustdoc','clippy-driver','rustfmt')},
        'runner': pin(__file__), 'stages': []}
    save(out / 'result.json', state)
    spec = importlib.util.spec_from_file_location('range_supervisor', source / 'tools/run_supervised.py')
    supervisor = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(supervisor)
    cargo = str(toolchain / 'cargo')
    def cmd(*values):
        return [cargo, *values]
    stages = [
        ('toolchain', 30, [str(toolchain / 'rustc'), '-Vv']),
        ('fmt', 60, cmd('fmt','--all','--check')),
        ('clippy', 1800, cmd('clippy','--workspace','--all-targets','--target-dir',str(target),'--','-D','warnings')),
        ('workspace-tests', 2400, cmd('test','--workspace','--no-fail-fast','--target-dir',str(target))),
        ('docs', 60, [sys.executable, '-B', 'tools/check_docs.py']),
        ('release-example', 1800, cmd('build','--release','-p','cli','--example','hu_scaling_bench','--target-dir',str(target))),
        ('release-oracle', 1800, cmd('test','--release','-p','holdem','--test','oracle_diff','--target-dir',str(target),'--','--include-ignored')),
        ('release-river-resolve', 1800, cmd('test','--release','-p','cli','--lib','--target-dir',str(target),'sol::tests::river_resolve_accuracy','--','--exact','--ignored')),
    ]
    if args.build_only:
        stages = [stage for stage in stages if stage[0] in ('toolchain', 'release-example')]
    try:
        for label, limit, argv in stages:
            verify_source()
            assert Path('/proc/sys/kernel/random/boot_id').read_text().strip() == boot
            assert deadline - time.time() > limit + 30, 'insufficient deadline for bounded stage'
            stage_out = out / 'stages' / label
            stage = {'label': label, 'argv': argv, 'timeout_seconds': limit, 'status': 'running'}
            state['stages'].append(stage)
            save(out / 'result.json', state)
            stage_out.mkdir(parents=True)
            arguments = ['--record', str(stage_out / 'supervisor.json'), '--cwd', str(source), '--timeout-seconds', str(limit),
                '--memory-limit-bytes', str(10 * 1024**3), '--min-free-memory-bytes', str(1024**3),
                '--disk-reserve-bytes', str(4 * 1024**3), '--disk-path', str(root),
                '--grace-seconds','5','--kill-wait-seconds','5','--poll-seconds','0.1',
                '--identity-file',str(manifest_path),'--identity-file',str(root / 'source-candidate.tar.gz'),
                '--identity-file', str(toolchain / 'cargo'), '--identity-file', str(toolchain / 'rustc'),
                '--identity-file', str(__file__)]
            code = supervisor.main(arguments + ['--', *argv])
            stage.update(status='passed' if code == 0 else 'failed', supervisor_exit=code,
                record=pin(stage_out / 'supervisor.json'))
            verify_source()
            save(out / 'result.json', state)
            print(json.dumps({'stage':label,'status':stage['status']}), flush=True)
            assert code == 0, 'stage failed: ' + label
            if label == 'release-example':
                state['binary'] = pin(target / 'release/examples/hu_scaling_bench')
        state['status'] = 'completed'
    except BaseException as error:
        state.update(status='failed', error=repr(error))
        raise
    finally:
        save(out / 'result.json', state)


if __name__ == '__main__':
    main()
