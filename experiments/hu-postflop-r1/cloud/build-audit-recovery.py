#!/usr/bin/env python3
"""Rebuild VM06 quality-audit examples after a Spot restart changed its CPU.

Run in a fresh finite systemd cgroup: MemoryMax=48G, RuntimeMaxSec=7500,
TimeoutStopSec=15, KillMode=control-group. The owner retains the original
20:09:11Z VM deadline and all prior evidence. No original evidence is edited.
No solve/performance measurement runs here; outputs are quality-audit tools.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import sys
import tarfile
import time

BASE = Path('/opt/r1')
CURRENT = BASE / 'audit-source'
BASELINE = BASE / 'baseline-audit'
TOOLS = BASE / 'audit-tools'
PRIOR = BASE / 'audit-pair-build'
OUTPUT = BASE / 'audit-pair-recovery'
OLD_TARGET = BASE / 'target/baseline-audit'
OLD_CURRENT_TARGET = BASE / 'target/current'
TARGET = BASE / 'target/baseline-audit-recovery'
CURRENT_TARGET = BASE / 'target/current-recovery'
TOOLCHAIN = BASE / 'rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin'
CARGO = TOOLCHAIN / 'cargo'
ARCHIVES = {
    'audit-source.tar.gz': 'f241167a9c765b839cbe560ec66c9c490a0b0193d8f23ba8de768c65eaaf043c',
    'baseline-source.tar.gz': 'fdd8c1c014a94c70b56efbd79a36c18f6f1634c20aaf9062660ee3a6133a0197',
}
TOOLCHAIN_COMMAND = 'rustc -Vv; cargo -V; rustup which rustc; rustup which cargo; uname -a; lscpu'


def identity(path):
    path = Path(path).resolve(strict=True)
    before = path.stat()
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    after = path.stat()
    stamp = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
    if stamp(before) != stamp(after):
        raise RuntimeError(f'file changed while hashing: {path}')
    return {'path': str(path), 'sha256': digest.hexdigest(), 'bytes': after.st_size}


def verify(expected):
    if identity(expected['path']) != expected:
        raise ValueError(f'evidence identity changed: {expected["path"]}')


def text(path):
    data = Path(path).read_bytes()
    if b'\0' in data:
        raise ValueError(f'NUL in evidence; refusing repair: {path}')
    return data.decode('utf-8')


def read(path):
    def duplicate_keys(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f'duplicate JSON key in {path}: {key}')
            result[key] = value
        return result
    def invalid_constant(value):
        raise ValueError(f'nonfinite JSON constant in {path}: {value}')
    return json.loads(text(path), object_pairs_hook=duplicate_keys, parse_constant=invalid_constant)


def write(path, value):
    temporary = path.with_suffix(path.suffix + '.tmp')
    with temporary.open('w', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, indent=2, sort_keys=True, allow_nan=False)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def build_input(name):
    parts = PurePosixPath(name).parts
    return (name in ('Cargo.toml', 'Cargo.lock') or
            (parts[0] == 'crates' and (name.endswith('.rs') or parts[-1] == 'Cargo.toml')) or
            (parts[0] == '.cargo' and name.endswith('.toml')))


def archive_manifest(archive, extracted=None):
    records, total = {}, 0
    with tarfile.open(archive, 'r:gz') as stream:
        for member in stream:
            name = PurePosixPath(member.name)
            if name.is_absolute() or '..' in name.parts or '\\' in member.name or not name.parts:
                raise ValueError(f'unsafe archive member: {member.name}')
            if member.isdir():
                continue
            if not member.isfile() or member.name in records:
                raise ValueError(f'nonregular/duplicate archive member: {member.name}')
            total += member.size
            if member.size > 32 * 1024**2 or total > 256 * 1024**2:
                raise ValueError('source archive exceeds bounds')
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
            if (extracted / prefix).is_symlink():
                raise ValueError('source directory symlink')
            for path in (extracted / prefix).rglob('*'):
                if path.is_symlink():
                    raise ValueError(f'source symlink: {path}')
                name = path.relative_to(extracted).as_posix()
                if path.is_file() and build_input(name):
                    actual.add(name)
        actual.update(name for name in ('Cargo.toml', 'Cargo.lock') if (extracted / name).is_file())
        if actual != {name for name in records if build_input(name)}:
            raise ValueError('current source build-input set differs from archive')
    return records


def cpu_identity(value):
    fields = {}
    for line in value.splitlines():
        if ':' in line:
            key, val = line.split(':', 1)
            if key.strip() in ('Architecture', 'CPU op-mode(s)', 'Vendor ID', 'Model name',
                               'CPU family', 'Model', 'Stepping', 'Flags'):
                key, val = key.strip(), val.strip()
                if key in fields:
                    raise ValueError(f'duplicate CPU field: {key}')
                fields[key] = sorted(val.split()) if key == 'Flags' else val
    if not {'Architecture', 'Vendor ID', 'Model name', 'CPU family', 'Model', 'Stepping', 'Flags'} <= fields.keys():
        raise ValueError('incomplete lscpu identity')
    return fields


def require_cgroup():
    lines = text('/proc/self/cgroup').splitlines()
    unified = [line[3:] for line in lines if line.startswith('0::')]
    if len(unified) != 1:
        raise ValueError('recovery requires a bounded cgroup v2 service')
    root = Path('/sys/fs/cgroup')
    group = root / unified[0].lstrip('/')
    limits = []
    for parent in (group, *group.parents):
        if not parent.is_relative_to(root):
            break
        # The cgroup v2 root has no memory.max on some hosts. Missing
        # controller files contribute no limit; a finite effective limit
        # remains mandatory across the extant ancestor files.
        limit_file = parent / 'memory.max'
        if not limit_file.exists():
            continue
        raw = text(limit_file).strip()
        if raw != 'max':
            limits.append(int(raw))
    if not limits or min(limits) > 48 * 1024**3:
        raise ValueError('outer cgroup must enforce MemoryMax <= 48 GiB')
    return {'path': str(group), 'effective_memory_max_bytes': min(limits)}


def expected_stages():
    common = ['python3', str(TOOLS / 'apply_baseline_audit.py'), '--root', str(BASELINE),
              '--reference-root', str(CURRENT)]
    cargo = str(CARGO)
    return [
        ('00-toolchain', ['/bin/bash', '-euo', 'pipefail', '-c', TOOLCHAIN_COMMAND], 60),
        ('01-fmt', [cargo, 'fmt', '--all', '--check'], 300),
        ('02-clippy', [cargo, 'clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings'], 1800),
        ('03-workspace-test', [cargo, 'test', '--locked', '--workspace', '--', '--test-threads=2'], 1800),
        ('04-tools-python', ['python3', '-m', 'unittest', 'discover', '-s', 'tools/tests', '-v'], 300),
        ('05-pipeline-python', ['python3', '-m', 'unittest', 'discover', '-s',
                                'experiments/hu-postflop-r1/pipeline', '-p', 'test_run_campaign.py', '-v'], 300),
        ('06-current-release', [cargo, 'build', '--locked', '--release', '-p', 'cli',
                                '--bin', 'solvers', '--example', 'hu_saved_profile_audit'], 1800),
        ('07-seed-baseline-cache', ['cp', '-a', '--reflink=auto',
            *[str(OLD_CURRENT_TARGET / 'release' / name) for name in ('deps', 'build', '.fingerprint')],
            str(OLD_TARGET / 'release')], 300),
        ('08-extract-baseline', ['tar', '--touch', '--no-same-owner', '-xzf',
                                str(BASE / 'baseline-source.tar.gz'), '-C', str(BASELINE)], 300),
        ('09-baseline-source-check', [*common, '--check'], 120),
        ('10-baseline-adapter-apply', [*common, '--apply'], 120),
        ('11-baseline-adapter-verify', [*common, '--verify'], 120),
    ]


def validate_previous(prior):
    if (prior.get('schema') != 'r1.audit-pair-build/v1'
            or prior.get('purpose') != 'saved_profile_quality_only'):
        raise ValueError('unsupported prior build evidence')
    expected_last = [str(CARGO), 'build', '--locked', '--release', '-p', 'cli',
                     '--example', 'hu_saved_profile_audit']
    specifications = expected_stages()
    excluded = {'12-baseline-example-release', '13-baseline-final-verify'}
    audit = {'verified_completed_stages': [item[0] for item in specifications],
             'excluded_stage_names': sorted(excluded), 'excluded_stage_success_accepted': False,
             'exclusion_reason': 'Old baseline release/final-verify are unnecessary for the fresh CPU rebuild. '
                                 'Their prior result statuses are historical claims only; corrupted bytes are retained without repair.',
             'excluded_text_evidence': []}
    if prior.get('status') == 'running' and len(prior['stages']) == 13:
        if (PRIOR / 'complete.json').exists():
            raise ValueError('running prior build conflicts with completion evidence')
        last = prior['stages'][-1]
        if (last['name'] != '12-baseline-example-release' or last['status'] != 'running'
                or last['argv'] != expected_last or last['cwd'] != str(BASELINE)
                or last['cargo_target_dir'] != str(OLD_TARGET) or last['timeout_seconds'] != 1800):
            raise ValueError('only stage 12 interruption can be recovered')
    elif prior.get('status') == 'completed' and len(prior['stages']) == 14:
        if [entry['name'] for entry in prior['stages'][12:]] != sorted(excluded):
            raise ValueError('unexpected historical baseline stage claims')
        audit['excluded_stage_claims'] = prior['stages'][12:]
    else:
        raise ValueError('expected completed stages 00..13 or interrupted stage 12 after completed 00..11')
    for entry, (name, argv, timeout) in zip(prior['stages'], specifications):
        cwd = BASELINE if name == '12-baseline-example-release' else CURRENT
        target = OLD_TARGET if name == '12-baseline-example-release' else OLD_CURRENT_TARGET
        if (entry['name'] != name or entry['argv'] != argv or entry['cwd'] != str(cwd)
                or entry['cargo_target_dir'] != str(target) or entry['timeout_seconds'] != timeout):
            raise ValueError(f'prior stage specification mismatch: {name}')
        if (entry['status'] != 'completed' or entry['supervisor_exit_code'] != 0
                or entry['child_exit_code'] != 0 or entry['stop_reason'] != 'completed'
                or entry['cleanup_complete'] is not True or entry['identity_unchanged'] is not True):
            raise ValueError(f'prior stage did not succeed: {name}')
        verify(entry['record'])
        if entry['record']['path'] != str(PRIOR / name / 'supervisor.json'):
            raise ValueError('prior record outside expected stage')
        record = read(entry['record']['path'])
        if (record['schema'] != 'solvers.supervised-run/v1' or record['state'] != 'completed'
                or record['supervisor_exit_code'] != 0 or record['child_exit_code'] != 0
                or record['stop_reason'] != 'completed' or record['cleanup_complete'] is not True
                or record['identity_unchanged'] is not True or record['argv'] != argv
                or record['cwd'] != str(cwd) or record['limits']['timeout_seconds'] != timeout
                or not record['identity_before'] or record['identity_before'] != record['identity_after']):
            raise ValueError(f'prior supervisor mismatch: {name}')
        if set(record['outputs']) != {'stdout', 'stderr', 'samples'}:
            raise ValueError(f'incomplete prior outputs: {name}')
        for item in [*record['identity_before'], *record['outputs'].values()]:
            verify(item)
    # Only source validation/current-build evidence is reused. Old baseline
    # stages 12/13 are byte-retained forensic evidence, never success evidence.
    for path in PRIOR.rglob('*'):
        if path.is_symlink():
            raise ValueError(f'prior evidence symlink: {path}')
        if path.is_file() and path.suffix in ('.json', '.jsonl', '.log'):
            if path.relative_to(PRIOR).parts[0] in excluded:
                data = path.read_bytes()
                record = {'identity': identity(path), 'nul_bytes': data.count(b'\0'),
                          'first_nul_offset': data.find(b'\0') if b'\0' in data else None,
                          'adopted_as_success_evidence': False}
                try:
                    content = text(path)
                    if path.suffix == '.json':
                        read(path)
                    elif path.suffix == '.jsonl':
                        for line in content.splitlines():
                            json.loads(line)
                    record['text_status'] = 'parseable'
                except (ValueError, UnicodeError) as error:
                    record['text_status'] = 'corrupted'
                    record['error'] = str(error)
                audit['excluded_text_evidence'].append(record)
                continue
            content = text(path)
            if path.suffix == '.json':
                read(path)
            elif path.suffix == '.jsonl':
                for line in content.splitlines():
                    json.loads(line)
    return audit


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--deadline-utc', default='2026-09-25T20:09:11Z')
    args = parser.parse_args(argv)
    deadline = dt.datetime.fromisoformat(args.deadline_utc.replace('Z', '+00:00')).timestamp()
    if deadline > dt.datetime.fromisoformat('2026-09-25T20:09:11+00:00').timestamp():
        raise ValueError('recovery cannot extend the original VM deadline')
    if sys.platform != 'linux' or any(path.exists() for path in (OUTPUT, CURRENT_TARGET, TARGET)):
        raise ValueError('Linux only; recovery output and both recovery targets must not exist')
    OUTPUT.mkdir()
    started = time.monotonic()
    state = {'schema': 'r1.audit-pair-build/v1', 'status': 'starting',
             'purpose': 'saved_profile_quality_only', 'started_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
             'recovery': {'kind': 'spot_stop_changed_cpu_fresh_release_rebuild', 'deadline_utc': args.deadline_utc,
                          'original_evidence_modified': False}, 'identities': {}, 'stages': []}

    def save():
        state['elapsed_seconds'] = time.monotonic() - started
        write(OUTPUT / 'result.json', state)

    save()
    try:
        for name in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER'):
            if os.environ.get(name):
                raise ValueError(f'unpinned compiler override: {name}')
        os.environ.update(CARGO_HOME=str(BASE / 'cargo'), RUSTUP_HOME=str(BASE / 'rustup'),
                          RUSTUP_TOOLCHAIN='1.97.0', CARGO_BUILD_JOBS='4', CARGO_INCREMENTAL='0',
                          CARGO_TARGET_DIR=str(TARGET), LC_ALL='C',
                          PATH=f'{TOOLCHAIN}:{BASE / "cargo/bin"}:' + os.environ.get('PATH', ''))
        state['recovery']['cgroup'] = require_cgroup()
        prior = read(PRIOR / 'result.json')
        state['recovery']['prior_evidence_assessment'] = validate_previous(prior)
        previous_identities = prior['identities']
        complete = read(PRIOR / 'current-build-complete.json')
        if not {'audit-source.tar.gz', 'baseline-source.tar.gz', 'compiler_binaries',
                'current_example', 'source06_solvers_not_paired_benchmark', 'immutable_inputs'} <= complete.keys():
            raise ValueError('incomplete prior current-build-complete identities')
        for key, value in complete.items():
            # The original builder appends the adapter manifest to the same
            # immutable-input list after writing current-build-complete.
            expected = previous_identities.get(key)
            if key == 'immutable_inputs':
                expected = expected[:len(value)]
            if expected != value:
                raise ValueError(f'current-build-complete identity mismatch: {key}')
        for name, digest in ARCHIVES.items():
            item = previous_identities[name]
            verify(item)
            if item['path'] != str(BASE / name) or item['sha256'] != digest:
                raise ValueError(f'unexpected source archive: {name}')
        for item in [*previous_identities['immutable_inputs'], *previous_identities['compiler_binaries'],
                     previous_identities['current_example'], previous_identities['source06_solvers_not_paired_benchmark'],
                     previous_identities['baseline_adapter_manifest']]:
            verify(item)
        if previous_identities['current_example']['path'] != str(OLD_CURRENT_TARGET / 'release/examples/hu_saved_profile_audit'):
            raise ValueError('unexpected current example path')
        if previous_identities['baseline_adapter_manifest']['path'] != str(BASELINE / 'r1-saved-profile-source-manifest.json'):
            raise ValueError('unexpected baseline adapter manifest path')
        if CARGO.is_symlink() or identity(CARGO) not in previous_identities['compiler_binaries']:
            raise ValueError('real pinned Cargo does not match validated compiler identity')
        current_files = archive_manifest(BASE / 'audit-source.tar.gz', CURRENT)
        if current_files != read(PRIOR / 'current-source-files.json'):
            raise ValueError('current source manifest changed')
        if archive_manifest(BASE / 'baseline-source.tar.gz') != read(PRIOR / 'baseline-source-files.json'):
            raise ValueError('baseline archive source manifest changed')
        current_boot = text('/proc/sys/kernel/random/boot_id').strip()
        if current_boot == prior['boot_id']:
            raise ValueError('this recovery requires a changed boot identity')
        state['boot_id'] = current_boot
        state['recovery']['previous_boot_id'] = prior['boot_id']
        state['recovery']['prior_build_status'] = prior['status']
        state['recovery']['prior_result_claimed_completed_stage_count'] = 14 if prior['status'] == 'completed' else 12
        state['recovery']['prior_verified_completed_stage_count'] = 12
        state['recovery']['prior_completion_marker_present'] = (PRIOR / 'complete.json').exists()
        state['recovery']['changed_boot'] = True
        state['recovery']['historical_current_example'] = previous_identities['current_example']
        state['recovery']['historical_source06_solvers'] = previous_identities['source06_solvers_not_paired_benchmark']
        if 'baseline_example' in previous_identities:
            state['recovery']['historical_baseline_example_claim'] = previous_identities['baseline_example']
        state['recovery']['validation_reused'] = 'Stages 00..05 passed against the exact source06 archive on the prior CPU; these tests are not rerun or attributed to the new CPU.'
        state['recovery']['native_cache_reused_from_prior_boot'] = False
        state['recovery']['prior_result'] = identity(PRIOR / 'result.json')
        state['recovery']['prior_current_build_complete'] = identity(PRIOR / 'current-build-complete.json')
        retained = {str(path.relative_to(PRIOR)): identity(path) for path in sorted(PRIOR.rglob('*')) if path.is_file()}
        write(OUTPUT / 'prior-evidence-files.json', retained)
        state['recovery']['prior_evidence_files'] = identity(OUTPUT / 'prior-evidence-files.json')
        state['identities'] = dict(previous_identities)
        state['identities'].pop('baseline_example', None)
        state['identities']['recovery_script'] = identity(__file__)
        immutable = [*retained.values(), *previous_identities['immutable_inputs'],
                     *previous_identities['compiler_binaries'], previous_identities['current_example'],
                     previous_identities['baseline_adapter_manifest'], identity(__file__)]
        immutable = list({item['path']: item for item in immutable}.values())
        spec = importlib.util.spec_from_file_location('r1_recovery_supervisor', CURRENT / 'tools/run_supervised.py')
        supervisor = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(supervisor)

        def stage(name, command, timeout, cwd=BASELINE, target=TARGET):
            if min(deadline - time.time(), 7200 - (time.monotonic() - started)) < timeout + 10:
                raise RuntimeError(f'insufficient unchanged deadline for {name}')
            for item in immutable:
                verify(item)
            directory = OUTPUT / name
            directory.mkdir()
            entry = {'name': name, 'argv': [str(arg) for arg in command], 'cwd': str(cwd),
                     'cargo_target_dir': str(target), 'timeout_seconds': timeout, 'status': 'running'}
            state['stages'].append(entry)
            state['status'] = 'running'
            save()
            os.environ['CARGO_TARGET_DIR'] = str(target)
            options = ['--record', str(directory / 'supervisor.json'), '--stdout', str(directory / 'stdout.log'),
                       '--stderr', str(directory / 'stderr.log'), '--cwd', str(cwd), '--timeout-seconds', str(timeout),
                       '--grace-seconds', '5', '--kill-wait-seconds', '5', '--poll-seconds', '0.25',
                       '--memory-limit-bytes', str(40 * 1024**3), '--min-free-memory-bytes', str(8 * 1024**3),
                       '--disk-reserve-bytes', str(10 * 1024**3), '--disk-path', str(BASE)]
            for item in immutable:
                options.extend(['--identity-file', item['path']])
            if min(deadline - time.time(), 7200 - (time.monotonic() - started)) < timeout + 10:
                raise RuntimeError(f'insufficient unchanged deadline after hashing for {name}')
            code = supervisor.main([*options, '--', *entry['argv']])
            record = read(directory / 'supervisor.json')
            entry.update(status=record['state'], supervisor_exit_code=code,
                         child_exit_code=record['child_exit_code'], stop_reason=record['stop_reason'],
                         cleanup_complete=record['cleanup_complete'], identity_unchanged=record['identity_unchanged'],
                         record=identity(directory / 'supervisor.json'))
            save()
            print(json.dumps(entry), flush=True)
            if (code or record['state'] != 'completed' or record['child_exit_code'] != 0
                    or record['stop_reason'] != 'completed' or record['cleanup_complete'] is not True
                    or record['identity_unchanged'] is not True):
                raise RuntimeError(f'{name} failed; retained evidence was not repaired')
            return directory

        cpu_stage = stage('00-toolchain', ['/bin/bash', '-euo', 'pipefail', '-c', TOOLCHAIN_COMMAND], 60, CURRENT, CURRENT_TARGET)
        old_cpu = cpu_identity(text(PRIOR / '00-toolchain/stdout.log'))
        new_text = text(cpu_stage / 'stdout.log')
        new_cpu = cpu_identity(new_text)
        if 'release: 1.97.0\n' not in new_text:
            raise ValueError('reboot compiler identity mismatch')
        for binary in previous_identities['compiler_binaries']:
            if binary['path'] not in new_text.splitlines():
                raise ValueError('effective reboot compiler path differs')
        state['recovery']['native_cpu_compatibility'] = {
            'status': 'changed_fresh_rebuild_required' if old_cpu != new_cpu else 'identical_fresh_rebuild',
            'previous': old_cpu, 'current': new_cpu}
        common = ['python3', TOOLS / 'apply_baseline_audit.py', '--root', BASELINE, '--reference-root', CURRENT, '--verify']
        stage('01-baseline-pre-verify', common, 120, CURRENT)
        # Fresh native build: no deps, fingerprints, build scripts or executables
        # from the previous CPU enter either new target directory.
        CURRENT_TARGET.mkdir()
        TARGET.mkdir()
        stage('06-current-release', [CARGO, 'build', '--locked', '--release', '-p', 'cli',
                                     '--bin', 'solvers', '--example', 'hu_saved_profile_audit'],
              1800, CURRENT, CURRENT_TARGET)
        state['identities']['current_example'] = identity(CURRENT_TARGET / 'release/examples/hu_saved_profile_audit')
        state['identities']['source06_solvers_not_paired_benchmark'] = identity(CURRENT_TARGET / 'release/solvers')
        immutable.extend([state['identities']['current_example'], state['identities']['source06_solvers_not_paired_benchmark']])
        if text('/proc/sys/kernel/random/boot_id').strip() != current_boot:
            raise ValueError('boot changed during current release build')
        intermediate = stage('06-current-toolchain-check', ['/bin/bash', '-euo', 'pipefail', '-c', TOOLCHAIN_COMMAND],
                             60, CURRENT, CURRENT_TARGET)
        if cpu_identity(text(intermediate / 'stdout.log')) != new_cpu:
            raise ValueError('CPU changed during current release build')
        write(OUTPUT / 'current-build-complete.json', state['identities'])
        save()
        (TARGET / 'release').mkdir()
        caches = [CURRENT_TARGET / 'release' / name for name in ('deps', 'build', '.fingerprint')]
        if any(not path.is_dir() or path.is_symlink() for path in caches):
            raise ValueError('missing/invalid freshly built dependency cache')
        stage('07-seed-baseline-cache', ['cp', '-a', '--reflink=auto', *caches, TARGET / 'release'], 300, CURRENT)
        write(OUTPUT / 'cache-seed.json', {'from_current_source_archive': state['identities']['audit-source.tar.gz'],
              'compiler_binaries': state['identities']['compiler_binaries'], 'cpu': new_cpu,
              'copied_directories': [str(path) for path in caches], 'destination': str(TARGET / 'release'),
              'copy_mode': 'reflink_auto_no_hardlinks', 'historical_cache_reused': False})
        manifest = read(previous_identities['baseline_adapter_manifest']['path'])
        for name in manifest['after']:
            if not build_input(name):
                raise ValueError('unexpected adapter build-input name')
            path = BASELINE / name
            if path.is_symlink() or not path.resolve().is_relative_to(BASELINE.resolve()):
                raise ValueError('unsafe baseline build input')
            os.utime(path, None)
        stage('12-baseline-example-release', [CARGO, 'build', '--locked', '--release', '-p', 'cli',
                                             '--example', 'hu_saved_profile_audit'], 1800)
        stage('13-baseline-final-verify', common, 120, CURRENT)
        final_cpu = stage('14-final-toolchain', ['/bin/bash', '-euo', 'pipefail', '-c', TOOLCHAIN_COMMAND], 60, CURRENT)
        if cpu_identity(text(final_cpu / 'stdout.log')) != new_cpu or text('/proc/sys/kernel/random/boot_id').strip() != current_boot:
            raise ValueError('CPU/boot changed during recovery release builds')
        if archive_manifest(BASE / 'audit-source.tar.gz', CURRENT) != current_files:
            raise ValueError('current source changed during recovery')
        for item in immutable:
            verify(item)
        state['identities']['baseline_example'] = identity(TARGET / 'release/examples/hu_saved_profile_audit')
        state['status'] = 'completed'
        state['ended_utc'] = dt.datetime.now(dt.timezone.utc).isoformat()
    except BaseException as error:
        state.update(status='failed', error=f'{type(error).__name__}: {error}',
                     ended_utc=dt.datetime.now(dt.timezone.utc).isoformat())
        raise
    finally:
        save()
        files = {str(path.relative_to(OUTPUT)): identity(path) for path in sorted(OUTPUT.rglob('*'))
                 if path.is_file() and path.name != 'retained-files.json'}
        write(OUTPUT / 'retained-files.json', files)
    write(OUTPUT / 'complete.json', {'status': 'completed', 'result': identity(OUTPUT / 'result.json'),
          'retained_files': identity(OUTPUT / 'retained-files.json'),
          'current_example': state['identities']['current_example'],
          'baseline_example': state['identities']['baseline_example'], 'changed_boot': True})
    print(json.dumps({'status': 'completed', 'evidence': str(OUTPUT)}), flush=True)


if __name__ == '__main__':
    main()
