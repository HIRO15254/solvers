"""Three finite, sequential adoption checks; resource ownership stays in run_supervised.py."""
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[4]
OUT = Path('E:/codex-work/solvers/r1-context-adoption-20260926')
BIN = Path('C:/Users/PC_User/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin')
TARGET = ROOT / 'target/r1-local-tests'


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def pin(path):
    data = path.read_bytes()
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def save(name, value):
    (OUT / name).write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)


def main():
    started = time.monotonic()
    deadline = started + 450
    OUT.mkdir(exist_ok=False)
    (OUT / 'records').mkdir()
    paths = git('ls-files', '-z', 'Cargo.toml', 'Cargo.lock', '.cargo/config.toml', 'crates').decode().strip('\0').split('\0')
    assert len(paths) == 199, len(paths)
    source = {p: pin(ROOT / p) for p in paths}
    patch = git('diff', '--binary', 'HEAD')
    (OUT / 'dirty.patch').write_bytes(patch)
    changed = git('diff', '--name-only', 'HEAD', '--', *paths).decode().splitlines()
    for p in changed:
        dest = OUT / 'changed-source' / p
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes((ROOT / p).read_bytes())
    environment = {
        'CARGO_BUILD_JOBS': '1', 'CARGO_PROFILE_DEV_DEBUG': '0',
        'CARGO_PROFILE_TEST_DEBUG': '0', 'CARGO_INCREMENTAL': '0',
        'CARGO_PROFILE_DEV_INCREMENTAL': 'false', 'CARGO_PROFILE_TEST_INCREMENTAL': 'false',
        'CARGO_TARGET_DIR': str(TARGET), 'CARGO_NET_OFFLINE': 'true',
        'RUSTFLAGS': '', 'RAYON_NUM_THREADS': '1', 'RUST_TEST_THREADS': '1',
        'RUSTC': str(BIN / 'rustc.exe'), 'RUSTFMT': str(BIN / 'rustfmt.exe'),
        'PATH': str(BIN) + os.pathsep + os.environ['PATH'],
    }
    removed = ['CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER']
    env = os.environ.copy()
    for key in removed:
        env.pop(key, None)
    env.update(environment)
    tools = [BIN / (n + '.exe') for n in ['cargo', 'rustc', 'rustfmt', 'cargo-fmt', 'cargo-clippy', 'clippy-driver']]
    tools += [Path(sys.executable), ROOT / 'tools/run_supervised.py', Path(__file__).resolve()]
    commands = {
        'fmt': [str(BIN / 'cargo.exe'), 'fmt', '--all', '--check'],
        'clippy': [str(BIN / 'cargo.exe'), 'clippy', '--workspace', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings'],
        'formats-tests': [str(BIN / 'cargo.exe'), 'test', '--locked', '--offline', '-p', 'formats', '--', '--test-threads=1'],
    }
    plan = {'schema': 'r1.context-adoption-plan/v1', 'created_at': now(),
            'source_commit': git('rev-parse', 'HEAD').decode().strip(), 'source_files': source,
            'dirty_patch': pin(OUT / 'dirty.patch'), 'changed_source_paths': changed,
            'environment': environment, 'removed_environment_keys': removed,
            'tools': {str(p): pin(p) for p in tools}, 'commands': commands,
            'cwd': str(ROOT), 'output_directory': str(OUT),
            'limits': {'stage_seconds': 120, 'overall_seconds': 450, 'confirmation_seconds': 30,
                       'memory_bytes': 768 * 1024**2, 'free_memory_bytes': 3 * 1024**3,
                       'disk_bytes': 4 * 1024**3, 'poll_seconds': 0.1, 'grace_seconds': 5, 'kill_seconds': 5},
            'confirmation_policy': 'At most one total; only Cargo 0, descendant-leftover failure, sole AttachConsole graceful error and complete cleanup; identical command/source/tool and target executable/DLL bytes required.',
            'disk_free_before': {str(p): shutil.disk_usage(p).free for p in [ROOT, OUT]},
            'scope': 'Warm Windows debug incremental checks on a shared host; no full workspace tests or performance certification.'}
    save('plan.json', plan)
    result = {'schema': 'r1.context-adoption-result/v1', 'started_at': now(), 'state': 'running', 'stages': []}
    save('result.json', result)
    used_confirmation = False

    def source_now():
        return {p: pin(ROOT / p) for p in paths}

    def run(label, command, seconds):
        assert source_now() == source, 'source changed before stage'
        assert all(pin(Path(p)) == v for p, v in plan['tools'].items()), 'tool changed'
        assert shutil.disk_usage(OUT).free >= 4 * 1024**3
        left = deadline - time.monotonic() - 12
        if left < seconds:
            raise RuntimeError('insufficient remaining overall time; no stage started')
        prefix = OUT / 'records' / label
        argv = [sys.executable, str(ROOT / 'tools/run_supervised.py'), '--record', str(prefix) + '.json',
                '--stdout', str(prefix) + '.stdout.log', '--stderr', str(prefix) + '.stderr.log',
                '--samples', str(prefix) + '.samples.jsonl', '--cwd', str(ROOT),
                '--timeout-seconds', str(seconds), '--grace-seconds', '5', '--kill-wait-seconds', '5',
                '--poll-seconds', '0.1', '--memory-limit-bytes', str(768 * 1024**2),
                '--min-free-memory-bytes', str(3 * 1024**3), '--disk-reserve-bytes', str(4 * 1024**3),
                '--disk-path', str(TARGET)]
        for p in [OUT / 'plan.json', *tools, *(ROOT / p for p in paths)]:
            argv += ['--identity-file', str(p)]
        process = subprocess.run([*argv, '--', *command], cwd=ROOT, env=env, capture_output=True)
        record = json.loads(Path(str(prefix) + '.json').read_text())
        entry = {'label': label, 'supervisor_exit_code': process.returncode,
                 'state': record['state'], 'child_exit_code': record['child_exit_code'],
                 'elapsed_seconds': record.get('elapsed_seconds'), 'stop_reason': record['stop_reason']}
        result['stages'].append(entry)
        save('result.json', result)
        print(json.dumps(entry), flush=True)
        assert source_now() == source and record['identity_unchanged'], 'source or identity changed'
        return record

    try:
        for label, command in commands.items():
            record = run(label, command, 120)
            if record['state'] == 'completed' and record['supervisor_exit_code'] == record['child_exit_code'] == 0:
                continue
            eligible = (not used_confirmation and record['state'] == 'failed' and record['child_exit_code'] == 0
                        and record['supervisor_exit_code'] == 1 and record['stop_reason'] == 'descendants_after_root_exit'
                        and record['cleanup_complete'] and len(record['errors']) == 1
                        and record['errors'][0]['where'] == 'graceful_signal'
                        and 'AttachConsole' in record['errors'][0]['message'])
            if not eligible:
                raise RuntimeError(label + ' failed; no further stage permitted')
            used_confirmation = True
            binaries = {str(p.relative_to(TARGET)): pin(p) for p in TARGET.rglob('*') if p.suffix.lower() in ('.exe', '.dll') and p.is_file()}
            save(label + '-confirmation-binaries-before.json', binaries)
            confirm = run(label + '-cleanup-confirmation', command, 30)
            after = {str(p.relative_to(TARGET)): pin(p) for p in TARGET.rglob('*') if p.suffix.lower() in ('.exe', '.dll') and p.is_file()}
            save(label + '-confirmation-binaries-after.json', after)
            assert binaries == after, 'compiled executable/DLL changed during confirmation'
            if not (confirm['state'] == 'completed' and confirm['supervisor_exit_code'] == confirm['child_exit_code'] == 0):
                raise RuntimeError(label + ' confirmation failed; no retry')
        result['state'] = 'checks_completed'
    except Exception as error:
        result['state'] = 'failed'
        result['error'] = str(error)
    finally:
        after = source_now()
        save('source-after.json', {'checked_at': now(), 'source_files': after, 'matches_before': after == source})
        result.update(ended_at=now(), overall_elapsed_seconds=time.monotonic() - started,
                      source_unchanged=after == source, confirmation_used=used_confirmation)
        save('result.json', result)
    print(json.dumps(result), flush=True)
    return 0 if result['state'] == 'checks_completed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
