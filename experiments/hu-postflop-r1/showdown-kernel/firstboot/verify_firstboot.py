"""Read-only first-boot inventory and execution-binding audit; no retained code runs.

Exit 2 is expected for this historical bundle: its inventory verifies, but
three collected outputs differ from the completed process records' pins.
"""
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile

BUNDLE_SHA = 'a17f29921feb237c7266342796bd9f100d472d1ed7904d9ceb1d552af48fa141'
MANIFEST_SHA = '80d0fd55d9cc4d4931ce4bc24aa345947e4b3454192e943d9f9616b817500e25'
BOOT = '02dcc8e8-f1c9-4ec6-a657-f0f9b12c0f0f'
RUNNER = '/tmp/r1-kernel-old/validate.py'
RUNNER_SOURCE = 'experiments/hu-postflop-r1/range-scaling/validate.py'
NEW_LABELS = ['toolchain', 'fmt', 'clippy', 'workspace-tests', 'docs',
              'release-example', 'release-oracle', 'release-river-resolve']


def identity(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def check(data, ref):
    assert identity(data) == {k: ref[k] for k in ('bytes', 'sha256')}, ref


def unpack(data):
    files, directories = {}, []
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
        seen = set()
        for member in archive:
            name = member.name
            path = PurePosixPath(name)
            assert name not in seen and not path.is_absolute() and '..' not in path.parts
            seen.add(name)
            if member.isdir():
                directories.append(name)
            else:
                assert member.isfile(), name
                files[name] = archive.extractfile(member).read()
    return files, directories


def counts(stdout):
    rows = re.findall(
        r'^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; '
        r'(\d+) measured; (\d+) filtered out;', stdout, re.M)
    if not rows:
        return None
    assert all(row[0] == 'ok' for row in rows)
    names = re.findall(r'^test (.+?) \.\.\. (ok|FAILED|ignored(?:,.*)?)$', stdout, re.M)
    result = dict(zip(('passed', 'failed', 'ignored', 'measured', 'filtered_out'),
                      (sum(int(row[i]) for row in rows) for i in range(1, 6))))
    assert result['failed'] == 0
    assert sum(status == 'ok' for _, status in names) == result['passed']
    assert sum(status.startswith('ignored') for _, status in names) == result['ignored']
    assert len({name for name, _ in names}) == len(names)
    return dict(result, summary_rows=len(rows), test_names=[name for name, status in names if status == 'ok'])


def expected_argv(label, result):
    cargo = result['tools']['cargo']['path']
    target = result['target']
    commands = {
        'toolchain': [result['tools']['rustc']['path'], '-Vv'],
        'fmt': [cargo, 'fmt', '--all', '--check'],
        'clippy': [cargo, 'clippy', '--workspace', '--all-targets', '--target-dir', target, '--', '-D', 'warnings'],
        'workspace-tests': [cargo, 'test', '--workspace', '--no-fail-fast', '--target-dir', target],
        'docs': ['/usr/bin/python3', '-B', 'tools/check_docs.py'],
        'release-example': [cargo, 'build', '--release', '-p', 'cli', '--example', 'hu_scaling_bench', '--target-dir', target],
        'release-oracle': [cargo, 'test', '--release', '-p', 'holdem', '--test', 'oracle_diff', '--target-dir', target, '--', '--include-ignored'],
        'release-river-resolve': [cargo, 'test', '--release', '-p', 'cli', '--lib', '--target-dir', target, 'sol::tests::river_resolve_accuracy', '--', '--exact', '--ignored'],
    }
    return commands[label]


def verify():
    root = Path(__file__).resolve().parent
    bundle = (root / 'kernel-firstboot.tar.gz').read_bytes()
    external = (root / 'kernel-firstboot.tar.gz.manifest.json').read_bytes()
    check(bundle, {'bytes': 4557773, 'sha256': BUNDLE_SHA})
    assert identity(external)['sha256'] == MANIFEST_SHA
    assert (root / 'kernel-firstboot.tar.gz.sha256').read_text().split() == [BUNDLE_SHA, 'kernel-firstboot.tar.gz']
    packed, dirs = unpack(bundle)
    assert not dirs and packed['retention-manifest.json'] == external
    manifest = json.loads(external)
    records = manifest['files']
    assert len(records) == 48 and all(r['included'] and r['kind'] == 'regular' for r in records)
    assert len({r['original_path'] for r in records}) == len(records)
    assert set(packed) == {'retention-manifest.json'} | {r['archive_member'] for r in records}
    raw = {}
    for record in records:
        data = packed[record['archive_member']]
        check(data, record)
        raw[record['original_path']] = data

    sources, runs, errors = {}, {}, []
    identities_only = {}
    for role in ('new', 'old'):
        base = f'/opt/r1/kernel-{role}'
        result = json.loads(raw[base + '/validation/result.json'])
        assert result['boot_id'] == BOOT
        assert result['source_root'] == base + '/source'
        assert result['target'] == f'/opt/r1/target/kernel-{role}'
        assert result['cgroup']['memory_max'] == str(12 * 1024**3)
        assert result['cgroup']['allowed_cpus'] == [0, 1, 2, 3]
        for key in ('source_manifest', 'source_archive', 'runner'):
            check(raw[result[key]['path']], result[key])
        source_manifest = json.loads(raw[result['source_manifest']['path']])
        archive = raw[result['source_archive']['path']]
        check(archive, {'bytes': source_manifest['archive_bytes'], 'sha256': source_manifest['archive_sha256']})
        source_files, source_dirs = unpack(archive)
        assert set(source_dirs) == set(source_manifest['directory_entries'])
        assert len(source_manifest['files']) == len(source_files)
        assert set(source_files) == {r['path'] for r in source_manifest['files']}
        for ref in source_manifest['files']:
            check(source_files[ref['path']], ref)
        assert source_files[RUNNER_SOURCE] == raw[RUNNER]
        source_refs = {result['source_root'] + '/' + p: data for p, data in source_files.items()}
        sources[role] = {'manifest': result['source_manifest'], 'archive': result['source_archive'],
                         'files': len(source_files), 'directories': len(source_dirs),
                         'base_commit': source_manifest['base_commit'], 'dirty': source_manifest['dirty'],
                         'changed_paths': source_manifest['changed_paths_against_base']}
        labels = NEW_LABELS if role == 'new' else ['toolchain', 'release-example']
        assert [s['label'] for s in result['stages']] == labels
        assert result['status'] == ('completed' if role == 'new' else 'running')
        assert result['mode'] == ('full-validation' if role == 'new' else 'release-build-only')
        stages = []
        for stage in result['stages']:
            label = stage['label']
            path = base + '/validation/stages/' + label + '/supervisor.json'
            record = json.loads(raw[path])
            finished = role == 'new' or label == 'toolchain'
            assert record['argv'] == stage['argv'] == expected_argv(label, result)
            resolved = list(record['argv'])
            if label == 'docs':
                resolved[0] = '/usr/bin/python3.12'
            assert record['resolved_argv'] == resolved
            assert record['cwd'] == result['source_root'] and record['shell'] is False
            assert record['limits']['timeout_seconds'] == stage['timeout_seconds']
            if finished:
                check(raw[path], stage['record'])
                assert stage['status'] == 'passed' and stage['supervisor_exit'] == 0
                assert record['state'] == record['stop_reason'] == 'completed'
                assert record['supervisor_exit_code'] == record['child_exit_code'] == 0
                assert record['cleanup_complete'] and not record['forced'] and not record['errors']
                assert record['identity_unchanged'] and record['identity_before'] == record['identity_after']
            else:
                assert stage['status'] == record['state'] == 'running'
                assert record['child_exit_code'] is None and record['cleanup_complete'] is None
                assert record['identity_after'] == [] and 'binary' not in result
            required_identities = {
                record['resolved_argv'][0], RUNNER,
                result['source_manifest']['path'], result['source_archive']['path'],
                result['source_root'] + '/tools/run_supervised.py',
                result['tools']['rustc']['path'],
            }
            assert required_identities <= {ref['path'] for ref in record['identity_before']}
            for ref in record['identity_before']:
                data = raw.get(ref['path'], source_refs.get(ref['path']))
                if data is not None:
                    check(data, ref)
                else:
                    assert ref['path'].startswith('/opt/r1/rustup/toolchains/') or ref['path'] == '/usr/bin/python3.12'
                    identities_only[ref['path']] = ref
                    tool = next((t for t in result['tools'].values() if t['path'] == ref['path']), None)
                    if tool:
                        assert ref == tool
            output_checks = {}
            for kind, ref in record['outputs'].items():
                actual = identity(raw[ref['path']])
                if 'sha256' in ref:
                    expected = {k: ref[k] for k in ('bytes', 'sha256')}
                    matched = actual == expected
                    if not matched:
                        errors.append({'role': role, 'stage': label, 'output': kind,
                                       'path': ref['path'], 'expected': expected, 'collected': actual})
                    output_checks[kind] = 'matched' if matched else 'MISMATCH'
                else:
                    assert not finished
                    output_checks[kind] = 'no_final_process_pin'
            stdout = raw[record['outputs']['stdout']['path']].decode('utf-8')
            parsed = counts(stdout) if label in ('workspace-tests', 'release-oracle', 'release-river-resolve') else None
            if parsed and label == 'workspace-tests':
                names = parsed.pop('test_names')
                parsed['unique_passed_names'] = len(names)
                parsed['kernel_test_names'] = [n for n in names if n.startswith('kernel::tests::')]
                assert len(parsed['kernel_test_names']) == 6
            stages.append({'label': label, 'argv': record['argv'], 'record': dict(path=path, **identity(raw[path])),
                           'recorded_state': record['state'], 'child_exit': record['child_exit_code'],
                           'recorded_elapsed_seconds': record.get('elapsed_seconds'),
                           'outputs': output_checks, 'raw_test_counts': parsed})
        binary = result.get('binary')
        if binary:
            check(raw[binary['path']], binary)
            assert binary['path'] == result['target'] + '/release/examples/hu_scaling_bench'
        runs[role] = {'recorded_status': result['status'], 'mode': result['mode'], 'stages': stages, 'binary': binary}

    assert len(errors) == 3, 'historical output mismatch inventory changed'
    return {'schema': 'r1.showdown-kernel-firstboot-audit/v1',
            'outcome': 'container_verified_execution_evidence_incomplete',
            'complete_validation_accepted': False, 'boot_id': BOOT,
            'bundle': identity(bundle), 'external_manifest': identity(external),
            'collector': {'included_files_verified': 48, 'skipped_files': 0,
                          'internal_manifest_bytes_equal_external': True, 'archive_exact_file_set': True},
            'sources': sources, 'runs': runs, 'process_output_pin_mismatches': errors,
            'identity_only_tools': list(identities_only.values()),
            'runner_reconstruction': {'path': RUNNER, **identity(raw[RUNNER]),
                'matches_both_verified_source_archives': RUNNER_SOURCE,
                'provenance': 'operator reports /tmp lost on reboot; restored from frozen source before collection'},
            'limitations': ['No retained source code or binary was executed by this verifier.',
                'Collector inventory equality does not establish process-output pin equality.',
                'Empty output cause is unknown; mismatches were observed after Spot interruption and recovery.',
                'Source archives are verified; source-after filesystem equality is a runner assertion, not an independent snapshot.',
                'Old build has no terminal result or retained binary. New release-river-resolve raw test count is unavailable.',
                'First-boot timings must not be combined with later-boot performance measurements.']}


if __name__ == '__main__':
    report = verify()
    print(json.dumps(report, indent=2, ensure_ascii=False, allow_nan=False))
    sys.exit(2 if report['process_output_pin_mismatches'] else 0)
