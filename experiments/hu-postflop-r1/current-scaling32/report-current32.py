"""Compact JSON from a completed proof and its already trusted verification JSON.

This presentation helper does not run the portable checker or authenticate a
verification document. Supply the successful trusted-check output for this proof.
It recomputes timing statistics from retained benchmark reports, checks their
bindings to result/summary, and records every input hash. No retained code runs.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import statistics
import struct

HERE = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise ValueError(message)


def decode(raw):
    def pairs(items):
        value = {}
        for key, item in items:
            require(key not in value, 'duplicate JSON key')
            value[key] = item
        return value
    def number(text):
        value = float(text)
        require(math.isfinite(value), 'nonfinite JSON number')
        return value
    def constant(text):
        raise ValueError('nonfinite JSON constant: ' + text)
    return json.loads(raw, object_pairs_hook=pairs, parse_float=number, parse_constant=constant)


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def content(value):
    return {key: value[key] for key in ('bytes', 'sha256')}


def trajectory(sample):
    return [{'iterations': row['iterations'], **{key: [struct.pack('>d', v).hex() for v in
            (row[key] if isinstance(row[key], list) else [row[key]])]
            for key in ('solver_ev', 'solver_br', 'nash_conv')}} for row in sample['stopping']['checks']]


def generate(proof, verification_path):
    paths = {name: proof / name for name in ('plan.json', 'build.json', 'result.json', 'retention.json')}
    paths.update(verification=verification_path, protocol=HERE / 'protocol.json')
    original = {name: path.read_bytes() for name, path in paths.items()}
    data = {name: decode(raw) for name, raw in original.items()}
    plan, build, result, verification = [data[name] for name in ('plan.json', 'build.json', 'result.json', 'verification')]
    protocol = data['protocol']
    require(verification['schema'] == 'r1.current-scaling32-verification/v1'
            and verification['status'] == 'completed' and verification['payload_integrity'] == 'verified'
            and verification['provenance_complete'] is True, 'completed trusted verification required')
    require((verification['build_stages_passed'], verification['samples_passed'], verification['warmups_passed'])
            == (2, 36, 9), 'verification stage counts differ')
    require(plan['schema'] == 'r1.current-scaling32-plan/v1' and plan['protocol'] == protocol
            and build['schema'] == 'r1.current-scaling32-build/v1' and build['status'] == 'completed'
            and result['schema'] == 'r1.current-scaling32-result/v1' and result['status'] == 'completed', 'proof schema/state differs')
    require(verification['host'] == plan['host'] and verification['summary'] == result['summary'], 'verification does not match proof')
    require(len(build['stages']) == 2 and [x['stage']['label'] for x in build['stages']] == ['toolchain', 'release-example']
            and all(x['status'] == 'passed' for x in build['stages']), 'two successful build stages required')
    entries = result['stages']
    require(len(entries) == 36 and all(x['status'] == 'passed' for x in entries)
            and sum(x['stage']['warmup'] is True for x in entries) == 9, '36 passed samples including 9 warmups required')
    expected = []
    for i, (case, definition) in enumerate(protocol['cases'].items()):
        for block in range(4):
            n = (i + block) % 3
            for workers in protocol['threads'][n:] + protocol['threads'][:n]:
                expected.append({'case': case, 'block': block, 'warmup': block == 0, 'threads': workers,
                                 'iterations': definition['iterations'], 'label': f'{case}-b{block}-t{workers}'})
    require([x['stage'] for x in entries] == plan['schedule'] == expected, 'sample order/counts differ')
    index = data['retention.json']['files']
    reports = {}
    for entry in entries:
        stage, sample = entry['stage'], entry['sample']
        path = plan['output'] + '/stages/' + stage['label'] + '/bench/result.json'
        record = index[path]
        require(record['path'] == path and re.fullmatch('[0-9a-f]{64}', record['sha256']), 'invalid report pin')
        raw = (proof / 'payload' / record['sha256']).read_bytes()
        require(pin(raw) == content(record), 'raw benchmark report hash differs')
        report = decode(raw)
        require(report['schema'] == 'r1.hu-scaling-bench/v1' and report['status'] == 'completed'
                and report['storage'] == 'f32' and report['layout'] == 'compact'
                and report['threads'] == stage['threads'], 'raw benchmark invocation differs')
        require(report['timing'] == sample['timing'] and report['timing']['run_seconds'] == sample['run_seconds'], 'raw timing differs')
        require(type(sample['run_seconds']) in (float, int) and math.isfinite(sample['run_seconds'])
                and sample['run_seconds'] > 0, 'invalid duration')
        for key in ('quality', 'iterations', 'stopping', 'counts', 'algorithm', 'rake', 'utility'):
            require(report[key] == sample[key], 'raw report differs: ' + key)
        require(sample['stopping']['target_met'] is True
                and sample['quality']['nash_conv'] <= protocol['cases'][stage['case']]['target_nash_conv'], 'quality target not met')
        reports[stage['label']] = content(record)
    cases = {}
    for case, definition in protocol['cases'].items():
        rows = [x for x in entries if x['stage']['case'] == case]
        first = rows[0]['sample']
        for entry in rows:
            sample = entry['sample']
            require(trajectory(sample) == trajectory(first), 'stopping trajectory bits differ')
            for key in ('quality', 'iterations', 'counts', 'global_combos', 'union_global_combos', 'algorithm', 'rake', 'utility'):
                require(sample[key] == first[key], 'worker/repeat agreement differs: ' + key)
            for name in ('canonical.bin', 'state.bin'):
                require(content(sample['artifacts'][name]) == content(first['artifacts'][name]), 'canonical/state pins differ')
            require(content(sample['normalized_config']) == content(first['normalized_config']), 'normalized config pins differ')
        workers = {}
        for count in (1, 16, 32):
            samples = [x['sample'] for x in rows if x['stage']['threads'] == count and not x['stage']['warmup']]
            require(len(samples) == 3, 'three measured runs required')
            times = [sample['run_seconds'] for sample in samples]
            workers[str(count)] = {'run_seconds': times, 'median_seconds': statistics.median(times),
                                   'iterations': [sample['iterations'] for sample in samples]}
        for count in (1, 16, 32):
            row = workers[str(count)]
            row['speedup_over_one'] = workers['1']['median_seconds'] / row['median_seconds']
            row['efficiency'] = row['speedup_over_one'] / count
            saved = result['summary']['cases'][case]['workers'][str(count)]
            require(all(saved[key] == value for key, value in row.items()), 'derived worker statistics differ')
        ratio = workers['32']['median_seconds'] / workers['16']['median_seconds']
        slower = workers['32']['median_seconds'] > workers['16']['median_seconds']
        require(result['summary']['cases'][case]['ratio_32_over_16'] == ratio
                and result['summary']['cases'][case]['32_slower_than16'] is slower, '32/16 summary differs')
        if case == 'flop':
            require(first['counts']['root_dims'] == [3, 3], 'expected narrow Flop fixture differs')
        cases[case] = {'workers': workers, 'ratio_32_over_16': ratio, '32_slower_than16': slower,
                       'fastest_observed_workers': min((1, 16, 32), key=lambda n: workers[str(n)]['median_seconds']),
                       'stopping_controls': definition, 'quality': first['quality'], 'trajectory_f64_bits': trajectory(first),
                       'root_dims': first['counts']['root_dims'], 'rake': first['rake'], 'utility': first['utility'],
                       'canonical_and_state': {name: content(value) for name, value in first['artifacts'].items()},
                       'warmups_passed': 3, 'measured_passed': 9, 'worker_repeat_agreement': True}
    require(all(paths[name].read_bytes() == raw for name, raw in original.items()), 'input changed during report')
    return {'schema': 'r1.current-scaling32-report/v1', 'status': 'completed', 'revision': protocol['revision'],
            'build_stages_passed': 2, 'samples_passed': 36, 'warmups_passed': 9, 'measured_samples': 27,
            'host': plan['host'], 'binary': build['binary'], 'cases': cases,
            'input_pins': {name: {'path': str(paths[name]), **pin(raw)} for name, raw in original.items()},
            'benchmark_report_pins': reports,
            'timing_scope': 'CFR plus every stopping EV/BR check through first passing target; setup and post-stop capture excluded',
            'agreement_basis': 'Trusted checker compared original canonical/state/config bytes, final solver EV/BR/NashConv bits and stopping-trajectory f64 bits across all 12 runs per case. Other derived/subgame quality fields use numeric equality. This helper rechecks raw report hashes, statistics, trajectory bits and retained artifact pins; it does not rehash the large canonical payloads.',
            'limitations': ['Caller-supplied verification must be the trusted portable-check output for this proof.',
                            'Three measured runs per worker are descriptive; no confidence interval or general optimal-worker conclusion.',
                            'Same 32-logical-CPU host with workers 1/16/32; this does not compare three separately sized VMs or imply 32 physical cores.',
                            'River has nonzero rake and unconfirmed external-reference assumptions; NashConv is an internal diagnostic, not zero-sum certification.',
                            'Flop has three combos per player, one flop bet size and later streets check down; no general full-range Flop claim.',
                            'Bit agreement concerns these retained inputs and runs, not all games/platforms. Reported BLAKE3 was syntax-checked, not independently recomputed.',
                            'Current-source endpoint comparison only; no old/new, I16, external accuracy or overall R1 certification.'],
            'memory_claim': None, 'speedup_required': False, 'external_certification': False, 'r1_certification': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--proof', type=Path, required=True)
    parser.add_argument('--verification', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True, help='new JSON file outside the proof directory')
    args = parser.parse_args()
    proof, verification = args.proof.resolve(strict=True), args.verification.resolve(strict=True)
    target = args.out.absolute()
    require(not target.exists() and not target.is_symlink() and not target.resolve().is_relative_to(proof)
            and target.resolve() != verification, 'new report outside proof required')
    report = generate(proof, verification)
    with target.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(report, stream, indent=2, allow_nan=False)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    print(json.dumps({'status': report['status'], 'report': str(target)}))


if __name__ == '__main__':
    main()
