"""Two-arm full-proof reader. Run stream checks on the bounded cloud recovery host."""
from __future__ import annotations

import argparse
import gzip
import importlib.util
import json
from pathlib import Path, PurePosixPath
import sys
import tarfile

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('sparse_rank_trusted_runner', HERE / 'run.py')
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)
need, read, pin, pair, relative = run.need, run.read, run.pin, run.pair, run.relative
HELPER = HERE.parent / 'chance-grain/analyze.py'
need(pin(HELPER)['sha256'] == '866dfdc35ff9e2e5f6f0ed2b681e657dd7aa80b29e12da43731801c8bc0b1fcf',
     'Trusted VM18 reader differs')
old = run.base.load('sparse_rank_common_reader', HELPER)
old.run = run
utc, digest, verify_host = old.utc, old.digest, old.verify_host


class Evidence(old.Evidence):
    """VM18 exact membership/path/state verifier with the new schema only."""
    def __init__(self, root):
        self.root = Path(root).resolve(strict=True)
        manifest = read(self.root / 'retained.json')
        need(manifest['schema'] == 'r1.sparse-rank-groups-retained/v1', 'Retained schema differs')
        self.files = manifest['files']
        actual, total = {}, 0
        for path in sorted(self.root.rglob('*'), key=lambda p: p.relative_to(self.root).parts):
            need(not path.is_symlink(), 'Evidence symlink')
            if not path.is_file():
                need(path.is_dir(), 'Nonregular evidence member')
                continue
            name = relative(path.relative_to(self.root).as_posix()).as_posix()
            total += path.stat().st_size
            need(total <= run.MAX_RETAINED, 'Retained evidence size exceeded')
            if name != 'retained.json':
                actual[name] = pin(path)
        need(actual == self.files, 'Retained membership or exact bytes differ')
        self.plan = read(self.root / 'plan.json')
        self.origin = PurePosixPath(self.plan['output'])
        need(self.origin.is_absolute(), 'Original Linux output root required')
        self.states = set()


def verify_plan(e):
    p = e.plan
    need(p['schema'] == 'r1.sparse-rank-groups/v1' and p['schedule'] == run.schedule()
         and p['phase'] == 'build' and p['plan_file'] == 'plan.json'
         and p['limits'] == run.phase_limits('build') and p['environment'] == run.environment(p['tools']),
         'Prepared protocol differs')
    run.bounds(p['launch_attempted_at'], p['stop_deadline_utc'], p['deadline_utc'], 'build', p['created_at'])
    verify_host(p['host'], 'build')
    need(set(p['tools']) == {'cargo', 'rustc', 'python'}, 'Build tools differ')
    manifest_name, _, manifest_path = e.control('manifest.json')
    _, _, installation_path = e.control('installation.json')
    _, _, overlay_path = e.control('overlay-manifest.json')
    _, _, overlay_installation_path = e.control('overlay-installation.json')
    manifest, installation = read(manifest_path), read(installation_path)
    overlay, applied = read(overlay_path), read(overlay_installation_path)
    need(manifest['schema'] == 'r1-cpu-profile-package/v1'
         and pin(manifest_path) == {'bytes': 75417, 'sha256': 'c376fe25a9ba2fbf2d61f5fca71bf366223fb374b7a1ef6a20cbe2b11403d04a'}
         and pin(manifest_path) == p['package']['manifest']
         and installation['manifest'] == p['package']['manifest']
         and installation['archive_sha256'] == p['package']['archive_sha256'] == run.BASE_ARCHIVE
         and manifest['source_revision'] == run.REVISION, 'Package binding differs')
    package_root = str(PurePosixPath(manifest_name).parent)
    need(installation['destination'] == package_root and installation['source_revision'] == manifest['source_revision']
         and installation['source_files'] == len(manifest['source_pins'])
         and installation['builds_or_solves_started'] == 0, 'Installation receipt differs')
    need(overlay['schema'] == 'r1-sparse-rank-groups-overlay/v1'
         and overlay['base_manifest'] == pin(manifest_path) and overlay['source_revision'] == manifest['source_revision']
         and pin(overlay_path) == p['package']['overlay'], 'Overlay base binding differs')
    need(applied['schema'] == 'r1-sparse-rank-groups-overlay-installation/v1'
         and applied['manifest'] == pin(overlay_path) and applied['base_manifest'] == pin(manifest_path)
         and applied['destination'] == package_root and applied['source_revision'] == manifest['source_revision']
         and applied['builds_or_solves_started'] == 0
         and pin(overlay_installation_path) == p['package']['overlay_installation'], 'Overlay installation differs')
    need(not set(manifest['files']) & set(overlay['files']), 'Overlay overwrites original member')
    for path in run.controls():
        suffix = path.relative_to(run.ROOT).as_posix()
        original, value, retained = e.control(suffix)
        need(original == package_root + '/' + suffix and pair(value) == pin(path) == pin(retained)
             and (manifest['files'] | overlay['files']).get(suffix) == pair(value), 'Trusted control pin differs')
    need({n.removeprefix('source/'): v for n, v in manifest['files'].items() if n.startswith('source/')}
         == manifest['source_pins'], 'Pristine source package pins differ')
    sources = p['sources']
    need(set(sources) == {'baseline', 'candidate'}, 'Two source arms required')
    for arm, source in sources.items():
        need(source['path'] == str(PurePosixPath(p['workspace']) / ('source-' + arm)), 'Source copy path differs')
    run.source_bindings(manifest['source_pins'], {a: s['files'] for a, s in sources.items()}, pin(run.CPU_ADAPTER))
    application = read(e.require(p['application']))
    need(e.name(p['application']['path']) == 'application.json'
         and application == {'schema': 'r1.sparse-rank-groups-application/v1',
                             'source_copy': sources['candidate']['path'], 'file': run.KERNEL,
                             'before': manifest['source_pins'][run.KERNEL], 'after': pin(HERE / 'kernel.rs'),
                             'compiled': False}, 'Candidate application differs')
    for name in ('application.stdout.log', 'application.stderr.log'):
        need(name in e.files and (e.root / name).read_bytes() == b'', 'Candidate application diagnostic differs')
    for source in sources.values():
        observed, total = {}, 0
        with tarfile.open(e.require(source['archive']), 'r|gz') as archive:
            for member in archive:
                name = relative(member.name).as_posix()
                need(member.isfile() and name not in observed and member.size <= 2 * 1024**2,
                     'Source archive member differs')
                total += member.size
                need(total <= 16 * 1024**2 and len(observed) < 1024, 'Source archive bound exceeded')
                with archive.extractfile(member) as stream:
                    observed[name] = digest(stream, member.size)
        need(observed == source['files']
             and list(observed) == sorted(source['files'], key=lambda n: PurePosixPath(n).parts),
             'Retained source archive bytes/order differ')


def verify_stage(e, row, binaries):
    need(row['status'] == 'completed' and row['supervisor_exit'] == 0
         and row['host_before'] == row['host_after'] == e.plan['host'], 'Stage completion/host differs')
    need(row['environment'] == e.plan['environment'] and row['forbidden_environment'] == [], 'Stage environment differs')
    need(utc(e.plan['created_at']) <= utc(row['started_at']) <= utc(row['ended_at']) <= utc(row['verified_at'])
         <= utc(e.plan['deadline_utc']), 'Stage outside deadline')
    need(read(e.require(row['completion'])) == {k: v for k, v in row.items() if k != 'completion'},
         'Immutable stage receipt differs')
    record = read(e.require(row['record']))
    run.terminal(record)
    need(e.name(row['record']['path']) == row['name'] + '/supervisor.json'
         and e.name(row['completion']['path']) == row['name'] + '/completed.json', 'Stage receipt path differs')
    need(record['argv'] == record['resolved_argv'] == row['command'] and record['shell'] is False, 'Workload command differs')
    building = row['kind'] in {'build', 'tests'}
    need(record['cwd'] == (e.plan['sources'][row['arm']]['path'] if building else str(e.origin)), 'Stage cwd differs')
    seconds = run.BUILD_SECONDS if building else 10 if row['kind'] == 'toolchain' else run.solve_seconds(row)
    need(record['limits'] == {**e.plan['limits'], 'timeout_seconds': seconds}, 'Stage limits differ')
    need(record['runtime']['logical_cpus'] == e.plan['host']['logical_cpus']
         and record['runtime']['machine'] == 'x86_64', 'Runtime CPU differs')
    identities = {x['path']: pair(x) for x in record['identity_before']}
    need(identities.get(str(e.origin / e.plan['plan_file'])) == e.files[e.plan['plan_file']], 'Plan identity differs')
    for suffix in ('experiments/hu-postflop-r1/flop-scaling/sparse-rank-groups/run.py', 'tools/run_supervised.py'):
        name, value, _ = e.control(suffix)
        need(identities.get(name) == pair(value), 'Supervisor control identity differs')
    tool = e.plan['tools']['cargo' if building else 'rustc'] if building or row['kind'] == 'toolchain' else binaries[row['arm']]
    need(identities.get(tool['path']) == pair(tool), 'Build tool or solver binary identity differs')
    for value in run.raw_outputs(record, e.origin / row['name']):
        e.require(value)
    need(row['process_seconds'] == record['elapsed_seconds'] and 0 < row['process_seconds'] <= seconds + 1,
         'Whole-process timer differs')
    need(row['root_os_peak_resident_bytes'] == record['last_sample']['root_os_peak_resident_bytes'] > 0
         and row['root_os_peak_source'] == record['last_sample']['root_os_peak_source'] == 'wait4.ru_maxrss_linux_kib',
         'Root RSS counter differs')
    return record


def verify_build(e):
    built, execution = read(e.root / 'build.json'), read(e.root / 'build-execution.json')
    need(built['schema'] == 'r1.sparse-rank-groups-build/v1'
         and built['status'] == execution['status'] == 'completed'
         and built['plan'] == e.files['plan.json'] and built['execution'] == e.files['build-execution.json']
         and built['stages'] == execution['stages'] and built['binaries'] == execution['binaries'], 'Build receipt differs')
    need(execution['schema'] == 'r1.sparse-rank-groups-execution/v1' and execution['phase'] == 'build'
         and execution['receipt_file'] == 'build-execution.json' and execution['plan'] == built['plan']
         and execution['counts'] == {'completed': 5, 'failed': 0, 'skipped': 0}, 'Build execution differs')
    need(utc(e.plan['created_at']) <= utc(execution['started_at']) <= utc(execution['ended_at'])
         <= utc(e.plan['deadline_utc']), 'Build deadline differs')
    expected_files = {n: v for n, v in e.files.items() if n in {'plan.json', 'build-execution.json',
                       'source-baseline.tar.gz', 'source-candidate.tar.gz', 'binary-baseline.gz', 'binary-candidate.gz',
                       'application.json', 'application.stdout.log', 'application.stderr.log'}
                      or n.startswith(('inputs/', 'toolchain/', 'build-baseline/', 'tests-baseline/',
                                       'build-candidate/', 'tests-candidate/'))}
    need(built['files'] == expected_files, 'Immutable complete build inventory differs')
    need(len(built['stages']) == 5 and all(all(row[k] == v for k, v in wanted.items())
         for row, wanted in zip(built['stages'], run.build_rows())), 'Build schedule differs')
    first = built['stages'][0]
    tools = e.plan['tools']
    need(first['command'] == [tools['rustc']['path'], '-Vv'], 'Toolchain command differs')
    record = verify_stage(e, first, {})
    version = e.require(record['outputs']['stdout']).read_text()
    need(version == execution['rustc_version'] and 'release: 1.97.0' in version
         and 'host: x86_64-unknown-linux-gnu' in version, 'Compiler version differs')
    need(set(built['binaries']) == {'baseline', 'candidate'}, 'Two binary arms required')
    previous = utc(first['verified_at'])
    for arm, build, tests in zip(run.ARMS, built['stages'][1::2], built['stages'][2::2]):
        need(previous <= utc(build['started_at']), 'Build ordering differs')
        target = str(PurePosixPath(e.plan['workspace']) / (arm + '-target'))
        expected = [tools['cargo']['path'], 'build', '--locked', '--offline', '--release', '-j2', '--target-dir', target,
                    '-p', 'holdem', '--example', run.EXAMPLE, '--message-format=json']
        need(build['command'] == expected, 'Portable build command differs')
        record = verify_stage(e, build, {})
        artifacts = []
        for line in e.require(record['outputs']['stdout']).read_text().splitlines():
            message = run.loads(line)
            if message.get('reason') == 'compiler-artifact' and message['target']['name'] == run.EXAMPLE:
                need(message['target']['kind'] == ['example'] and message['profile']['opt_level'] == '3'
                     and not message['profile']['test'] and not message['fresh'], 'Fresh optimized example differs')
                artifacts.append(message['executable'])
        binary = built['binaries'][arm]
        need(artifacts == [binary['path']] and binary['path'] == target + '/release/examples/' + run.EXAMPLE,
             'Binary path/artifact count differs')
        with gzip.open(e.require(build['retained_binary']), 'rb') as stream:
            need(digest(stream, 128 * 1024**2) == pair(binary), 'Retained binary bytes differ')
        need(utc(build['verified_at']) <= utc(tests['started_at'])
             and tests['command'] == run.test_command(e.plan, arm), 'Core test command/order differs')
        record = verify_stage(e, tests, {})
        run.validate_tests(e.require(record['outputs']['stdout']).read_text(), arm)
        previous = utc(tests['verified_at'])
    need(previous <= utc(execution['ended_at']), 'Build terminal ordering differs')
    return built['binaries'], execution


def verify_measurement(e, execution):
    original, p = e.plan, read(e.root / 'measurement.json')
    changing = {'phase', 'plan_file', 'host', 'limits', 'created_at', 'deadline_utc', 'deadline_monotonic'}
    extra = {'build_plan', 'build_receipt', 'build_execution', 'highcpu_armed_at', 'highcpu_stop_deadline_utc'}
    need(set(p) == set(original) | extra and {k: v for k, v in p.items() if k not in changing | extra}
         == {k: v for k, v in original.items() if k not in changing}, 'Measurement changed source/protocol')
    need(p['phase'] == 'measure' and p['plan_file'] == 'measurement.json' and p['limits'] == run.phase_limits('measure')
         and p['build_plan'] == e.files['plan.json'] and p['build_receipt'] == e.files['build.json']
         and p['build_execution'] == e.files['build-execution.json'], 'Build-to-measurement binding differs')
    run.bounds(p['launch_attempted_at'], p['stop_deadline_utc'], p['deadline_utc'], 'measure', p['created_at'],
               p['highcpu_armed_at'], p['highcpu_stop_deadline_utc'])
    need(utc(execution['ended_at']) <= utc(p['highcpu_armed_at']) <= utc(p['created_at']), 'Measurement precedes build')
    verify_host(p['host'], 'measure')
    need(p['host']['boot_id'] != original['host']['boot_id'] and p['host']['instance_id'] == original['host']['instance_id'],
         'Build/measurement resize boundary differs')
    e.plan = p


def summarize(rows):
    expected = run.schedule()
    need(len(rows) == len(expected) == 38 and all(all(row[k] == v for k, v in wanted.items())
         for row, wanted in zip(rows, expected)) and all(row['status'] == 'completed' for row in rows),
         'Incomplete or reordered fixed schedule')
    groups = []
    for case in run.CASES:
        for arm in run.ARMS:
            for workers in run.WORKERS:
                selected = [r for r in rows if r['kind'] == 'matrix' and r['case'] == case
                            and r['arm'] == arm and r['workers'] == workers and not r['warmup']]
                need([r['round'] for r in selected] == [1, 2, 3], 'Measured rounds differ')
                samples = [old.observations(r) for r in selected]
                groups.append({'case': case, 'arm': arm, 'workers': workers, 'iterations': 16,
                               'stages': [r['name'] for r in selected],
                               'metrics': {k: old.stats([s[k] for s in samples]) for k in samples[0]}})
    lookup = {(g['case'], g['arm'], g['workers']): g for g in groups}
    ratios = []
    for case in run.CASES:
        for workers in run.WORKERS:
            baseline = lookup[(case, 'baseline', workers)]['metrics']
            candidate = lookup[(case, 'candidate', workers)]['metrics']
            ratios.append({'case': case, 'workers': workers,
                           'candidate_over_baseline_cfr_median': candidate['cfr_wall_seconds']['median'] / baseline['cfr_wall_seconds']['median'],
                           'candidate_over_baseline_quality_median': candidate['quality_7_walks_wall_seconds']['median'] / baseline['quality_7_walks_wall_seconds']['median'],
                           'candidate_over_baseline_rss_maximum': candidate['root_os_peak_resident_bytes']['maximum'] / baseline['root_os_peak_resident_bytes']['maximum']})
    for group in groups:
        base16 = lookup[(group['case'], group['arm'], 16)]['metrics']
        group['same_arm_16worker_relative'] = {}
        for metric in ('cfr_wall_seconds', 'quality_7_walks_wall_seconds', 'cfr_plus_quality_wall_seconds'):
            speedup = base16[metric]['median'] / group['metrics'][metric]['median']
            group['same_arm_16worker_relative'][metric] = {'speedup': speedup,
                                                         'relative_efficiency': speedup / (group['workers'] / 16)}
    guards = {
        'both_inputs_32worker_cfr_ratio_at_most_0_95': all(r['candidate_over_baseline_cfr_median'] <= .95 for r in ratios if r['workers'] == 32),
        'both_inputs_16worker_cfr_ratio_at_most_1_03': all(r['candidate_over_baseline_cfr_median'] <= 1.03 for r in ratios if r['workers'] == 16),
        'all_quality_median_ratios_at_most_1_05': all(r['candidate_over_baseline_quality_median'] <= 1.05 for r in ratios),
        'all_root_rss_maximum_ratios_at_most_1_10': all(r['candidate_over_baseline_rss_maximum'] <= 1.10 for r in ratios),
        'all_cfr_quality_three_sample_max_over_min_at_most_1_15': all(
            g['metrics'][m]['max_over_min'] is not None and g['metrics'][m]['max_over_min'] <= 1.15
            for g in groups for m in ('cfr_wall_seconds', 'quality_7_walks_wall_seconds'))}
    return {'groups': groups, 'comparisons': ratios, 'predeclared_guards': guards,
            'performance_screen': 'passed' if all(guards.values()) else 'rejected', 'production_adoption': False}


def check(root):
    e = Evidence(root)
    verify_plan(e)
    build_host = e.plan['host']
    binaries, build_execution = verify_build(e)
    verify_measurement(e, build_execution)
    execution = read(e.root / 'execution.json')
    need(execution['schema'] == 'r1.sparse-rank-groups-execution/v1' and execution['phase'] == 'measure'
         and execution['receipt_file'] == 'execution.json' and execution['plan'] == e.files['measurement.json']
         and execution['binaries'] == binaries, 'Measurement execution binding differs')
    need(execution['status'] == 'completed' and len(execution['stages']) == 38
         and execution['counts'] == {'completed': 38, 'failed': 0, 'skipped': 0}, 'All 38 solves required')
    need(utc(e.plan['created_at']) <= utc(execution['started_at']) <= utc(execution['ended_at'])
         <= utc(e.plan['deadline_utc']), 'Measurement deadline differs')
    canonical, frontiers = {}, {}
    previous = utc(e.plan['created_at'])
    for row, expected in zip(execution['stages'], run.schedule()):
        need(all(row[k] == value for k, value in expected.items()), 'Fixed solve schedule differs')
        need(previous <= utc(row['started_at']), 'Solve stage ordering overlaps')
        key = run.canonical_key(row)
        canonical[key] = old.verify_solve(e, row, binaries, canonical.get(key))
        shape = row['grain']
        need(row['case'] not in frontiers or frontiers[row['case']] == shape, 'Frontier differs within fixture')
        frontiers[row['case']] = shape
        previous = utc(row['verified_at'])
    need(previous <= utc(execution['ended_at']), 'Terminal measurement precedes last verification')
    need(canonical == execution['canonical'] and set(canonical) == {'smoke-narrow', *run.CASES}
         and len(e.states) == 3, 'Canonical final inventory differs')
    return {'schema': 'r1.sparse-rank-groups-report/v1', 'status': 'completed', 'payload_integrity': 'verified',
            'build_plan': e.files['plan.json'], 'measurement_plan': e.files['measurement.json'],
            'execution': e.files['execution.json'], 'build_host': build_host, 'measurement_host': e.plan['host'],
            'frontier_observations': frontiers, 'exact_state_and_quality_across_arms_workers': True,
            'counts': {'build_commands': 2, 'core_test_commands': 2, 'smoke': 4, 'canonical': 2, 'warmup': 8, 'measured': 24},
            **summarize(execution['stages']),
            'limitations': ['Fixed two synthetic Flop inputs; F32/DCFR16 and CFV capture disabled.',
                            'Same compiler, portable x86-64-v3 flags and depth2; separately built source-bound binaries.',
                            '16 guest physical cores and 32 logical CPUs do not prove dedicated or 32 physical cores.',
                            'CPU includes spin/allocation/scheduling; whole-process RSS is not per-phase memory.',
                            'Three samples, possibly subsecond CFR intervals; no adaptive retries or mixed boots.',
                            'A performance screen is not adoption, broader correctness, or target-exploitability certification.']}


# The reused solver/state verifier calls this stricter two-arm stage verifier.
old.verify_stage = verify_stage


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    need(not args.report.exists(), 'New report path required')
    try:
        report, code = check(args.out), 0
    except Exception as error:
        report, code = {'schema': 'r1.sparse-rank-groups-report/v1', 'status': 'not_evaluable', 'error': str(error),
                        'payload_integrity': 'not_verified', 'groups': [], 'performance_screen': 'not_evaluable',
                        'production_adoption': False, 'performance_claims': False}, 2
    with args.report.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(report, stream, indent=2, sort_keys=True, allow_nan=False)
        stream.write('\n')
    print(json.dumps({'status': report['status'], 'payload_integrity': report['payload_integrity']}))
    return code


if __name__ == '__main__':
    raise SystemExit(main())
