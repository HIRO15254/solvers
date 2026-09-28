"""One bounded posthoc decode of existing VM20 data; original failed campaign stays failed."""
import argparse
import datetime as dt
import gzip
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import signal
import subprocess
import sys
import time

ROOT = Path('/opt/r1/flop-cpu-profile-proof01')
PACKAGE = Path('/opt/r1/flop-cpu-profile-package')
OUTPUT = Path('/opt/r1/flop-cpu-profile-posthoc01')
INSTANCE = '2562385330130146252'
SOURCE_PINS = {'run.py': (33317, 'e8c428023386f76cf3c719c467b980e7c4e36e72f408692d85fc90fe4eb1fa66'),
               'analyze.py': (24584, '5f7a03b4fc5bff751aa9843456a531d1c46ac0303e15ae4c55820562770d2c61')}
HEADER = re.compile(r'^\s*(\d+)\s*/\s*(\d+)\s+(\d+)\.(\d{9}):\s+(\d+)\s+cpu-clock:\s*([0-9a-fA-F]+)\s+(.+)\s+\((.*)\)\s*$')
CAP = 16 * 1024**2


def need(value, reason):
    if not value:
        raise ValueError(reason)


def pin(path):
    need(path.is_file() and not path.is_symlink(), 'Regular input required')
    sha, size = hashlib.sha256(), 0
    with path.open('rb') as stream:
        while block := stream.read(1024**2):
            size += len(block)
            sha.update(block)
    return {'bytes': size, 'sha256': sha.hexdigest()}


def read(path):
    need(path.stat().st_size <= 2 * 1024**2, 'Compact JSON bound exceeded')
    return json.loads(path.read_bytes())


def save(path, value):
    with path.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, indent=2, allow_nan=False)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())


def leaf_summary(text, phases):
    points = []
    for line in text.splitlines():
        if not line.strip():
            continue
        match = HEADER.fullmatch(line)
        need(match is not None, 'Unrecognized leaf-only or LOST record; no skipped lines')
        pid, tid, sec, ns, period, ip, symbol, dso = match.groups()
        need(int(period) > 0, 'Zero sample period')
        points.append((int(pid), int(tid), int(sec) * 10**9 + int(ns), int(period), ip, symbol.strip(), dso))
    need(points, 'No decoded samples')
    own = [p for p in points if p[0] == phases['pid']]
    start, end = (phases['phases']['cfr'][key] for key in ('start_ns', 'end_ns'))
    need(own and min(p[2] for p in own) < start and max(p[2] for p in own) >= end, 'Stream does not bracket full CFR')
    selected = [p for p in own if start <= p[2] < end]
    need(selected, 'No CFR samples')
    leaves = {}
    for p in selected:
        key = p[5] + ' (' + p[6] + ')'
        entry = leaves.setdefault(key, {'samples': 0, 'period_sum': 0})
        entry['samples'] += 1
        entry['period_sum'] += p[3]
    return {'all_samples': len(points), 'cfr_samples': len(selected), 'cfr_period_sum': sum(p[3] for p in selected),
            'tid_count': len({p[1] for p in selected}), 'exclusive_leaf_symbols_mangled': dict(sorted(leaves.items())),
            'unknown_leaf_samples': sum(p[5] in ('[unknown]', 'unknown', '0x0') or p[6] in ('[unknown]', 'unknown') for p in selected),
            'unknown_any_frame_samples': None, 'at_callchain_limit_samples': None,
            'scope': 'Exclusive physical leaf only; inline/callchain expansion disabled; unknown leaves retained'}


def capture(argv, name, deadline, allowed=(0,)):
    import resource
    stdout, stderr = (OUTPUT / (name + suffix) for suffix in ('.stdout.log', '.stderr.log'))
    record = {'argv': argv, 'started_at': dt.datetime.now(dt.timezone.utc).isoformat(), 'timeout_seconds': 20,
              'maximum_combined_output_bytes_for_success': CAP, 'hard_per_file_bytes': CAP,
              'maximum_combined_failure_bytes': CAP * 2, 'stop_reason': 'running'}
    process = None
    started = time.monotonic()
    try:
        with stdout.open('xb') as so, stderr.open('xb') as se:
            process = subprocess.Popen(argv, stdout=so, stderr=se, start_new_session=True,
                                       preexec_fn=lambda: resource.setrlimit(resource.RLIMIT_FSIZE, (CAP, CAP)))
            end = min(started + 20, deadline - 5)
            while process.poll() is None:
                if stdout.stat().st_size + stderr.stat().st_size > CAP:
                    record['stop_reason'] = 'output_cap'
                    raise ValueError('Posthoc output cap exceeded')
                if time.monotonic() >= end:
                    record['stop_reason'] = 'timeout'
                    raise TimeoutError('Posthoc helper deadline')
                time.sleep(.02)
            record['returncode'] = process.wait()
        need(record['returncode'] in allowed, 'Posthoc helper failed')
        need(stdout.stat().st_size + stderr.stat().st_size <= CAP, 'Posthoc final output cap exceeded')
        record['stop_reason'] = 'completed'
    except BaseException as error:
        record['error'] = repr(error)
        if record['stop_reason'] == 'running':
            record['stop_reason'] = 'nonzero_exit_or_final_output_cap'
        if process is not None and process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=3)
        record['returncode'] = None if process is None else process.returncode
        raise
    finally:
        record['elapsed_seconds'] = time.monotonic() - started
        record['ended_at'] = dt.datetime.now(dt.timezone.utc).isoformat()
        record['files'] = {k: {'path': str(p), **pin(p)} for k, p in (('stdout', stdout), ('stderr', stderr)) if p.exists()}
        save(OUTPUT / (name + '.json'), record)
    return stdout.read_text(), stderr.read_text(), record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    import resource
    resource.setrlimit(resource.RLIMIT_AS, (2 * 1024**3, 2 * 1024**3))
    need(len(os.sched_getaffinity(0)) == 2, 'Recovery must have exactly2 logical CPUs')
    OUTPUT.mkdir()
    (OUTPUT / 'posthoc-leaf.py').write_bytes(Path(__file__).read_bytes())
    deadline = time.monotonic() + 110
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('Global110s posthoc deadline')))
    signal.alarm(110)
    result = {'schema': 'r1.vm20-posthoc-leaf/v1', 'status': 'running', 'campaign_status': 'failed',
              'performance_comparison': 'not_evaluated', 'production_adoption': False,
              'solvers_rerun': 0, 'perf_record_rerun': 0, 'original_files_modified': None,
              'source': pin(Path(__file__)), 'started_at': dt.datetime.now(dt.timezone.utc).isoformat()}
    original_perf = ROOT / 'narrow-r0-w16/perf.data'
    before, original_inventory = None, None
    try:
        source_root = PACKAGE / 'experiments/hu-postflop-r1/flop-scaling/cpu-profile'
        for name, expected in SOURCE_PINS.items():
            need(pin(source_root / name) == dict(zip(('bytes', 'sha256'), expected)), 'Frozen reader/runner changed')
        spec = importlib.util.spec_from_file_location('posthoc_frozen_reader', source_root / 'analyze.py')
        trusted = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(trusted)
        run = trusted.run
        need(not (ROOT / 'retained.json').exists(), 'Expected failed campaign without final manifest')
        # This is an explicitly partial view, never a fabricated retained manifest.
        e = object.__new__(trusted.Evidence)
        e.root, e.states, e.files = ROOT, set(), {}
        total = 0
        for path in sorted(ROOT.rglob('*')):
            need(not path.is_symlink(), 'Original proof symlink')
            if path.is_file():
                total += path.stat().st_size
                need(total <= 512 * 1024**2, 'Partial original evidence exceeds512MiB')
                e.files[path.relative_to(ROOT).as_posix()] = pin(path)
            else:
                need(path.is_dir(), 'Nonregular proof input')
        e.plan = trusted.read(ROOT / 'plan.json')
        original_inventory = dict(e.files)
        e.origin = PurePosixPath(e.plan['output'])
        need(str(e.origin) == str(ROOT) and e.plan['host']['instance_id'] == INSTANCE, 'Original source host/root differs')
        stop = dt.datetime.fromisoformat(e.plan['stop_deadline_utc'].replace('Z', '+00:00')).timestamp()
        need(time.time() + 90 < stop, 'Insufficient original STOP recovery window')
        trusted.verify_plan(e)
        binaries, build = trusted.verify_build(e)
        trusted.verify_measurement(e, build)
        need(e.plan['host']['instance_id'] == INSTANCE and e.plan['host']['logical_cpus'] == 32, 'Measurement identity differs')
        execution = trusted.read(ROOT / 'execution.json')
        need(execution['status'] == 'failed' and execution['counts'] == {'completed': 3, 'failed': 1, 'skipped': 7}, 'Original failed schedule differs')
        need(execution['plan'] == e.files['measurement.json'] and execution['binaries'] == binaries, 'Execution binding differs')
        rows = execution['stages']
        expected = [{'name': 'perf-preflight-measure', 'kind': 'perf-preflight'}, *run.schedule()]
        need(len(rows) == 11 and all(all(row[k] == v for k, v in wanted.items()) for row, wanted in zip(rows, expected)), 'Original schedule differs')
        trusted.verify_preflight(e, rows[0], binaries)
        canonical = {}
        for row in rows[1:3]:
            need(row['kind'] == 'canonical', 'Expected2 canonical solves')
            canonical[row['case']] = trusted.verify_solve(e, row, binaries, None)
        need(execution['canonical'] == canonical and all(r['status'] == 'skipped' for r in rows[4:]), 'Canonical or skipped schedule differs')
        row = rows[3]
        need(row['name'] == 'narrow-r0-w16' and row['status'] == 'failed' and row['supervisor_exit'] == 0, 'Failed row identity differs')
        record = trusted.read(e.require(row['record']))
        run.terminal(record)
        need(record['argv'] == record['resolved_argv'] == row['command'] and record['shell'] is False, 'Successful child command differs')
        need(row['host_before'] == row['host_after'] == e.plan['host'] and row['environment'] == e.plan['environment'] and row['forbidden_environment'] == [], 'Profile host/environment differs')
        need(record['limits'] == {**e.plan['limits'], 'timeout_seconds': 90}, 'Original solver limits differ')
        need(row['expected_child_affinity'] == e.plan['host']['affinity']
             and record['runtime']['logical_cpus'] == 32 and record['runtime']['machine'] == 'x86_64', 'Child runtime/affinity differs')
        need(row['process_seconds'] == record['elapsed_seconds'] and 0 < row['process_seconds'] <= 91, 'Child elapsed timer differs')
        need(trusted.utc(e.plan['created_at']) <= trusted.utc(row['started_at']) <= trusted.utc(row['ended_at'])
             <= trusted.utc(e.plan['deadline_utc']), 'Original solver ran outside measurement deadline')
        output = ROOT / row['name'] / 'artifacts'
        child = [binaries['baseline']['path'], 'narrow', '16', '64', str(output)]
        perf = e.plan['tools']['perf']['path']
        need(row['child_command'] == child and row['command'] == run.perf_command(perf, original_perf, child), 'Profile recording command changed')
        need(pin(Path(binaries['baseline']['path'])) == run.pair(binaries['baseline']) and pin(Path(perf)) == run.pair(e.plan['tools']['perf']), 'Binary/perf changed')
        identities = {item['path']: run.pair(item) for item in record['identity_before']}
        need(identities.get(binaries['baseline']['path']) == run.pair(binaries['baseline'])
             and identities.get(perf) == run.pair(e.plan['tools']['perf'])
             and identities.get(str(ROOT / 'measurement.json')) == e.files['measurement.json'], 'Profile supervisor input identity differs')
        artifacts = {name: trusted.read(e.require(row['outputs'][name])) for name in ('result.json', 'invocation.json', 'quality.json', 'cpu.json', 'phases.json')}
        run.base.validate_values(row, artifacts['result.json'], {**artifacts['invocation.json'], 'quality_chance_depth': 2}, artifacts['quality.json'])
        run.validate_cpu(row, artifacts['cpu.json'], artifacts['result.json'], row['expected_child_affinity'])
        run.validate_phases(row, artifacts['phases.json'], artifacts['result.json'])
        need(artifacts['result.json'] == row['result'] and artifacts['cpu.json'] == row['cpu'] and artifacts['phases.json'] == row['phases'], 'Result/phase receipt differs')
        need(e.require(row['outputs']['quality.json']).read_bytes() == e.require(canonical['narrow']['quality']).read_bytes(), 'Full canonical quality differs')
        failure = trusted.read(e.require(execution['failed_state_capture']))
        need(failure['fullbyte_compression_verified'] is True and failure['raw_removed'] is True and failure['original'] == row['outputs']['state.bin'], 'Failed state capture differs')
        with gzip.open(e.require(failure['gzip']), 'rb') as stream:
            state = trusted.digest(stream, row['outputs']['state.bin']['bytes'])
        need(state == run.pair(row['outputs']['state.bin']) == run.pair(canonical['narrow']['state']), 'Full failed-row state differs from canonical')
        before = pin(original_perf)
        need(before['bytes'] == 1167196 and before == e.files['narrow-r0-w16/perf.data'], 'Raw perf identity/size differs')
        result['validated_originals'] = {'instance_id': INSTANCE, 'binary': binaries['baseline'], 'build_plan': e.files['plan.json'],
            'measurement_plan': e.files['measurement.json'], 'execution': e.files['execution.json'], 'perf_data': before,
            'full_state': state, 'full_quality': run.pair(row['outputs']['quality.json']), 'canonical_solves_verified': 2,
            'profile_child_supervisor': run.pair(row['record']), 'original_inventory': e.files}
        # Distinct fresh outputs: do not append to or replace the failed script/dump.
        helpout, helperr, _ = capture([perf, 'script', '-h'], 'help', deadline, (0, 129))
        need(all(option in helpout + helperr for option in ('--demangle', '--inline', '--hide-call-graph')), 'Installed perf options unavailable')
        command = [perf, 'script', '--ns', '--show-lost-events', '--no-demangle', '--no-inline', '-G', '-F', run.PERF_FIELDS, '-i', str(original_perf)]
        text, errors, _ = capture(command, 'leaf', deadline)
        need(not errors.strip(), 'Leaf decoder stderr requires review')
        sample = leaf_summary(text, artifacts['phases.json'])
        dump, errors, _ = capture([perf, 'script', '--no-demangle', '--no-inline', '-G', '-D', '-i', str(original_perf)], 'dump', deadline)
        need(not errors.strip(), 'Dump decoder stderr requires review')
        census = run.record_census(dump)
        need(census['record_counts']['PERF_RECORD_SAMPLE'] == sample['all_samples'] and not census['loss_or_throttle_records_present'], 'Incomplete/lost/throttled sample census')
        attrs, errors, _ = capture([perf, 'evlist', '-v', '-i', str(original_perf)], 'evlist', deadline)
        need(not errors.strip(), 'evlist stderr requires review')
        run.validate_attributes(attrs)
        need(pin(original_perf) == before, 'Original perf changed')
        result.update(status='single_existing_profile_posthoc_validated', observation={'case': 'narrow', 'workers': 16, 'round': 0,
            'iterations': 64, 'sampling': sample, 'census': census, 'result': artifacts['result.json'], 'cpu': artifacts['cpu.json'],
            'phases': artifacts['phases.json']}, limits=['Original campaign failed after3 completed stages;7 skipped. No complete8-profile or scaling claim.',
            'Posthoc decoding changes textual rendering only; no new record or solver execution.',
            'Exclusive mangled physical leaf symbols; no inclusive, inline, callchain completeness or memory-bandwidth inference.',
            'This validates fixed-iteration identical state/quality against same-input canonical, not convergence or external quality.'])
    except BaseException as error:
        result.update(status='posthoc_failed_not_evaluated', error=repr(error))
    finally:
        signal.alarm(0)
        result['ended_at'] = dt.datetime.now(dt.timezone.utc).isoformat()
        try:
            if before is not None:
                result['original_perf_after'] = pin(original_perf)
                need(result['original_perf_after'] == before, 'Original perf changed after decode')
            if original_inventory is not None:
                after = {}
                for path in ROOT.rglob('*'):
                    need(not path.is_symlink(), 'Original input became symlink')
                    if path.is_file():
                        after[path.relative_to(ROOT).as_posix()] = pin(path)
                result['original_files_modified'] = after != original_inventory
                need(after == original_inventory, 'Original evidence changed during posthoc decode')
        except BaseException as error:
            result.update(status='posthoc_failed_not_evaluated', original_files_modified=True, final_identity_error=repr(error))
        result['outputs'] = {p.name: pin(p) for p in OUTPUT.iterdir() if p.is_file()}
        save(OUTPUT / 'report.json', result)
    print(json.dumps({'status': result['status'], 'report': pin(OUTPUT / 'report.json'), 'error': result.get('error')}))
    return int(result['status'] != 'single_existing_profile_posthoc_validated')


def self_test():
    import unittest
    class Tests(unittest.TestCase):
        def test_leaf_period_unknown_and_cfr_clip(self):
            text = '\n'.join(['10/10 1.000000000: 3 cpu-clock: aa outside (/x)', '10/11 2.000000000: 5 cpu-clock: bb _Rname (/x)', '10/12 2.500000000: 7 cpu-clock: cc [unknown] ([unknown])', '10/10 3.000000000: 9 cpu-clock: dd outside (/x)'])
            result = leaf_summary(text, {'pid': 10, 'phases': {'cfr': {'start_ns': 2000000000, 'end_ns': 3000000000}}})
            self.assertEqual((result['all_samples'], result['cfr_samples'], result['cfr_period_sum'], result['unknown_leaf_samples']), (4, 2, 12, 1))
            self.assertIsNone(result['unknown_any_frame_samples'])
        def test_reject_unbracketed(self):
            with self.assertRaises(ValueError):
                leaf_summary('10/10 2.000000000: 3 cpu-clock: aa name (/x)', {'pid': 10, 'phases': {'cfr': {'start_ns': 2000000000, 'end_ns': 3000000000}}})
        def test_reject_chain_or_lost_line(self):
            for line in (' aa name (/x)', 'PERF_RECORD_LOST 2'):
                with self.assertRaises(ValueError):
                    leaf_summary(line, {})
    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests))
    need(result.wasSuccessful(), 'Pure metadata tests failed')


if __name__ == '__main__':
    raise SystemExit(main())
