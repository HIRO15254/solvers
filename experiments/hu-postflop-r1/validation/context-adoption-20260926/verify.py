"""Offline check of exact raw bytes, Git-backed source, stage identities and outcomes."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def pin(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def main():
    manifest = json.loads((HERE / 'manifest.json').read_text())
    raw = {}
    for name, record in manifest['files'].items():
        compressed = (HERE / record['retained_path']).read_bytes()
        assert pin(compressed) == record['gzip'], name
        raw[name] = gzip.decompress(compressed)
        assert pin(raw[name]) == record['original'], name
    plan = json.loads(raw['plan.json'])
    result = json.loads(raw['result.json'])
    after = json.loads(raw['source-after.json'])
    assert len(plan['source_files']) == 199
    assert result['source_unchanged'] and after['matches_before']
    assert after['source_files'] == plan['source_files']
    assert pin(raw['dirty.patch']) == plan['dirty_patch']
    archive = subprocess.check_output(['git', 'archive', plan['source_commit'], '--',
                                       'Cargo.toml', 'Cargo.lock', '.cargo/config.toml', 'crates'], cwd=ROOT)
    with tarfile.open(fileobj=io.BytesIO(archive)) as stream:
        sources = {member.name: stream.extractfile(member).read() for member in stream if member.isfile()}
    assert set(sources) == set(plan['source_files'])
    for name in plan['changed_source_paths']:
        sources[name] = raw['changed-source/' + name]
    assert {name: pin(data) for name, data in sources.items()} == plan['source_files']
    expected = {str(Path(plan['cwd']) / p): v for p, v in plan['source_files'].items()}
    expected.update(plan['tools'])
    expected[str(Path(plan['output_directory']) / 'plan.json')] = pin(raw['plan.json'])
    assert pin((HERE / 'run.py').read_bytes()) == plan['tools'][str(HERE / 'run.py')]
    assert result['overall_elapsed_seconds'] <= 450
    assert plan['environment']['CARGO_TARGET_DIR'] == str(Path(plan['cwd']) / 'target/r1-local-tests')
    for key, value in {'CARGO_BUILD_JOBS': '1', 'RAYON_NUM_THREADS': '1', 'RUST_TEST_THREADS': '1',
                       'CARGO_INCREMENTAL': '0', 'CARGO_PROFILE_DEV_DEBUG': '0', 'CARGO_PROFILE_TEST_DEBUG': '0',
                       'CARGO_PROFILE_DEV_INCREMENTAL': 'false', 'CARGO_PROFILE_TEST_INCREMENTAL': 'false',
                       'CARGO_NET_OFFLINE': 'true', 'RUSTFLAGS': ''}.items():
        assert plan['environment'][key] == value, key
    reports = []
    for stage in result['stages']:
        label = stage['label']
        record = json.loads(raw['records/' + label + '.json'])
        base = label.removesuffix('-cleanup-confirmation')
        assert record['argv'] == plan['commands'][base]
        assert record['cwd'] == plan['cwd'] and record['shell'] is False
        assert record['identity_unchanged'] and record['identity_before'] == record['identity_after']
        assert {p['path']: {'bytes': p['bytes'], 'sha256': p['sha256']} for p in record['identity_before']} == expected
        for field in ['state', 'supervisor_exit_code', 'child_exit_code', 'elapsed_seconds', 'stop_reason']:
            assert stage[field] == record[field]
        assert record['cleanup_complete'] and record['last_sample']['pids'] == []
        limits = record['limits']
        assert limits['timeout_seconds'] == (30 if label != base else 120)
        assert limits['memory_limit_bytes'] == 768 * 1024**2
        assert limits['min_free_memory_bytes'] == 3 * 1024**3
        assert limits['disk_reserve_bytes'] == 4 * 1024**3
        assert limits['grace_seconds'] == limits['kill_wait_seconds'] == 5
        for name in ['stdout', 'stderr', 'samples']:
            suffix = {'stdout': '.stdout.log', 'stderr': '.stderr.log', 'samples': '.samples.jsonl'}[name]
            assert pin(raw['records/' + label + suffix]) == {k: record['outputs'][name][k] for k in ['bytes', 'sha256']}
        samples = [json.loads(line) for line in raw['records/' + label + '.samples.jsonl'].splitlines()]
        assert len(samples) == record['measurement']['sample_count']
        peak = max(s['tree_resident_bytes'] for s in samples)
        assert peak == record['measurement']['sampled_peak_tree_resident_bytes']
        passed = record['state'] == 'completed' and record['supervisor_exit_code'] == record['child_exit_code'] == 0
        if passed:
            assert not record['errors'] and not record['forced']
        else:
            assert record['state'] == 'failed' and record['supervisor_exit_code'] == 1 and record['child_exit_code'] == 0
            assert record['stop_reason'] == 'descendants_after_root_exit'
            assert len(record['errors']) == 1 and 'AttachConsole' in record['errors'][0]['message']
        counts = re.findall(rb'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored', raw['records/' + label + '.stdout.log'])
        tests = {key: sum(int(row[index]) for row in counts) for key, index in [('passed', 1), ('failed', 2), ('ignored', 3)]}
        if base == 'formats-tests':
            assert counts and all(row[0] == b'ok' for row in counts)
            assert tests == {'passed': 79, 'failed': 0, 'ignored': 0}, tests
            for name in ['finish_failure_preserves_sink_error',
                         'large_small_large_frames_do_not_carry_history_or_pledged_size',
                         'reused_context_matches_fresh_frames_at_stream_buffer_boundaries',
                         'write_all_failure_preserves_sink_error']:
                line = ('test sol_indexed::context_reuse_tests::' + name + ' ... ok').encode()
                assert line in raw['records/' + label + '.stdout.log'], name
            if label == base:
                compiled = ('Compiling formats v0.1.0 (' + str(Path(plan['cwd']) / 'crates/formats') + ')').encode()
                assert compiled in raw['records/' + label + '.stderr.log']
        reports.append({'stage': label, 'state': record['state'], 'cargo_exit': record['child_exit_code'],
                        'supervisor_exit': record['supervisor_exit_code'], 'elapsed_seconds': record['elapsed_seconds'],
                        'sampled_peak_tree_resident_bytes': peak, 'test_counts': tests if counts else None})
    confirmations = [s for s in result['stages'] if s['label'].endswith('-cleanup-confirmation')]
    assert len(confirmations) <= 1 and bool(confirmations) == result['confirmation_used']
    for stage in confirmations:
        base = stage['label'].removesuffix('-cleanup-confirmation')
        assert raw[base + '-confirmation-binaries-before.json'] == raw[base + '-confirmation-binaries-after.json']
    assert result['state'] == 'checks_completed'
    for name in ['fmt', 'clippy', 'formats-tests']:
        assert any(s['label'] in [name, name + '-cleanup-confirmation'] and s['state'] == 'completed' for s in result['stages'])
    print(json.dumps({'verified': True, 'source_commit': plan['source_commit'], 'source_count': 199,
                      'raw_files': len(raw), 'overall_elapsed_seconds': result['overall_elapsed_seconds'],
                      'stages': reports, 'scope': 'Windows shared-host checks only; full workspace tests and Linux acceptance not certified.'}, indent=2))


if __name__ == '__main__':
    main()
