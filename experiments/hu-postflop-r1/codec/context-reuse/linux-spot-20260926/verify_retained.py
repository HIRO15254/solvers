"""Verify VM08 evidence without the VM, toolchain, source checkout or original paths."""
from __future__ import annotations

import argparse
import datetime as dt
import gzip
import io
import itertools
import json
import math
from pathlib import Path, PurePosixPath
import random
import re
import statistics
import tarfile

from retain import content, file_pin, fingerprint, read_json, require

HERE = Path(__file__).resolve().parent
SIGINT = 'a_canceled_heads_up_solve_closes_as_canceled_and_resumes'
EXAMPLE = 'crates/formats/examples/sol_codec_bench.rs'
CONTEXT_TESTS = ['finish_failure_preserves_sink_error',
                 'large_small_large_frames_do_not_carry_history_or_pledged_size',
                 'reused_context_matches_fresh_frames_at_stream_buffer_boundaries',
                 'write_all_failure_preserves_sink_error']
IDENTITY_ONLY = {'cargo', 'rustc', 'rustdoc', 'clippy-driver', 'rustfmt', 'python'}


def join(root, *names):
    return str(PurePosixPath(root).joinpath(*names))


class Store:
    def __init__(self, directory):
        self.root = Path(directory)
        self.manifest = read_json((self.root / 'manifest.json').read_bytes())
        require(self.manifest['schema'] == 'r1.context-linux-retention/v1', 'wrong retention schema')
        self.blobs, self.paths, self.virtual = self.manifest['blobs'], self.manifest['originals'], {}
        for name, entry in self.blobs.items():
            require(re.fullmatch(r'blobs/[0-9a-f]{64}\.gz', name), 'unsafe blob path')
            require(name == 'blobs/' + entry['decoded']['sha256'] + '.gz', 'blob name differs from original SHA')
            require(entry['encoding'] == 'gzip' and file_pin(self.root / name) == content(entry), 'gzip hash differs')
            with gzip.open(self.root / name, 'rb') as stream:
                require(fingerprint(stream) == entry['decoded'], 'original payload hash differs')
        collector = self.manifest['collector']
        original = read_json(self.blob(collector['manifest_blob']))
        require(original['schema'] == 'solvers.r1-retention/v1', 'wrong collector schema')
        require(original['archive_filename'] == collector['archive_name'], 'collector filename differs')
        digest_line = self.blob(collector['sha256_blob']).decode('ascii').strip()
        require(digest_line == collector['archive']['sha256'] + '  ' + collector['archive_name'], 'archive SHA sidecar differs')
        rows = original['files']
        require(len({r['original_path'] for r in rows}) == len(rows), 'duplicate original path')
        included_members = [r['archive_member'] for r in rows if r['included']]
        require(len(set(included_members)) == len(included_members), 'duplicate collector member')
        included_blobs = {'blobs/' + r['sha256'] + '.gz' for r in rows if r['included']}
        expected, recovered, missing = {}, [], []
        for row in rows:
            if row['kind'] != 'regular':
                require(not row['included'], 'retained non-file')
                continue
            blob = 'blobs/' + row['sha256'] + '.gz'
            available = blob in included_blobs and blob in self.blobs and self.blobs[blob]['decoded'] == content(row)
            require(not row['included'] or available, 'included payload missing')
            if available:
                expected[row['original_path']] = blob
                if not row['included']:
                    recovered.append(row['original_path'])
            else:
                missing.append(row['original_path'])
        require(expected == self.paths, 'retained original aliases differ from collector')
        require(recovered == collector['hash_recovered_paths'] and missing == collector['missing_paths'], 'availability differs')
        require(set(self.blobs) == set(self.paths.values()) | {collector['manifest_blob'], collector['sha256_blob']}, 'unreferenced retained blob')

    def blob(self, name):
        require(self.blobs[name]['decoded']['bytes'] <= 64 * 1024**2, 'payload requires streaming access')
        return gzip.decompress((self.root / name).read_bytes())

    def open(self, path):
        if path in self.virtual:
            return io.BytesIO(self.virtual[path])
        require(path in self.paths, 'required original payload unavailable: ' + path)
        return gzip.open(self.root / self.paths[path], 'rb')

    def raw(self, path):
        with self.open(path) as stream:
            data = stream.read(64 * 1024**2 + 1)
        require(len(data) <= 64 * 1024**2, 'payload requires streaming access')
        return data

    def read(self, path):
        return read_json(self.raw(path))

    def identity(self, path):
        value = fingerprint(io.BytesIO(self.virtual[path])) if path in self.virtual else self.blobs[self.paths[path]]['decoded']
        return {'path': path, **value}

    def bound(self, reference):
        require(self.identity(reference['path']) == reference, 'payload binding differs: ' + reference['path'])

    def add(self, path, data):
        if path in self.paths or path in self.virtual:
            require(self.raw(path) == data, 'archive and separately retained source bytes differ')
        self.virtual[path] = data

    def equal(self, first, second):
        # Identical aliases resolve to the very same verified original bytes.
        if first in self.paths and second in self.paths and self.paths[first] == self.paths[second]:
            return
        with self.open(first) as a, self.open(second) as b:
            while True:
                left, right = a.read(1024 * 1024), b.read(1024 * 1024)
                require(left == right, 'original bytes differ: ' + first + ' / ' + second)
                if not left:
                    break


def archive_files(raw, expected, directories):
    result = {}
    with tarfile.open(fileobj=io.BytesIO(raw), mode='r:gz') as archive:
        members = archive.getmembers()
        require(len({m.name for m in members}) == len(members), 'duplicate source tar member')
        require({m.name for m in members} == set(expected) | set(directories), 'source tar exact set differs')
        for member in members:
            path = PurePosixPath(member.name)
            require(not path.is_absolute() and '..' not in path.parts and '\\' not in member.name, 'unsafe source member')
            if member.name in directories:
                require(member.isdir(), 'wrong directory member')
            else:
                require(member.isfile(), 'source must be a regular file')
                data = archive.extractfile(member).read()
                require(fingerprint(io.BytesIO(data)) == expected[member.name], 'source archive bytes differ')
                result[member.name] = data
    return result


def schedule(protocol):
    rng, rows = random.Random(protocol['seed']), []
    arms = protocol['arms']
    for index, case in enumerate(protocol['inputs']):
        warm = arms[index:] + arms[:index]
        rows += [{'case': case, 'block': 0, 'arm': arm, 'warmup': True} for arm in warm]
        blocks = list(itertools.permutations(arms)) + [tuple(warm)]
        rng.shuffle(blocks)
        for number, block in enumerate(blocks, 1):
            rows += [{'case': case, 'block': number, 'arm': arm, 'warmup': False} for arm in block]
    return rows


def stages(plan):
    tool, source = plan['tools'], plan['sources']['candidate']['root']
    cargo, python = tool['cargo']['path'], tool['python']['path']
    commands = [
        ('toolchain', [tool['rustc']['path'], '-Vv'], 30),
        ('fmt', [cargo, 'fmt', '--all', '--check'], 60),
        ('clippy', [cargo, 'clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings'], 1800),
        ('workspace-tests', [cargo, 'test', '--locked', '--workspace', '--', '--test-threads=2'], 2400),
        ('docs', [python, 'tools/check_docs.py'], 60),
        ('python-tools', [python, '-m', 'unittest', 'discover', '-s', 'tools/tests', '-v'], 300),
        ('sigint-build', [cargo, 'test', '--locked', '--release', '-p', 'cli', '--test', 'cli_integration', SIGINT, '--no-run'], 1800),
        ('sigint-test', [cargo, 'test', '--locked', '--release', '-p', 'cli', '--test', 'cli_integration', SIGINT, '--', '--ignored', '--exact', '--test-threads=1'], 180)]
    result = [{'label': label, 'arm': 'candidate', 'kind': 'validation', 'argv': argv, 'cwd': source, 'timeout': timeout}
              for label, argv, timeout in commands]
    for arm in plan['protocol']['arms']:
        result.append({'label': 'release-' + arm, 'arm': arm, 'kind': 'build', 'cwd': plan['sources'][arm]['root'],
                       'timeout': 1800, 'argv': [cargo, 'build', '--locked', '--release', '-p', 'formats', '--example', 'sol_codec_bench']})
    for item in plan['order']:
        label = f"{item['case']}-{item['block']}-{item['arm']}"
        result.append({**item, 'label': label, 'kind': 'sample', 'cwd': plan['output'],
                       'timeout': plan['protocol']['limits']['sample_seconds'],
                       'argv': [join(plan['output'], 'binaries', item['arm']), plan['inputs'][item['case']]['path'],
                                'stream-write', '1', join(plan['output'], 'stages', label, 'output')]})
    return result


def validation(stage, record, store):
    text = store.raw(record['outputs']['stdout']['path']).decode()
    label = stage['label']
    rows = [list(map(int, row)) for row in re.findall(
        r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out', text)]
    if label == 'toolchain':
        require('release: 1.97.0' in text and 'host: x86_64-unknown-linux-gnu' in text, 'wrong recorded toolchain')
    if label in ('workspace-tests', 'sigint-test'):
        require(rows and sum(row[0] for row in rows) > 0 and all(row[1] == 0 for row in rows), 'Rust tests did not pass')
    if label == 'workspace-tests':
        for name in CONTEXT_TESTS:
            require(text.count('test sol_indexed::context_reuse_tests::' + name + ' ... ok') == 1, 'current context test missing/duplicated')
    if label == 'sigint-test':
        require('test ' + SIGINT + ' ... ok' in text and len(rows) == 1 and rows[0][:3] == [1, 0, 0], 'SIGINT test did not execute')
    return {'test_summaries': rows, 'totals': [sum(row[index] for row in rows) for index in range(5)]}


def compare(plan, state, store):
    metrics = {}
    for case in plan['protocol']['inputs']:
        raw = {arm: [] for arm in plan['protocol']['arms']}
        reference = None
        for entry in state['stages']:
            stage = entry['stage']
            if stage['kind'] != 'sample' or stage['case'] != case:
                continue
            files = entry['sample']['files']
            reference = reference or files
            for name in ('canonical.bin', 'root-canonical.bin', 'rewritten.sol'):
                require(content(files[name]) == content(reference[name]), 'cross-arm byte hash differs')
                store.equal(files[name]['path'], reference[name]['path'])
            if not stage['warmup']:
                raw[stage['arm']].append((stage['block'], entry['sample']['operation_seconds']))
        seconds = {arm: [value for _, value in sorted(rows)] for arm, rows in raw.items()}
        require(all(len(rows) == 7 for rows in seconds.values()), 'missing measured block')
        medians = {arm: statistics.median(rows) for arm, rows in seconds.items()}
        comparisons = {}
        for a, b in [('candidate', 'bulk'), ('candidate', 'legacy'), ('bulk', 'legacy')]:
            comparisons[a + '_over_' + b] = {'median_ratio': medians[a] / medians[b],
                'paired_ratios': [x / y for x, y in zip(seconds[a], seconds[b])],
                'strictly_faster_blocks': sum(x < y for x, y in zip(seconds[a], seconds[b]))}
        metrics[case] = {'seconds': seconds, 'medians': medians, 'comparisons': comparisons}
    screen = plan['protocol']['screen']
    supports = all(c['comparisons']['candidate_over_bulk']['median_ratio'] <= screen['all_cases_candidate_over_bulk_max']
                   and c['comparisons']['candidate_over_legacy']['median_ratio'] <= screen['all_cases_candidate_over_legacy_max'] for c in metrics.values())
    flop = metrics['flop']['comparisons']['candidate_over_bulk']
    supports = supports and flop['median_ratio'] <= screen['flop_candidate_over_bulk_max'] and flop['strictly_faster_blocks'] >= screen['flop_strictly_faster_blocks_min']
    return {'cases': metrics, 'descriptive_adoption_screen': supports, 'r1_acceptance': None,
            'canonical_and_original_sol_byte_equality': True}


def verify(directory, expect=None):
    store = Store(directory)
    candidates = [path for path in store.paths if path.endswith('/plan.json')
                  and store.read(path).get('schema') == 'r1.context-linux-plan/v1']
    require(len(candidates) == 1, 'exactly one VM08 plan is required')
    plan = store.read(candidates[0])
    root = plan['output']
    require(candidates[0] == join(root, 'plan.json'), 'plan path differs')
    state = store.read(join(root, 'result.json'))
    require(state['schema'] == 'r1.context-linux-result/v1', 'wrong result schema')
    store.bound(state['plan'])
    protocol = plan['protocol']
    require(protocol == read_json((HERE / 'protocol.json').read_bytes()), 'fixed protocol differs')
    require(plan['order'] == schedule(protocol) and len(plan['order']) == 72, 'fixed order differs')
    require(plan['host']['logical_cpus'] == 2 and plan['outer_cgroup_at_build']['effective_memory_max_bytes'] <= 6 * 1024**3, 'host/containment differs')
    require(plan['environment'] == {'CARGO_HOME': '/opt/r1/cargo', 'RUSTUP_HOME': '/opt/r1/rustup',
            'RUSTUP_TOOLCHAIN': '1.97.0', 'CARGO_BUILD_JOBS': '1', 'RAYON_NUM_THREADS': '1',
            'RUST_TEST_THREADS': '2', 'CARGO_PROFILE_DEV_DEBUG': '0', 'CARGO_PROFILE_TEST_DEBUG': '0',
            'CARGO_INCREMENTAL': '0', 'RUSTC': plan['tools']['rustc']['path'],
            'RUSTDOC': plan['tools']['rustdoc']['path']}, 'planned build environment differs')
    source_roots = []
    for arm, source in plan['sources'].items():
        source_roots.append(PurePosixPath(source['root']))
        store.bound(source['archive'])
        store.bound(source['manifest'])
        manifest = store.read(source['manifest']['path'])
        require(manifest['base_commit'] == source['revision'], 'source revision differs')
        if arm in protocol['revisions']:
            require(source['revision'] == protocol['revisions'][arm], 'wrong source revision')
        require(content(source['archive']) == {'bytes': manifest['archive_bytes'], 'sha256': manifest['archive_sha256']}, 'archive/manifest binding differs')
        pins = {row['path']: content(row) for row in manifest['files']}
        require(len(pins) == len(manifest['files']) and pins == source['files'], 'source exact set differs')
        files = archive_files(store.raw(source['archive']['path']), pins, manifest.get('directory_entries', []))
        for name, data in files.items():
            store.add(join(source['root'], name), data)
        require(pins[EXAMPLE] == protocol['example'], 'benchmark source differs')
        if arm == 'candidate':
            require(pins['crates/formats/src/sol_indexed.rs'] == protocol['candidate_writer'], 'candidate writer differs')
    require(set(plan['sources']) == set(protocol['arms']) and len(set(source_roots)) == 3, 'source arms differ')
    require(all(not a.is_relative_to(b) for a in source_roots for b in source_roots if a != b), 'nested source roots')
    targets = [PurePosixPath(v) for v in plan['targets'].values()]
    require(set(plan['targets']) == set(protocol['arms']) and len(set(targets)) == 3, 'target separation differs')
    require(len({target.parent for target in targets}) == 1
            and all(PurePosixPath(plan['targets'][arm]).name == arm for arm in protocol['arms']), 'target arm mapping differs')
    require(all(not target.is_relative_to(source) for target in targets for source in source_roots), 'target inside source')
    # Inputs may be separate collector files or recovered exactly from the fixed input archive.
    if any(p['path'] not in store.paths for p in plan['inputs'].values()):
        input_archive = join(str(PurePosixPath(plan['sources']['candidate']['archive']['path']).parent), 'inputs.tar.gz')
        files = archive_files(store.raw(input_archive), {case + '.sol': expected for case, expected in protocol['inputs'].items()}, [])
        for case, ref in plan['inputs'].items():
            store.add(ref['path'], files[case + '.sol'])
    for case, reference in plan['inputs'].items():
        require(content(reference) == protocol['inputs'][case], 'fixed input differs')
        store.bound(reference)
        require(store.raw(reference['path'])[:10] == b'SLVRSOLV\x03\x00', 'wrong SOL wire version')
    require(set(plan['inputs']) == set(protocol['inputs']), 'input cases differ')
    for name, reference in plan['tools'].items():
        if name not in IDENTITY_ONLY:
            store.bound(reference)
    require(store.read(plan['tools']['protocol']['path']) == protocol, 'retained protocol differs')
    configuration = store.read(plan['tools']['sources_config']['path'])
    require(configuration == {arm: {'root': s['root'], 'revision': s['revision'], 'archive': s['archive']['path'], 'manifest': s['manifest']['path']} for arm, s in plan['sources'].items()}, 'source configuration binding differs')
    all_stages = stages(plan)
    require(set(state['binaries']).issubset(protocol['arms']), 'unexpected frozen binary arm')
    for arm, binary in state['binaries'].items():
        require(binary['path'] == join(root, 'binaries', arm), 'frozen binary arm path differs')
        store.bound(binary)
    terminal = state['status'] in ('ready_for_measurement', 'completed')
    expected_count = 11 if state['status'] == 'ready_for_measurement' else 83
    require(len(state['stages']) <= len(all_stages), 'extra stage')
    if terminal:
        require(len(state['stages']) == expected_count, 'missing terminal stage')
    stage_reports = []
    previous_end = None
    for entry, stage in zip(state['stages'], all_stages):
        require(entry['stage'] == stage and entry['label'] == stage['label'], 'stage order/command differs')
        if entry['status'] != 'passed':
            require(not terminal, 'terminal run contains failed/running stage')
            continue
        store.bound(entry['record'])
        record = store.read(entry['record']['path'])
        require(entry['supervisor_exit'] == 0, 'saved supervisor exit differs')
        require(record['state'] == 'completed' and record['supervisor_exit_code'] == record['child_exit_code'] == 0
                and record['cleanup_complete'] and not record['forced'] and not record['errors'], 'supervisor did not complete cleanly')
        require(record['argv'] == stage['argv'] and record['cwd'] == stage['cwd'], 'supervisor command differs')
        require(entry['host_before'] == entry['host_after'] == plan['host'], 'boot/CPU changed')
        fixed = [state['plan'], plan['tools']['runner'], plan['tools']['protocol'], plan['tools']['rustc'],
                 *plan['inputs'].values(), plan['tools']['python'], plan['tools']['supervisor']]
        executable = next((v for v in plan['tools'].values() if v['path'] == stage['argv'][0]), None)
        executable = executable or state['binaries'][stage['arm']]
        fixed.append(executable)
        expected_pins = {v['path']: v for v in fixed}
        for when in ['identity_before', 'identity_after']:
            actual = {v['path']: v for v in record[when]}
            require(len(actual) == len(record[when]) and actual == expected_pins, 'before/after executable/input binding differs')
        require(record['identity_unchanged'] and record['identity_before'] == record['identity_after'], 'identity changed during execution')
        limits = record['limits']
        require(limits['timeout_seconds'] == stage['timeout'] and limits['memory_limit_bytes'] == 6 * 1024**3
                and limits['min_free_memory_bytes'] == 768 * 1024**2 and limits['disk_reserve_bytes'] == 4 * 1024**3
                and limits['grace_seconds'] == limits['kill_wait_seconds'] == 5, 'stage limits differ')
        for reference in record['outputs'].values():
            store.bound(reference)
        count, peak, elapsed, last = 0, 0, -1, None
        with store.open(record['outputs']['samples']['path']) as samples:
            for line in samples:
                sample = read_json(line)
                require(sample['elapsed_seconds'] >= elapsed, 'sampling order differs')
                elapsed = sample['elapsed_seconds']
                count, peak, last = count + 1, max(peak, sample['tree_resident_bytes']), sample
        require(count == record['measurement']['sample_count'] and peak == record['measurement']['sampled_peak_tree_resident_bytes'], 'sample count/peak differs')
        require(last == record['last_sample'] and last['pids'] == [], 'final cleanup sample differs')
        started, ended = dt.datetime.fromisoformat(record['started_at']), dt.datetime.fromisoformat(record['ended_at'])
        require(started <= ended <= dt.datetime.fromisoformat(plan['deadline_utc']), 'deadline/chronology differs')
        require(previous_end is None or previous_end <= started, 'stage overlap')
        previous_end = ended
        summary = {'label': stage['label'], 'elapsed_seconds': record['elapsed_seconds'], 'sampled_full_process_peak_rss_bytes': peak}
        if stage['kind'] == 'validation':
            summary['rust_tests'] = validation(stage, record, store)
            require(summary['rust_tests'] == entry['validation'], 'saved test totals differ')
        elif stage['kind'] == 'build':
            binary = state['binaries'][stage['arm']]
            store.bound(binary)
            require(content(entry['compiled_binary']) == content(binary), 'compiled/frozen binary differs')
            require(entry['compiled_binary']['path'] == join(plan['targets'][stage['arm']], 'release/examples/sol_codec_bench'), 'compiled target path differs')
        else:
            binary = state['binaries'][stage['arm']]
            store.bound(binary)
            output = stage['argv'][-1]
            report = store.read(join(output, 'result.json'))
            require(store.read(record['outputs']['stdout']['path']) == report, 'benchmark stdout/result differs')
            require(report['schema'] == 'r1.sol-codec-sample/v1' and report['status'] == 'completed'
                    and report['operation'] == 'stream-write' and report['iterations'] == 1
                    and report['format_version'] == 3 and report['metadata']['mode'] == 'Full', 'sample report differs')
            duration = report['timing']['operation_seconds']
            require(type(duration) in (float, int) and math.isfinite(duration) and duration > 0, 'invalid duration')
            require(duration <= record['elapsed_seconds'], 'writer interval exceeds supervised process interval')
            files = {name: store.identity(join(output, name)) for name in ['result.json', 'rewritten.sol', 'canonical.bin', 'root-canonical.bin']}
            require({'operation_seconds': duration, 'files': files} == entry['sample'], 'saved sample identity/timing differs')
            input_pin = plan['inputs'][stage['case']]
            require(content(files['rewritten.sol']) == content(input_pin), 'original SOL hash differs')
            store.equal(files['rewritten.sol']['path'], input_pin['path'])
            require(report['input']['bytes'] == report['rewritten']['bytes'] == input_pin['bytes'], 'SOL length differs')
            for name, field in [('canonical.bin', 'canonical'), ('root-canonical.bin', 'root_canonical')]:
                require(report[field]['file'] == name and report[field]['bytes'] == files[name]['bytes'], 'canonical output differs')
        stage_reports.append(summary)
    comparison = None
    if state['status'] == 'completed':
        comparison = compare(plan, state, store)
        require(comparison == state['comparison'], 'saved timing comparison differs')
    if terminal:
        require(set(state['binaries']) == set(protocol['arms']), 'missing frozen binary')
    if expect:
        require(state['status'] == {'build': 'ready_for_measurement', 'complete': 'completed'}[expect], 'requested execution phase not completed')
    return {'payload_integrity_verified': True, 'run_status': state['status'], 'terminal_phase_verified': terminal,
            'stage_count': len(stage_reports), 'sample_count': sum(s['stage']['kind'] == 'sample' and s['status'] == 'passed' for s in state['stages']),
            'sample_attempts': sum(s['stage']['kind'] == 'sample' for s in state['stages']),
            'original_aliases': len(store.paths), 'gzip_blobs': len(store.blobs), 'stages': stage_reports, 'comparison': comparison,
            'limitations': ['Compiler/Python executables are identity-only; their original bytes are not retained.',
                            'Compiled target binaries are represented by their verified byte-identical frozen copies.',
                            'Source archives preserve the pinned inputs; live source checks were performed by the retained runner.',
                            'Sampled RSS covers the full process, including canonical generation; it is not writer-phase memory.',
                            'This does not certify solver quality, R1 acceptance, or the historical 126-process experiment.']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--expect', choices=('build', 'complete'))
    args = parser.parse_args()
    print(json.dumps(verify(args.directory, args.expect), indent=2, allow_nan=False))


if __name__ == '__main__':
    main()
