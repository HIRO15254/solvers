#!/bin/bash
# Run inside the caller's finite systemd service/cgroup (KillMode=control-group,
# RuntimeMaxSec <= 7500, TimeoutStopSec <= 15, MemoryMax <= 48G). The parent owns
# the Spot STOP lifetime, retained boot disk, evidence collection and cleanup.
# This builds quality-audit examples only; source03 remains the paired benchmark.
set -euo pipefail
export CARGO_HOME=/opt/r1/cargo
export RUSTUP_HOME=/opt/r1/rustup
export RUSTUP_TOOLCHAIN=1.97.0
export CARGO_BUILD_JOBS=4
export CARGO_INCREMENTAL=0
# Supervisor canonicalizes the child executable path. Rustup proxy symlinks
# would consequently lose argv[0]=cargo, so use the actual toolchain binaries.
export PATH=/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin:/opt/r1/cargo/bin:$PATH

python3 - "$0" <<'PY'
import datetime as dt
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tarfile
import time

BASE = Path('/opt/r1')
CURRENT = BASE / 'audit-source'
BASELINE = BASE / 'baseline-audit'
TOOLS = BASE / 'audit-tools'
OUTPUT = BASE / 'audit-pair-build'
CURRENT_TARGET = BASE / 'target/current'
BASELINE_TARGET = BASE / 'target/baseline-audit'
SCRIPT = Path(sys.argv[1]).resolve(strict=True)
ARCHIVES = {
    BASE / 'audit-source.tar.gz': 'f241167a9c765b839cbe560ec66c9c490a0b0193d8f23ba8de768c65eaaf043c',
    BASE / 'baseline-source.tar.gz': 'fdd8c1c014a94c70b56efbd79a36c18f6f1634c20aaf9062660ee3a6133a0197',
}
TOOL_FILES = (
    'apply_baseline_audit.py', 'source-pins.json', 'audit_shared.rs.in',
    'audit_example.rs.in', 'baseline_adapter.rs.in', 'test_apply_baseline_audit.py', 'README.md',
)
STARTED = time.monotonic()
DEADLINE = STARTED + 7200
GRACE = 5
KILL_WAIT = 5
state = {'schema': 'r1.audit-pair-build/v1', 'status': 'starting',
         'started_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
         'dispatch_budget_seconds': 7200, 'outer_cgroup_required': True,
         'purpose': 'saved_profile_quality_only', 'stages': [], 'identities': {}}


def identity(path):
    path = path.resolve(strict=True)
    before = path.stat()
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    after = path.stat()
    stamp = lambda item: (item.st_dev, item.st_ino, item.st_size, item.st_mtime_ns, item.st_ctime_ns)
    if stamp(before) != stamp(after):
        raise RuntimeError(f'file changed while hashing: {path}')
    return {'path': str(path), 'sha256': digest.hexdigest(), 'bytes': after.st_size}


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + '.tmp')
    with temporary.open('w', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, indent=2, sort_keys=True, allow_nan=False)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def save_state():
    state['elapsed_seconds'] = time.monotonic() - STARTED
    write_json(OUTPUT / 'result.json', state)


def build_input(name):
    parts = PurePosixPath(name).parts
    return (name in ('Cargo.toml', 'Cargo.lock')
            or (parts[0] == 'crates' and (name.endswith('.rs') or parts[-1] == 'Cargo.toml'))
            or (parts[0] == '.cargo' and name.endswith('.toml')))


def archive_manifest(archive, extracted=None):
    """Validate regular archive members; compare current source bytes before use."""
    records, total = {}, 0
    with tarfile.open(archive, 'r:gz') as stream:
        for member in stream:
            name = PurePosixPath(member.name)
            if name.is_absolute() or '..' in name.parts or '\\' in member.name or not name.parts:
                raise ValueError(f'unsafe archive member: {member.name}')
            if member.isdir():
                continue
            if not member.isfile() or member.name in records:
                raise ValueError(f'nonregular or duplicate archive member: {member.name}')
            total += member.size
            if member.size > 32 * 1024**2 or total > 256 * 1024**2:
                raise ValueError('source archive exceeds bounded source size')
            with stream.extractfile(member) as source:
                digest = hashlib.sha256(source.read()).hexdigest()
            records[member.name] = {'sha256': digest, 'bytes': member.size}
            if extracted is not None:
                target = extracted / member.name
                if target.is_symlink() or not target.resolve().is_relative_to(extracted.resolve()):
                    raise ValueError(f'unsafe extracted source: {target}')
                actual = identity(target)
                if actual['sha256'] != digest or actual['bytes'] != member.size:
                    raise ValueError(f'archive/source mismatch: {target}')
    if extracted is not None:
        actual = set()
        for prefix in ('crates', '.cargo'):
            base = extracted / prefix
            if base.is_symlink():
                raise ValueError(f'source directory is a symlink: {base}')
            for path in base.rglob('*'):
                if path.is_symlink():
                    raise ValueError(f'source symlink: {path}')
                relative = path.relative_to(extracted).as_posix()
                if path.is_file() and build_input(relative):
                    actual.add(relative)
        actual.update(name for name in ('Cargo.toml', 'Cargo.lock') if (extracted / name).is_file())
        expected = {name for name in records if build_input(name)}
        if actual != expected:
            raise ValueError(f'extracted build-input set differs: {sorted(actual ^ expected)}')
    return records


def stage(name, argv, timeout, cwd=CURRENT, target=CURRENT_TARGET):
    # Never shorten a later stage's timeout to make it fit. Hashing/OS stalls
    # still require the finite outer cgroup, as with the campaign supervisor.
    if DEADLINE - time.monotonic() < timeout + GRACE + KILL_WAIT:
        raise RuntimeError(f'insufficient dispatch budget for {name}')
    for expected in immutable:
        if identity(Path(expected['path'])) != expected:
            raise RuntimeError(f'build input/tool changed before {name}: {expected["path"]}')
    directory = OUTPUT / name
    directory.mkdir()
    entry = {'name': name, 'argv': [str(arg) for arg in argv], 'cwd': str(cwd),
             'cargo_target_dir': str(target), 'timeout_seconds': timeout, 'status': 'running'}
    state['stages'].append(entry)
    state['status'] = 'running'
    save_state()
    os.environ['CARGO_TARGET_DIR'] = str(target)
    arguments = ['--record', str(directory / 'supervisor.json'),
                 '--stdout', str(directory / 'stdout.log'), '--stderr', str(directory / 'stderr.log'),
                 '--cwd', str(cwd), '--timeout-seconds', str(timeout),
                 '--grace-seconds', str(GRACE), '--kill-wait-seconds', str(KILL_WAIT),
                 '--poll-seconds', '0.25', '--memory-limit-bytes', str(40 * 1024**3),
                 '--min-free-memory-bytes', str(8 * 1024**3),
                 '--disk-reserve-bytes', str(10 * 1024**3), '--disk-path', str(BASE)]
    for expected in immutable:
        arguments.extend(['--identity-file', expected['path']])
    if DEADLINE - time.monotonic() < timeout + GRACE + KILL_WAIT:
        raise RuntimeError(f'insufficient dispatch budget after identity check for {name}')
    code = supervisor.main([*arguments, '--', *entry['argv']])
    record_path = directory / 'supervisor.json'
    record = json.loads(record_path.read_text()) if record_path.is_file() else {}
    entry.update(status=record.get('state', 'record_missing'), supervisor_exit_code=code,
                 child_exit_code=record.get('child_exit_code'), stop_reason=record.get('stop_reason'),
                 cleanup_complete=record.get('cleanup_complete'), identity_unchanged=record.get('identity_unchanged'))
    if record_path.is_file():
        entry['record'] = identity(record_path)
    save_state()
    print(json.dumps(entry), flush=True)
    if code or entry['status'] != 'completed' or entry['child_exit_code'] != 0 or entry['cleanup_complete'] is not True or entry['identity_unchanged'] is not True:
        raise RuntimeError(f'{name} failed; inspect retained supervisor/stdout/stderr')
    return directory


if OUTPUT.exists() or BASELINE.exists() or BASELINE_TARGET.exists():
    raise SystemExit('refusing to overwrite build evidence or a baseline audit source/target')
if not CURRENT.is_dir() or CURRENT.is_symlink() or TOOLS.is_symlink():
    raise SystemExit('current source/audit tools must be ordinary supplied directories')
OUTPUT.mkdir()
save_state()
try:
    for name in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER'):
        if os.environ.get(name):
            raise RuntimeError(f'unpinned compiler override: {name}')
    for archive, expected in ARCHIVES.items():
        actual = identity(archive)
        if actual['sha256'] != expected:
            raise ValueError(f'source archive SHA-256 mismatch: {archive}')
        state['identities'][archive.name] = actual
    current_files = archive_manifest(BASE / 'audit-source.tar.gz', CURRENT)
    baseline_files = archive_manifest(BASE / 'baseline-source.tar.gz')
    write_json(OUTPUT / 'current-source-files.json', current_files)
    write_json(OUTPUT / 'baseline-source-files.json', baseline_files)
    immutable = [identity(SCRIPT), *[identity(path) for path in ARCHIVES],
                 *[identity(TOOLS / name) for name in TOOL_FILES],
                 identity(CURRENT / 'tools/run_supervised.py')]
    state['identities']['immutable_inputs'] = immutable
    state['cargo_environment'] = {name: value for name, value in os.environ.items()
                                  if name.startswith(('CARGO_', 'RUSTUP_', 'RUSTC'))}
    state['cgroup'] = Path('/proc/self/cgroup').read_text()
    state['boot_id'] = Path('/proc/sys/kernel/random/boot_id').read_text().strip()
    save_state()
    specification = importlib.util.spec_from_file_location('r1_audit_build_supervisor', CURRENT / 'tools/run_supervised.py')
    supervisor = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(supervisor)
    # Refuse archive timestamp reuse: Cargo must see current workspace sources
    # newer than any cached local crate outputs. Content identities stay fixed.
    for name in current_files:
        if build_input(name):
            os.utime(CURRENT / name, None)
    compiler = stage('00-toolchain', ['/bin/bash', '-euo', 'pipefail', '-c',
        'rustc -Vv; cargo -V; rustup which rustc; rustup which cargo; uname -a; lscpu'], 60)
    compiler_text = (compiler / 'stdout.log').read_text()
    if 'release: 1.97.0\n' not in compiler_text:
        raise RuntimeError('the effective compiler is not Rust 1.97.0')
    compiler_paths = [Path(line) for line in compiler_text.splitlines()
                      if line.startswith('/opt/r1/rustup/toolchains/') and Path(line).is_file()]
    if len(compiler_paths) != 2:
        raise RuntimeError('effective rustc/cargo paths were not recorded')
    cargo_binary = Path('/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/cargo')
    if cargo_binary not in compiler_paths or cargo_binary.is_symlink():
        raise RuntimeError('rustup which cargo does not identify the pinned real cargo executable')
    state['identities']['compiler_binaries'] = [identity(path) for path in compiler_paths]
    save_state()
    stage('01-fmt', [cargo_binary, 'fmt', '--all', '--check'], 300)
    stage('02-clippy', [cargo_binary, 'clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings'], 1800)
    stage('03-workspace-test', [cargo_binary, 'test', '--locked', '--workspace', '--', '--test-threads=2'], 1800)
    stage('04-tools-python', ['python3', '-m', 'unittest', 'discover', '-s', 'tools/tests', '-v'], 300)
    stage('05-pipeline-python', ['python3', '-m', 'unittest', 'discover', '-s',
          'experiments/hu-postflop-r1/pipeline', '-p', 'test_run_campaign.py', '-v'], 300)
    stage('06-current-release', [cargo_binary, 'build', '--locked', '--release', '-p', 'cli',
          '--bin', 'solvers', '--example', 'hu_saved_profile_audit'], 1800)
    current_binary = CURRENT_TARGET / 'release/examples/hu_saved_profile_audit'
    state['identities']['current_example'] = identity(current_binary)
    state['identities']['source06_solvers_not_paired_benchmark'] = identity(CURRENT_TARGET / 'release/solvers')
    write_json(OUTPUT / 'current-build-complete.json', state['identities'])
    save_state()

    BASELINE.mkdir()
    BASELINE_TARGET.mkdir(parents=True)
    (BASELINE_TARGET / 'release').mkdir()
    # Reflink or ordinary copies preserve independence; never use hard links.
    # Only reproducible release caches are seeded, not measured executables.
    cache_paths = [CURRENT_TARGET / 'release' / name for name in ('deps', 'build', '.fingerprint')]
    for path in cache_paths:
        if not path.is_dir() or path.is_symlink():
            raise RuntimeError(f'missing/invalid release cache: {path}')
    stage('07-seed-baseline-cache', ['cp', '-a', '--reflink=auto', *cache_paths,
          BASELINE_TARGET / 'release'], 300)
    write_json(OUTPUT / 'cache-seed.json', {
        'from_current_source_archive': state['identities']['audit-source.tar.gz'],
        'compiler_binaries': state['identities']['compiler_binaries'],
        'copied_directories': [str(path) for path in cache_paths],
        'destination': str(BASELINE_TARGET / 'release'), 'copy_mode': 'reflink_auto_no_hardlinks'})
    # Extract after the copy, with present mtimes, so baseline workspace crates
    # cannot silently reuse current workspace binaries from copied fingerprints.
    stage('08-extract-baseline', ['tar', '--touch', '--no-same-owner', '-xzf',
          BASE / 'baseline-source.tar.gz', '-C', BASELINE], 300)
    adapter = TOOLS / 'apply_baseline_audit.py'
    common = ['python3', str(adapter), '--root', str(BASELINE), '--reference-root', str(CURRENT)]
    stage('09-baseline-source-check', [*common, '--check'], 120)
    stage('10-baseline-adapter-apply', [*common, '--apply'], 120)
    stage('11-baseline-adapter-verify', [*common, '--verify'], 120)
    applied = BASELINE / 'r1-saved-profile-source-manifest.json'
    state['identities']['baseline_adapter_manifest'] = identity(applied)
    immutable.append(identity(applied))
    stage('12-baseline-example-release', [cargo_binary, 'build', '--locked', '--release', '-p', 'cli',
          '--example', 'hu_saved_profile_audit'], 1800, cwd=BASELINE, target=BASELINE_TARGET)
    stage('13-baseline-final-verify', [*common, '--verify'], 120)
    if archive_manifest(BASE / 'audit-source.tar.gz', CURRENT) != current_files:
        raise RuntimeError('current source changed during validation/build')
    state['identities']['baseline_example'] = identity(BASELINE_TARGET / 'release/examples/hu_saved_profile_audit')
    state['status'] = 'completed'
    state['ended_utc'] = dt.datetime.now(dt.timezone.utc).isoformat()
except BaseException as error:
    state['status'] = 'failed'
    state['error'] = f'{type(error).__name__}: {error}'
    state['ended_utc'] = dt.datetime.now(dt.timezone.utc).isoformat()
    raise
finally:
    save_state()
    retained = {str(path.relative_to(OUTPUT)): identity(path) for path in sorted(OUTPUT.rglob('*'))
                if path.is_file() and path.name not in ('retained-files.json',)}
    write_json(OUTPUT / 'retained-files.json', retained)
if state['status'] == 'completed':
    write_json(OUTPUT / 'complete.json', {'status': 'completed', 'result': identity(OUTPUT / 'result.json'),
               'retained_files': identity(OUTPUT / 'retained-files.json'),
               'current_example': state['identities']['current_example'],
               'baseline_example': state['identities']['baseline_example']})
    print(json.dumps({'status': 'completed', 'evidence': str(OUTPUT)}), flush=True)
PY
