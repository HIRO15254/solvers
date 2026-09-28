"""Replay compact VM21 results and receipts; never read/decompress archive parts."""
import datetime as dt
import hashlib
import json
from pathlib import Path
import statistics

HERE = Path(__file__).resolve().parent


def read(path):
    return json.loads(path.read_bytes())


def pin(path):
    raw = path.read_bytes()
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def main():
    files, sdk_count = {}, 0
    for path in sorted(HERE.glob('*.result.json')):
        item = read(path)
        if 'argv' not in item:
            assert item['schema'] == 'r1.vm21-transfer-result/v1'
            continue
        sdk_count += 1
        assert item['exit_code'] == 0
        for channel in ('stdout', 'stderr'):
            raw = path.with_name(path.name.removesuffix('.result.json') + '.' + channel + '.log')
            assert pin(raw) == item[channel]
            files[raw.relative_to(HERE).as_posix()] = item[channel]
        files[path.name] = pin(path)
    report_path = HERE / 'downloads/proof01/sparse-rank-analysis01.json'
    assert pin(report_path)['sha256'] == '17dd8082bf12d30cfb61156b1893a33a7199de37d860dddb5e9c2d3d158633a7'
    report = read(report_path)
    assert report['status'] == 'completed' and report['payload_integrity'] == 'verified'
    assert report['exact_state_and_quality_across_arms_workers'] and not report['production_adoption']
    statistic_count, failed_variability = 0, []
    for group in report['groups']:
        for name, summary in group['metrics'].items():
            values = summary['samples']
            assert len(values) == 3
            assert summary['minimum'] == min(values) and summary['maximum'] == max(values)
            assert summary['median'] == statistics.median(values)
            expected = max(values) / min(values) if min(values) else None
            assert summary['max_over_min'] == expected
            statistic_count += 1
        for name in ('cfr_wall_seconds', 'quality_7_walks_wall_seconds'):
            if group['metrics'][name]['max_over_min'] > 1.15:
                failed_variability.append([group['case'], group['arm'], group['workers'], name])
    assert statistic_count == 264
    assert failed_variability == [['narrow', 'candidate', 32, 'quality_7_walks_wall_seconds']]
    lookup = {(g['case'], g['arm'], g['workers']): g for g in report['groups']}
    for comparison in report['comparisons']:
        case, workers = comparison['case'], comparison['workers']
        baseline, candidate = (lookup[(case, arm, workers)]['metrics'] for arm in ('baseline', 'candidate'))
        for name, metric, statistic in (
            ('candidate_over_baseline_cfr_median', 'cfr_wall_seconds', 'median'),
            ('candidate_over_baseline_quality_median', 'quality_7_walks_wall_seconds', 'median'),
            ('candidate_over_baseline_rss_maximum', 'root_os_peak_resident_bytes', 'maximum')):
            assert comparison[name] == candidate[metric][statistic] / baseline[metric][statistic]
    for group in report['groups']:
        first = lookup[(group['case'], group['arm'], 16)]
        for name, values in group['same_arm_16worker_relative'].items():
            speedup = first['metrics'][name]['median'] / group['metrics'][name]['median']
            assert values == {'speedup': speedup, 'relative_efficiency': speedup / (group['workers'] / 16)}
    guards = report['predeclared_guards']
    assert sum(guards.values()) == 4 and len(guards) == 5 and report['performance_screen'] == 'rejected'
    for phase, expected in [('build', 5), ('measure', 38)]:
        path = HERE / ('build-status05.stdout.log' if phase == 'build' else 'measure-status04.stdout.log')
        state = read(path)[phase]
        assert state['status'] == 'completed' and state['counts']['completed'] == expected
        assert sum(state['counts'].values()) == expected and state['wrapper_finish']['exit_code'] == 0
        assert 'MainPID=0' in state['service'] and 'ActiveState=inactive' in state['service']
    transport, download = read(HERE / 'split01.stdout.log'), read(HERE / 'download-check.json')
    assert transport['archive_sha256'] == download['sha256'] and transport['archive_bytes'] == download['bytes']
    for item, verified in zip(transport['parts'], download['parts'], strict=True):
        assert Path(item['path']).name == verified['path']
        assert all(item[k] == verified[k] for k in ('bytes', 'sha256'))
    for item in transport['files']:
        if item in transport['parts']:
            continue
        path = HERE / 'downloads/proof01' / Path(item['path']).name
        assert pin(path) == {k: item[k] for k in ('bytes', 'sha256')}
        files[path.relative_to(HERE).as_posix()] = pin(path)
    complete = read(HERE / 'transfer-proof01.completed.json')
    assert complete['status'] == 'all_intended_files_verified'
    assert complete['cumulative_payload_bytes'] == transport['payload_bytes'] == 127124619
    operations = read(HERE / 'cleanup-operations01.stdout.log')
    assert all(r['targetId'] == '715936786015339093' and r['status'] == 'DONE' for r in operations)
    deleted, = [r for r in operations if r['operationType'] == 'delete']
    assert dt.datetime.fromisoformat(deleted['endTime']) < dt.datetime.fromisoformat('2026-09-28T06:20:51+00:00')
    for resource in ('instances', 'disks', 'addresses'):
        assert read(HERE / ('absence-' + resource + '01.stdout.log')) == []
    for name in ('review-results.py', 'download-check.json', 'transfer-proof01.completed.json', 'report.jp.md'):
        files[name] = pin(HERE / name)
    result = {'schema': 'r1.vm21-compact-result-review/v1', 'status': 'passed', 'sdk_receipts': sdk_count,
              'recomputed_statistic_groups': statistic_count, 'failed_variability': failed_variability,
              'performance_screen': 'rejected', 'production_adoption': False, 'files': files,
              'large_part_hash_repeated': False, 'local_native_or_archive_decompression': False,
              'scope': 'Small receipt integrity, statistics, transfer records and cleanup; full state verification was performed by the retained GCP reader.'}
    with (HERE / 'result-review.json').open('x') as stream:
        json.dump(result, stream, indent=2)
        stream.write('\n')
    print(json.dumps({k: result[k] for k in ('status', 'sdk_receipts', 'recomputed_statistic_groups', 'performance_screen')}))


if __name__ == '__main__':
    main()
