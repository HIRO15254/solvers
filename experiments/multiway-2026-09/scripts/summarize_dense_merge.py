"""Validate fixed old/new dense merge learning measurements and exported equality."""
from __future__ import annotations

import argparse
import copy
from datetime import datetime
import hashlib
import json
from pathlib import Path
import re
import statistics
import zipfile

ORDER = ['legacy-1', 'transactional-1', 'transactional-2', 'legacy-2', 'legacy-3', 'transactional-3']
COMMANDS = {
    'cargo fmt --all --check',
    'cargo clippy --workspace --all-targets -- -D warnings',
    'cargo clippy -p cli --examples --features research-draw-abstraction -- -D warnings',
    'cargo test --workspace',
    'cargo test -p cli --examples --features research-draw-abstraction',
    'cargo test -p multiway --features research-average-sampling --lib',
    'cargo build --release -p cli --example mw_checkpoint_audit',
}
FOLD = 'fold/fold/fold/fold'
OPEN = FOLD + '/raise-to:3000'
THREE = OPEN + '/raise-to:10000'
FOUR = THREE + '/raise-to:21000'
FIVE = FOUR + '/raise-to:100000:all-in'
SUPPORTS = ['root', FOLD, OPEN, THREE, FOUR, FIVE]


def require(condition, label):
    if not condition:
        raise ValueError(label)


def read(path):
    return json.loads(Path(path).read_text(encoding='utf-8-sig'))


def serialized(value):
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False, allow_nan=False) + '\n').encode()


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def checked(path, expected):
    require(re.fullmatch('[0-9a-f]{64}', expected) is not None and sha(path) == expected, 'file identity: ' + str(path))


def same_path(left, right):
    require(Path(left).resolve() == Path(right).resolve(), 'path identity')


def positive(value):
    require(type(value) in (int, float) and 0 < value < float('inf'), 'positive finite measurement')
    return value


def normalized(raw):
    """Only four documented timer locations may differ; preserve everything else."""
    result = copy.deepcopy(raw)
    result.pop('constructionElapsedSecs')
    result['freshTraining'].pop('solveElapsedSecs')
    result['deviatorTraining'].pop('elapsedSecs')
    for item in result['evaluations']:
        item.pop('elapsedSecs')
    return result


def arguments(exp):
    args = ['--config', str(Path(exp['config']).resolve()), '--fresh-sweeps', '8192',
            '--threads', '8', '--memory', '8GiB', '--cache-dir', str(Path(exp['cacheDir']).resolve()),
            '--samples', '128', '--evaluation-seeds', '101,202', '--br-traversals', '1',
            '--node', 'root', '--node-frequency-samples', '0']
    for path in SUPPORTS:
        args.extend(['--support-node', path])
    return args


def source_checks(implementation, base_revision):
    for field in ('binary', 'sourceManifest', 'sourceZip', 'verification'):
        checked(implementation[field], implementation[field + 'Sha256'])
    manifest = read(implementation['sourceManifest'])
    require(manifest['baseRevision'] == base_revision, 'source base revision')
    files = {entry['path']: entry['sha256'] for entry in manifest['files']}
    require(len(files) == len(manifest['files']) and len(files) >= 174, 'complete source manifest')
    with zipfile.ZipFile(implementation['sourceZip']) as archive:
        require(len(archive.namelist()) == len(files) and set(archive.namelist()) == set(files), 'source archive entries')
        for path, digest in files.items():
            require(hashlib.sha256(archive.read(path)).hexdigest() == digest, 'archived source identity')
    verification = read(implementation['verification'])
    require(verification['status'] == 'passed' and verification['sourceFiles'] == len(files)
            and verification['sourceManifestSha256'] == implementation['sourceManifestSha256']
            and verification['binarySha256'] == implementation['binarySha256'], 'verified source and binary')
    require(len(verification['checks']) == 7
            and {c['command'] for c in verification['checks']} == COMMANDS, 'seven required checks')
    for check in verification['checks']:
        require(check['exitCode'] == 0, 'verification command failed')
        checked(check['log'], check['sha256'])
        text = Path(check['log']).read_text(encoding='utf-8-sig')
        require('test result: FAILED' not in text and not re.search(r'^error(?:\[|:)', text, re.M), 'verification error log')
        if check['command'].startswith('cargo test '):
            rows = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', text)
            totals = [sum(int(row[i]) for row in rows) for i in range(3)]
            require(rows and totals[0] > 0 and totals[1] == 0
                    and totals == [check['passed'], check['failed'], check['ignored']]
                    and len(rows) == check['suites'], 'verification test totals')
    return files


def regression_checks(red, green, focused_passed):
    decisive = 'dense_merge_late_slot_overflow_preserves_entire_state'
    require(decisive in red and 'test result: FAILED. 0 passed; 1 failed;' in red,
            'decisive pre-fix regression failure')
    require(type(focused_passed) is int and focused_passed == 7
            and 'test result: FAILED' not in green
            and decisive + ' ... ok' in green
            and 'test result: ok. 7 passed; 0 failed;' in green,
            'focused post-fix regressions')


def training_checks(raw):
    fresh = raw['freshTraining']
    config = fresh['solverConfig']
    require(raw['schemaVersion'] == 'solvers.multiway-checkpoint-audit/v1'
            and raw['sweeps'] == 8192 and fresh['requestedSweeps'] == 8192
            and fresh['metrics']['sweeps'] == 8192 and raw['solverStateVersion'] == 4,
            'fresh successful training')
    require(config['seed'] == 0 and config['sweep_batch'] == 4
            and config['traverser_vector'] is True, 'fixed training seed and algorithm')


def preflight(run, exp):
    require([case['name'] for case in exp['cases']] == ORDER, 'fixed six-case order')
    require(exp['sweeps'] == 8192 and exp['threads'] == 8 and exp['memory'] == '8GiB'
            and exp['timeoutSeconds'] == 600 and exp['supportNodes'] == SUPPORTS, 'fixed cost screen')
    require(exp['cloudResourcesStarted'] is False and exp['broaderGoalComplete'] is False, 'scope')
    for field in ('config', 'runner', 'plan'):
        checked(exp[field], exp[field + 'Sha256'])
    regression = exp['regressionEvidence']
    for field in ('preFixLog', 'postFixLog'):
        checked(regression[field], regression[field + 'Sha256'])
    red = Path(regression['preFixLog']).read_text(encoding='utf-8-sig')
    green = Path(regression['postFixLog']).read_text(encoding='utf-8-sig')
    regression_checks(red, green, regression['focusedPassed'])
    same_path(exp['runner'], Path(__file__).with_name('run_average_sampling_measurement.ps1'))
    checked(__file__, exp['summarizerSha256'])
    checked(Path(__file__).parent / 'tests/test_summarize_dense_merge.py', exp['testScriptSha256'])
    checked(run / 'experiment-preexecution.json', exp['preexecutionSha256'])
    before = read(run / 'experiment-preexecution.json')
    mutable = {'status', 'completedUtc', 'preexecutionSha256'}
    require({k: v for k, v in exp.items() if k not in mutable}
            == {k: v for k, v in before.items() if k not in mutable}, 'predeclared conditions unchanged')
    require(set(exp['implementations']) == {'legacy', 'transactional'}, 'two implementations')
    files = {name: source_checks(value, exp['baseRevision']) for name, value in exp['implementations'].items()}
    changed = sorted(path for path in files['legacy'] if files['legacy'][path] != files['transactional'].get(path))
    added = sorted(set(files['transactional']) - set(files['legacy']))
    require(changed == exp['changedCompiledFiles'] and added == exp['addedCompiledFiles'], 'declared compiled changes')
    for case in exp['cases']:
        kind = case['name'].rsplit('-', 1)[0]
        require(case['implementation'] == kind, 'case implementation')
        same_path(case['job'], run / (case['name'] + '-job.json'))
        checked(case['job'], case['jobSha256'])
        job = read(case['job'])
        require(job['arguments'] == arguments(exp) and job['timeoutSeconds'] == 600
                and job['configSha256'] == exp['configSha256']
                and job['validationReport'] == exp['validationReport'], 'literal job')
        impl = exp['implementations'][kind]
        require(job['binarySha256'] == impl['binarySha256']
                and job['sourceManifestSha256'] == impl['sourceManifestSha256'], 'job implementation identity')
    return dict(changedCompiledFiles=changed, addedCompiledFiles=added,
                sourceFiles={name: len(value) for name, value in files.items()})


def summarize_case(run, exp, case):
    folder = run / case['name']
    raw, measured = read(folder / 'stdout.json'), read(folder / 'measurement.json')
    impl = exp['implementations'][case['implementation']]
    require(measured['schemaVersion'] == 'solvers.checkpoint-audit-measurement/v1'
            and measured['exitCode'] == 0 and measured['timedOut'] is False, 'successful process')
    same_path(measured['binary'], impl['binary'])
    same_path(measured['job'], case['job'])
    require(measured['arguments'] == arguments(exp) and measured['timeoutSeconds'] == 600
            and measured['sourceRevision'] == exp['baseRevision']
            and measured['validationReport'] == exp['validationReport'], 'measured conditions')
    for field, digest in dict(binary=impl['binarySha256'], sourceManifest=impl['sourceManifestSha256'],
                              config=exp['configSha256'], job=case['jobSha256']).items():
        require(measured[field + 'Sha256'] == digest, 'measured identity: ' + field)
    checked(folder / 'stdout.json', measured['stdoutSha256'])
    training_checks(raw)
    same_path(raw['config'], exp['config'])
    require(raw['evaluationSamplesPerSeed'] == 128 and raw['evaluationSeeds'] == [101, 202]
            and [item['seed'] for item in raw['evaluations']] == [101, 202]
            and raw['deviatorTraining']['traversalsPerSeat'] == 1, 'ordinary schedule')
    require([n['context']['requested'] for n in raw['policySupport']] == SUPPORTS
            and all(n['expectedBuckets'] == 169 and len(n['rows']) == 169 for n in raw['policySupport']), 'six full support nodes')
    require(len(raw['nodes']) == 1 and raw['nodes'][0]['frequency'] is None, 'no frequency sampling')
    wall = positive(measured['wallSeconds'])
    construct = positive(raw['constructionElapsedSecs'])
    train = positive(raw['freshTraining']['solveElapsedSecs'])
    extra = positive(raw['deviatorTraining']['elapsedSecs']) + sum(positive(e['elapsedSecs']) for e in raw['evaluations'])
    require(construct + train + extra <= wall + .01, 'timer containment')
    started = datetime.fromisoformat(measured['startedUtc'].replace('Z', '+00:00'))
    require(started.tzinfo is not None, 'timestamp timezone')
    return dict(case=case['name'], implementation=case['implementation'], trainSeconds=train,
                constructionSeconds=construct, wallSeconds=wall,
                peakWorkingSetBytes=positive(measured['observedPeakWorkingSetBytes']), peakMethod=measured['peakMethod'],
                startedUnixSeconds=started.timestamp(), finishedUnixSeconds=started.timestamp() + wall,
                stdoutSha256=measured['stdoutSha256'], measurementSha256=sha(folder / 'measurement.json'),
                normalizedOutputSha256=hashlib.sha256(serialized(normalized(raw))).hexdigest())


def compare(cases):
    require([c['case'] for c in cases] == ORDER, 'complete comparison order')
    require(len({c['normalizedOutputSha256'] for c in cases}) == 1, 'all non-clock exported output exactly matches')
    require(all(left['finishedUnixSeconds'] <= right['startedUnixSeconds'] + .001
                for left, right in zip(cases, cases[1:])), 'serialized processes')
    medians = {name: {field: statistics.median(c[field] for c in cases if c['implementation'] == name)
                      for field in ('trainSeconds', 'constructionSeconds', 'wallSeconds', 'peakWorkingSetBytes')}
               for name in ('legacy', 'transactional')}
    return dict(medians=medians, transactionalToLegacyRatios={field: medians['transactional'][field] / medians['legacy'][field]
                for field in medians['legacy']}, exportedOutputExactlyMatches=True,
                completeProductionStateEqualityClaim=False, independentLearningSeeds=1, timingRepeatsPerImplementation=3)


def summarize(run, case_name=None):
    run = Path(run)
    exp = read(run / 'experiment.json')
    evidence = preflight(run, exp)
    require(case_name is None or case_name in ORDER, 'unknown case')
    cases = [summarize_case(run, exp, c) for c in exp['cases'] if case_name is None or c['name'] == case_name]
    created = datetime.fromisoformat(exp['createdUtc'].replace('Z', '+00:00'))
    require(created.tzinfo is not None and all(created.timestamp() <= c['startedUnixSeconds'] for c in cases), 'plan precedes measurements')
    return dict(schemaVersion='solvers.dense-merge-summary/v1', status='completed-cost-screen' if case_name is None else 'completed-case',
                experiment=exp, experimentSha256=sha(run / 'experiment.json'), evidence=evidence, cases=cases,
                comparison=compare(cases) if case_name is None else None, broaderGoalComplete=False,
                interpretation='Error-path correctness is established separately by full-state regression tests. This screen compares normal exported learning outputs and costs at fixed sweeps, one training seed and three timing replicates. It does not claim complete production-state or checkpoint byte identity, better strategy quality, whole-call rollback, a process memory cap, or a timing confidence interval.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run', type=Path)
    parser.add_argument('--case', choices=ORDER)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    output = serialized(summarize(args.run, args.case))
    if args.output:
        args.output.write_bytes(output)
    else:
        print(output.decode(), end='')


if __name__ == '__main__':
    main()
