"""Bounded two-arm scratch-array screen, using the frozen VM18 supervision kernel."""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys
import time
import traceback

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
BASE = HERE.parent / 'chance-grain/run.py'
BASE_SHA = '143a89ace93b169787b34f0c26f4683b1fde643d9fbe5553e6120cd92f8e2b79'
if hashlib.sha256(BASE.read_bytes()).hexdigest() != BASE_SHA:
    raise ValueError('Frozen VM18 runner changed')
spec = importlib.util.spec_from_file_location('sparse_rank_frozen_runner', BASE)
base = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = base
spec.loader.exec_module(base)
need, read, pin, pair, located, relative = (getattr(base, n) for n in ('need', 'read', 'pin', 'pair', 'located', 'relative'))
common, durable, SUPERVISOR, PINS = base.common, base.durable, base.SUPERVISOR, base.PINS
CASES, ARMS, WORKERS, ITERATIONS = ('narrow', 'expanded'), ('baseline', 'candidate'), (16, 32), 16
BUILD_SECONDS, WINDOW_SECONDS, MEASURE_SECONDS = 480, 1200, 360
STOP_SECONDS, HIGHCPU_SECONDS, MAX_RETAINED = 2700, 480, 240 * 1024**2
EXAMPLE, CPU_ADAPTER, CPU_ADAPTER_SHA = base.EXAMPLE, base.CPU_ADAPTER, base.CPU_ADAPTER_SHA
SOLVER, SOLVER_SHA = base.SOLVER, base.SOLVER_SHA
KERNEL = 'crates/holdem/src/kernel.rs'
KERNEL_SHA = 'd59724dd4d9a1c797bebf83422c06b75c90020796bfbf8f492417910770bb076'
CANDIDATE_SHA = '90d08f5b41a30fffffb90c8ebf76196bf84fa286b7e01aca584d538ef9851ba7'
BASE_MANIFEST = {'bytes': 75417, 'sha256': 'c376fe25a9ba2fbf2d61f5fca71bf366223fb374b7a1ef6a20cbe2b11403d04a'}
BASE_ARCHIVE = 'ad156105b5323c91c4c9e715525e9efdf75ec07acb193db3eb3106581fec466f'
REVISION = '6a5545efb0bee4a4940d260b9b97a8cf841edec1'
BASE_TEST_NAMES = base.TEST_NAMES
KERNEL_TEST_NAMES = tuple('kernel::tests::' + n for n in (
    'compact_kernels_match_old_bits_and_pairwise_on_asymmetric_overlapping_support',
    'compact_kernels_match_old_bits_and_pairwise_with_full_support',
    'compact_kernels_remove_identical_and_shared_card_only_opponents',
    'compact_and_global_kernels_agree_with_extreme_reach'))
CANDIDATE_TEST_NAMES = tuple('kernel::sparse_rank_group_tests::' + n for n in (
    'sparse_rank_groups_repeated_cards_and_asymmetric_support_match_old_bits',
    'sparse_rank_groups_zero_missing_and_tied_mass_match_old_bits',
    'sparse_rank_groups_dense_ties_fractional_and_subnormal_mass_match_old_bits',
    'sparse_rank_groups_empty_table_keeps_outputs'))


def utc(text):
    value = dt.datetime.fromisoformat(text.replace('Z', '+00:00'))
    need(value.utcoffset() == dt.timedelta(0), 'UTC timestamp required')
    return value


def bounds(launch, stop, deadline, phase, current, highcpu_arm=None, highcpu_stop=None):
    launch, stop, deadline, current = map(utc, (launch, stop, deadline, current))
    need(STOP_SECONDS - 1 < (stop - launch).total_seconds() <= STOP_SECONDS, 'original STOP must be launch +45 minutes, floored')
    need(launch <= current < deadline < stop, 'phase clock/deadline outside original lifecycle')
    if phase == 'build':
        need(deadline <= launch + dt.timedelta(seconds=WINDOW_SECONDS), 'build deadline exceeds launch +20 minutes')
    else:
        start, end = map(utc, (highcpu_arm, highcpu_stop))
        need(launch <= start <= current and HIGHCPU_SECONDS - 1 < (end - start).total_seconds() <= HIGHCPU_SECONDS,
             '32-vCPU STOP must be phase arm +8 minutes, floored')
        need(240 < (deadline - current).total_seconds() <= MEASURE_SECONDS, 'measurement requires >240..360 seconds')
        need(deadline <= end - dt.timedelta(seconds=20), '32-vCPU stop margin missing')
        need(end <= stop, '32-vCPU stop extends original lifecycle')
    return deadline


def schedule():
    rows = []
    for workers in (1, 32):
        for arm in ARMS:
            rows.append(dict(name=f'smoke-narrow-w{workers}-{arm}', kind='smoke', case='narrow', arm=arm,
                             depth=2, workers=workers, iterations=2, round=None, warmup=False))
    for case in CASES:
        rows.append(dict(name=f'canonical-{case}', kind='canonical', case=case, arm='baseline', depth=2,
                         workers=1, iterations=ITERATIONS, round=None, warmup=False))
    for case in CASES:
        for r in range(4):
            for workers in (WORKERS if r % 2 == 0 else WORKERS[::-1]):
                for arm in (ARMS if r % 2 == 0 else ARMS[::-1]):
                    rows.append(dict(name=f'{case}-r{r}-w{workers}-{arm}', kind='matrix', case=case, arm=arm,
                                     depth=2, workers=workers, iterations=ITERATIONS, round=r, warmup=r == 0))
    return rows


def solve_seconds(row):
    return 60 if row['kind'] == 'canonical' else 20


def controls():
    return [Path(__file__), HERE / 'analyze.py', HERE / 'protocol.jp.md', HERE / 'prepare.py', HERE / 'provenance.json',
            HERE / 'candidate.patch', HERE / 'kernel.rs', HERE / 'tests.rs.in', BASE, HERE.parent / 'chance-grain/analyze.py',
            base.COMMON, common.SHARED, base.DURABLE, SUPERVISOR, CPU_ADAPTER,
            HERE.parent / 'chance-grain/adapter/prepare.py', HERE.parent / 'chance-grain/adapter/provenance.json']


def source_bindings(originals, sources, adapter):
    need(originals[SOLVER]['sha256'] == SOLVER_SHA and originals[KERNEL]['sha256'] == KERNEL_SHA
         and adapter['sha256'] == CPU_ADAPTER_SHA, 'frozen source/adapter differs')
    example = f'crates/holdem/examples/{EXAMPLE}.rs'
    need(example not in originals and set(sources) == set(ARMS), 'source arms differ')
    need(sources['baseline'] == originals | {example: adapter}, 'baseline source differs')
    candidate = pin(HERE / 'kernel.rs')
    need(candidate['sha256'] == CANDIDATE_SHA and sources['candidate'] == sources['baseline'] | {KERNEL: candidate},
         'candidate source differs outside selected kernel')
    need(all(n in originals for n in ('Cargo.toml', 'Cargo.lock', '.cargo/config.toml')), 'workspace metadata missing')


def archive_source(source, files, destination):
    # Tar members in path-component order avoid parent/file order ambiguity.
    return base_archive_source(source, dict(sorted(files.items(), key=lambda item: PurePosixPath(item[0]).parts)), destination)


def validate_package(manifest, installed, overlay, applied):
    mp, op = ROOT / 'manifest.json', ROOT / 'overlay-manifest.json'
    need(manifest['schema'] == 'r1-cpu-profile-package/v1' and pin(mp) == BASE_MANIFEST
         and manifest['source_revision'] == REVISION and installed['archive_sha256'] == BASE_ARCHIVE,
         'old frozen package identity differs')
    need(overlay['schema'] == 'r1-sparse-rank-groups-overlay/v1' and overlay['base_manifest'] == pin(mp)
         and overlay['source_revision'] == manifest['source_revision'], 'overlay base binding differs')
    need(installed['manifest'] == pin(mp) and installed['destination'] == str(ROOT)
         and installed['source_revision'] == manifest['source_revision']
         and installed['source_files'] == len(manifest['source_pins']) and installed['builds_or_solves_started'] == 0,
         'old installation differs')
    need(len(installed['archive_sha256']) == 64 and all(c in '0123456789abcdef' for c in installed['archive_sha256']), 'old archive identity missing')
    need(applied['schema'] == 'r1-sparse-rank-groups-overlay-installation/v1' and applied['manifest'] == pin(op)
         and applied['base_manifest'] == pin(mp) and applied['destination'] == str(ROOT)
         and applied['source_revision'] == manifest['source_revision'] and applied['builds_or_solves_started'] == 0,
         'overlay installation differs')
    need(not set(manifest['files']) & set(overlay['files']), 'overlay overwrites old package member')
    for name, value in (manifest['files'] | overlay['files']).items():
        p = ROOT / relative(name)
        need(p.is_file() and not p.is_symlink() and p.resolve().is_relative_to(ROOT) and pin(p) == value, 'installed file pin differs')
    need({n.removeprefix('source/'): v for n, v in manifest['files'].items() if n.startswith('source/')} == manifest['source_pins'], 'pristine source pins differ')
    for p in controls():
        need((manifest['files'] | overlay['files']).get(p.relative_to(ROOT).as_posix()) == pin(p), 'control not package-pinned')


def prepare(args):
    machine = base.host('build')
    created = base.now()
    end = bounds(args.launch_attempted_at, args.stop_deadline_utc, args.deadline_utc, 'build', created)
    out, workspace, source = args.out.resolve(), args.workspace.resolve(), args.source.resolve(strict=True)
    paths = [out, workspace, source]
    need(len(set(paths)) == 3 and all(not a.is_relative_to(b) for a in paths for b in paths if a != b), 'owned paths overlap')
    need(source == (ROOT / 'source').resolve(strict=True), 'pristine packaged source required')
    need(not out.exists() and not workspace.exists(), 'fresh output/workspace required')
    mp, ip, op, oi = (ROOT / n for n in ('manifest.json', 'installation.json', 'overlay-manifest.json', 'overlay-installation.json'))
    manifest, installed, overlay, applied = map(read, (mp, ip, op, oi))
    validate_package(manifest, installed, overlay, applied)
    need(base.inventory(source) == manifest['source_pins'], 'pristine source inventory differs')
    tools = {n: located(getattr(args, n).resolve(strict=True)) for n in ('cargo', 'rustc')}
    tools['python'] = located(Path(sys.executable).resolve(strict=True))
    out.mkdir(parents=True)
    workspace.mkdir(parents=True)
    for p in (out / 'inputs', out / 'canonical', workspace / 'canonical'):
        p.mkdir()
    sources = {}
    example = f'crates/holdem/examples/{EXAMPLE}.rs'
    for arm in ARMS:
        destination = workspace / ('source-' + arm)
        destination.mkdir()
        for name in manifest['source_pins']:
            target = destination / relative(name)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / name, target)
        (destination / example).parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(CPU_ADAPTER, destination / example)
        sources[arm] = destination
    command = [tools['python']['path'], '-B', str(HERE / 'prepare.py'), '--apply-to', str(sources['candidate']), '--receipt', str(out / 'application.json')]
    applied_result = subprocess.run(command, capture_output=True, timeout=10)
    for stream in ('stdout', 'stderr'):
        (out / ('application.' + stream + '.log')).write_bytes(getattr(applied_result, stream))
    need(applied_result.returncode == 0, 'candidate application failed')
    files = {arm: base.inventory(path) for arm, path in sources.items()}
    source_bindings(manifest['source_pins'], files, pin(CPU_ADAPTER))
    captured = {}
    for p in [*controls(), mp, ip, op, oi]:
        value = pin(p)
        retained = out / 'inputs' / value['sha256']
        if not retained.exists():
            shutil.copyfile(p, retained)
        captured[str(p)] = {**value, 'retained': retained.relative_to(out).as_posix()}
    records = {}
    for arm, path in sources.items():
        archive = out / ('source-' + arm + '.tar.gz')
        archive_source(path, files[arm], archive)
        records[arm] = dict(path=str(path), files=files[arm], archive=located(archive))
    plan = dict(schema='r1.sparse-rank-groups/v1', output=str(out), workspace=str(workspace), created_at=created,
                deadline_utc=args.deadline_utc, deadline_monotonic=time.monotonic() + end.timestamp() - time.time(),
                launch_attempted_at=args.launch_attempted_at, stop_deadline_utc=args.stop_deadline_utc,
                host=machine, phase='build', plan_file='plan.json', sources=records, controls=captured, tools=tools,
                environment=base.environment(tools), limits=base.phase_limits('build'), schedule=schedule(),
                application=located(out / 'application.json'),
                package=dict(manifest=pin(mp), archive_sha256=installed['archive_sha256'], overlay=pin(op), overlay_installation=pin(oi)))
    durable.sync_files([located(p) for p in out.rglob('*') if p.is_file()])
    durable.atomic_json(out / 'plan.json', plan, once=True)
    base.live(plan)
    print(json.dumps(dict(status='prepared', solves=38, build_stages=5)), flush=True)


def build_rows():
    rows = [dict(name='toolchain', kind='toolchain')]
    for arm in ARMS:
        rows += [dict(name='build-' + arm, kind='build', arm=arm), dict(name='tests-' + arm, kind='tests', arm=arm)]
    return rows


def test_command(plan, arm):
    return [plan['tools']['cargo']['path'], 'test', '--locked', '--offline', '--release', '-j2', '--target-dir',
            str(PurePosixPath(plan['workspace']) / (arm + '-target')), '-p', 'engine', '-p', 'holdem', '-p', 'cfr-ref',
            '--tests', '--', '--test-threads=1']


def validate_tests(stdout, arm):
    need(arm in ARMS, 'unknown test arm')
    for name in BASE_TEST_NAMES + KERNEL_TEST_NAMES + (CANDIDATE_TEST_NAMES if arm == 'candidate' else ()):
        need(stdout.splitlines().count('test ' + name + ' ... ok') == 1, 'required regression missing: ' + name)


def stage(out, plan, row, receipt, seconds, cwd, binaries=()):
    # The inherited solve is unchanged except its per-row finite supervisor cap.
    if row['kind'] in ('smoke', 'canonical', 'matrix'):
        seconds = solve_seconds(row)
    return base_stage(out, plan, row, receipt, seconds, cwd, binaries)


def measure_prepare(args):
    out = args.out.resolve(strict=True)
    original, built, execution = (read(out / n) for n in ('plan.json', 'build.json', 'build-execution.json'))
    need(built['status'] == execution['status'] == 'completed' and built['plan'] == pin(out / 'plan.json')
         and built['execution'] == pin(out / 'build-execution.json') and built['stages'] == execution['stages']
         and built['binaries'] == execution['binaries'], 'immutable successful build required')
    need(base.inventory_subset(out, built['files']) == built['files'], 'build evidence changed')
    machine = base.host('measure')
    need(machine['boot_id'] != original['host']['boot_id'] and machine['instance_id'] == original['host']['instance_id'], 'same-instance resized boot required')
    created = base.now()
    end = bounds(original['launch_attempted_at'], original['stop_deadline_utc'], args.deadline_utc, 'measure', created,
                 args.highcpu_armed_at, args.highcpu_stop_deadline_utc)
    need(utc(execution['ended_at']) <= utc(args.highcpu_armed_at), 'high-CPU phase predates successful build')
    plan = {**original, 'phase': 'measure', 'plan_file': 'measurement.json', 'host': machine, 'limits': base.phase_limits('measure'),
            'created_at': created, 'deadline_utc': args.deadline_utc, 'deadline_monotonic': time.monotonic() + end.timestamp() - time.time(),
            'build_plan': pin(out / 'plan.json'), 'build_receipt': pin(out / 'build.json'), 'build_execution': pin(out / 'build-execution.json'),
            'highcpu_armed_at': args.highcpu_armed_at, 'highcpu_stop_deadline_utc': args.highcpu_stop_deadline_utc}
    for binary in built['binaries'].values():
        need(pin(binary['path']) == pair(binary), 'portable binary differs')
    need(not (out / 'measurement.json').exists() and not (out / 'execution.json').exists(), 'no measurement retry/resume')
    durable.atomic_json(out / 'measurement.json', plan, once=True)
    base.live(plan)
    print(json.dumps(dict(status='measurement_prepared', deadline=plan['deadline_utc'])), flush=True)


def finish_manifest(out):
    files = {n: p for n, p in base.inventory(out).items() if n != 'retained.json'}
    need(sum(v['bytes'] for v in files.values()) <= MAX_RETAINED, 'retained bound exceeded')
    durable.sync_files([dict(path=str(out / n), **p) for n, p in files.items()])
    durable.atomic_json(out / 'retained.json', dict(schema='r1.sparse-rank-groups-retained/v1', files=files), once=True)


def execute(args):
    out = args.out.resolve(strict=True)
    building = args.phase == 'build'
    plan = read(out / ('plan.json' if building else 'measurement.json'))
    receipt_file = 'build-execution.json' if building else 'execution.json'
    need(plan['phase'] == ('build' if building else 'measure') and str(out) == plan['output'] and not (out / receipt_file).exists(), 'no retry/resume')
    rows = [{**r, 'status': 'pending'} for r in (build_rows() if building else schedule())]
    built = None if building else read(out / 'build.json')
    if built:
        need(pin(out / 'build.json') == plan['build_receipt'] and pin(out / 'build-execution.json') == plan['build_execution']
             and base.inventory_subset(out, built['files']) == built['files'], 'immutable build changed')
    receipt = dict(schema='r1.sparse-rank-groups-execution/v1', phase=plan['phase'], receipt_file=receipt_file,
                   status='running', plan=pin(out / plan['plan_file']), started_at=base.now(), stages=rows,
                   binaries={} if building else built['binaries'], canonical={}, prepared_plan=plan)
    current = None
    try:
        base.live(plan, BUILD_SECONDS + 10 if building else 30)
        base.clean_environment(plan)
        for current in rows:
            kind = current['kind']
            if kind == 'toolchain':
                current['command'] = [plan['tools']['rustc']['path'], '-Vv']
                directory = stage(out, plan, current, receipt, 10, out)
                record = read(directory / 'supervisor.json')
                version = Path(record['outputs']['stdout']['path']).read_text()
                need('release: 1.97.0' in version and 'host: x86_64-unknown-linux-gnu' in version, 'compiler differs')
                receipt['rustc_version'] = version
                base.complete(out, current, receipt)
            elif kind in ('build', 'tests'):
                arm = current['arm']
                target = Path(plan['workspace']) / (arm + '-target')
                if kind == 'build':
                    need(not target.exists(), 'fresh arm target required')
                    current['command'] = [plan['tools']['cargo']['path'], 'build', '--locked', '--offline', '--release', '-j2',
                                          '--target-dir', str(target), '-p', 'holdem', '--example', EXAMPLE, '--message-format=json']
                else:
                    current['command'] = test_command(plan, arm)
                directory = stage(out, plan, current, receipt, BUILD_SECONDS, Path(plan['sources'][arm]['path']))
                if kind == 'build':
                    binary = located(target / 'release/examples' / EXAMPLE)
                    receipt['binaries'][arm] = binary
                    current['retained_binary'] = durable.gzip_verified(binary['path'], out / ('binary-' + arm + '.gz'), binary)
                else:
                    record = read(directory / 'supervisor.json')
                    validate_tests(Path(record['outputs']['stdout']['path']).read_text(), arm)
                base.complete(out, current, receipt)
            else:
                key = base.canonical_key(current)
                receipt['canonical'][key] = base.solve(out, plan, current, receipt, receipt['canonical'].get(key))
        base.live(plan)
        need(all(r['status'] == 'completed' for r in rows), 'incomplete fixed schedule')
        receipt['status'] = 'completed'
    except BaseException as error:
        receipt.update(status='failed', error=repr(error), traceback=traceback.format_exc())
        if current is not None and current['status'] != 'completed':
            current.update(status='failed', error=repr(error))
        for row in rows:
            if row['status'] == 'pending':
                row.update(status='skipped', reason='stopped on first failure')
        base.preserve_failed_state(out, plan, current, receipt)
        raise
    finally:
        receipt.pop('prepared_plan', None)
        receipt['ended_at'] = base.now()
        receipt['counts'] = {s: sum(r['status'] == s for r in rows) for s in ('completed', 'failed', 'skipped')}
        base.persist(out, receipt)
        if building and receipt['status'] == 'completed':
            files = base.inventory(out)
            durable.sync_files([dict(path=str(out / n), **p) for n, p in files.items()])
            durable.atomic_json(out / 'build.json', dict(schema='r1.sparse-rank-groups-build/v1', status='completed', plan=receipt['plan'],
                execution=pin(out / receipt_file), stages=rows, binaries=receipt['binaries'], files=files), once=True)
        else:
            try:
                finish_manifest(out)
            except BaseException as error:
                receipt.update(status='failed', retention_manifest_error=repr(error))
                base.persist(out, receipt)
                if 'error' not in receipt:
                    raise
        print(json.dumps(dict(status=receipt['status'], phase=plan['phase'], counts=receipt['counts'])), flush=True)


# Only these campaign hooks differ. Full byte state/quality comparison, durable
# gzip, host/feature/environment checks and terminal supervisor checks stay frozen.
base_stage, base_archive_source = base.stage, base.archive_source
for name in ('controls', 'schedule', 'source_bindings', 'MAX_RETAINED', 'stage'):
    setattr(base, name, globals()[name])
base.__file__ = __file__
for name in ('phase_limits', 'host', 'validate_features', 'one_per_core', 'terminal', 'raw_outputs', 'environment',
             'canonical_key', 'validate_cpu', 'validate_values', 'validate_grain', 'loads', 'inventory', 'forbidden_environment'):
    globals()[name] = getattr(base, name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('phase', choices=('prepare', 'build', 'measure-prepare', 'measure'))
    parser.add_argument('--out', type=Path, required=True)
    for name in ('source', 'workspace', 'cargo', 'rustc'):
        parser.add_argument('--' + name, type=Path)
    parser.add_argument('--deadline-utc', '--build-deadline-utc', '--measurement-deadline-utc', dest='deadline_utc')
    for name in ('launch-attempted-at', 'stop-deadline-utc', 'highcpu-armed-at', 'highcpu-stop-deadline-utc'):
        parser.add_argument('--' + name)
    args = parser.parse_args()
    if args.phase == 'prepare':
        need(all(getattr(args, n) for n in ('source', 'workspace', 'cargo', 'rustc', 'deadline_utc', 'launch_attempted_at', 'stop_deadline_utc')), 'prepare arguments missing')
        prepare(args)
    elif args.phase == 'measure-prepare':
        need(args.deadline_utc and args.highcpu_armed_at and args.highcpu_stop_deadline_utc, 'measurement bounds missing')
        measure_prepare(args)
    else:
        execute(args)


if __name__ == '__main__':
    main()
