"""Validate new fit budgets explicitly, reusing tested moment/source checks."""
from datetime import datetime, timedelta
import hashlib
import json
import math
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path.cwd() / 'tools'))
import summarize_whole_preflop_deviation as pilot

run = Path(__file__).resolve().parent

def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def read(path):
    return json.loads(Path(path).read_text(encoding='utf-8-sig'))

exp = read(run / 'experiment.json')
assert sha(run / 'experiment-preexecution.json') == exp['preexecutionSha256']
before = read(run / 'experiment-preexecution.json')
mutable = {'status', 'completedUtc', 'preexecutionSha256'}
assert {k: v for k, v in exp.items() if k not in mutable} == {k: v for k, v in before.items() if k not in mutable}
assert exp['diagnostic'] == dict(fitTraversalsPerSeat=131072, fitSeed=2601, heldOutSamples=32768, heldOutSeeds=[2801, 2802])
for field in ('previousExperiment', 'config', 'plan', 'runner', 'helper', 'helperDependency'):
    assert sha(exp[field]) == exp[field + 'Sha256'], field
for name, digest in exp['sidecars'].items():
    assert sha(run / name) == digest, name
assert exp['cloudResourcesStarted'] is False and exp['broaderGoalComplete'] is False
subprocess.run(['python', 'runs/whole-preflop-deviation-20260910/validate_run.py'], check=True)
prior = read(exp['previousExperiment'])
assert exp['implementation'] == prior['implementation']
assert exp['baseRevision'] == prior['baseRevision']
assert [case['name'] for case in exp['cases']] == ['ordinary', 'enumerated']
previous_end = datetime.fromisoformat(exp['createdUtc'])
arms = {}
for case in exp['cases']:
    for field in ('job', 'oldJob', 'oldOutput'):
        assert sha(case[field]) == case[field + 'Sha256'], field
    old_args = read(case['oldJob'])['arguments']
    old_args[old_args.index('--preflop-deviation-fit-traversals') + 1] = '131072'
    old_args[old_args.index('--preflop-deviation-seeds') + 1] = '2801,2802'
    job = read(case['job'])
    assert job['arguments'] == old_args and job['timeoutSeconds'] == 600
    directory = run / case['name']
    measurement = read(directory / 'measurement.json')
    raw = read(directory / 'stdout.json')
    old = read(case['oldOutput'])
    assert measurement['exitCode'] == 0 and measurement['timedOut'] is False
    assert measurement['timeoutSeconds'] == 600 and measurement['arguments'] == job['arguments']
    assert measurement['schemaVersion'] == 'solvers.checkpoint-audit-measurement/v1'
    assert measurement['sourceRevision'] == exp['baseRevision']
    assert measurement['validationReport'] == job['validationReport'] == exp['validationReport']
    assert Path(measurement['binary']).resolve() == Path(exp['implementation']['binary']).resolve()
    assert Path(measurement['job']).resolve() == Path(case['job']).resolve()
    for field, digest in dict(binary=exp['implementation']['binarySha256'],
                              sourceManifest=exp['implementation']['sourceManifestSha256'],
                              config=exp['configSha256'], job=case['jobSha256']).items():
        assert measurement[field + 'Sha256'] == digest
        if field != 'job':
            assert job[field + 'Sha256'] == digest
    assert sha(directory / 'stdout.json') == measurement['stdoutSha256']
    assert 0 < measurement['wallSeconds'] < 600 and measurement['observedPeakWorkingSetBytes'] > 0
    start = datetime.fromisoformat(measurement['startedUtc'])
    assert start >= previous_end
    previous_end = start + timedelta(seconds=measurement['wallSeconds'])
    pilot.previous.raw_checks(raw, candidate=case['name'] == 'enumerated')
    assert pilot.normalized_old(raw) == pilot.normalized_old(old)
    value = raw['preflopDeviation']
    pilot.previous.finite_json(value)
    assert set(value) == set(old['preflopDeviation'])
    assert value['schemaVersion'] == 'solvers.multiway-preflop-deviation/v1'
    assert value['scope'] == 'all-preflop-decisions-with-frozen-postflop'
    assert value['config'] == exp['diagnostic']
    assert value['variant'] == {'purify_threshold': 0.0, 'use_current_strategy': False}
    assert value['minFitVisits'] == 8 and value['maxBufferedSamples'] == 4096
    pilot.previous.fingerprint(value['fitPolicyFingerprint'])
    pilot.previous.number(value['fitElapsedSecs'], 'fit clock', positive=True)
    assert len(value['fitCoverage']) == 6
    for fit in value['fitCoverage']:
        assert set(fit) == {'traversals', 'visited_infosets', 'retained_infosets', 'total_visits', 'retained_visits'}
        for field, count in fit.items():
            pilot.previous.integer(count, field)
        assert fit['traversals'] == 131072
        assert fit['retained_infosets'] <= fit['visited_infosets'] <= fit['total_visits']
        assert fit['retained_infosets'] != 0 or fit['retained_visits'] == 0
        assert 8 * fit['retained_infosets'] <= fit['retained_visits'] <= fit['total_visits']
        missing = fit['visited_infosets'] - fit['retained_infosets']
        assert missing <= fit['total_visits'] - fit['retained_visits'] <= 7 * missing
    assert [held['seed'] for held in value['heldOut']] == [2801, 2802]
    rows = []
    for held in value['heldOut']:
        assert set(held) == set(old['preflopDeviation']['heldOut'][0])
        assert held['samples'] == 32768 and pilot.previous.integer(held['totalDealAttempts'], 'attempts') >= 32768
        pilot.previous.number(held['elapsedSecs'], 'held-out clock', positive=True)
        assert all(len(held[field]) == 6 for field in ('baseline', 'deviating', 'gains', 'coverage', 'candidatePolicyCoverage'))
        for seat, position in enumerate(pilot.POSITIONS):
            baseline, deviating, gain = (held[field][seat] for field in ('baseline', 'deviating', 'gains'))
            for estimate in (baseline, deviating, gain):
                pilot.estimate(estimate)
            assert math.isclose(gain['mean'], deviating['mean'] - baseline['mean'], rel_tol=1e-10, abs_tol=1e-10)
            coverage = held['coverage'][seat]
            pilot.coverage(coverage, candidate=False)
            pilot.coverage(held['candidatePolicyCoverage'][seat], candidate=True)
            decisions = coverage['decision_visits_by_street']['preflop']
            trained = coverage['trained_action_visits_by_street']['preflop']
            rows.append(dict(seed=held['seed'], seat=seat, position=position, baseline=baseline, deviating=deviating, gain=gain,
                             preflopDecisionVisits=decisions, preflopTrainedActionVisits=trained,
                             preflopTrainedVisitFraction=trained / decisions if decisions else None))
    phase_sum = (raw['constructionElapsedSecs'] + raw['freshTraining']['solveElapsedSecs']
                 + raw['preflopSupportCensus']['elapsedSecs'] + raw['deviatorTraining']['elapsedSecs']
                 + sum(item['elapsedSecs'] for item in raw['evaluations'])
                 + value['fitElapsedSecs'] + sum(item['elapsedSecs'] for item in value['heldOut']))
    assert phase_sum <= measurement['wallSeconds'] + 0.05
    arms[case['name']] = dict(diagnostic=value, rows=rows, oldOutputExactlyReplayed=True,
                            normalizedOldOutputSha256=hashlib.sha256(pilot.normalized_old(old)).hexdigest(),
                            constructionElapsedSecs=raw['constructionElapsedSecs'], solveElapsedSecs=raw['freshTraining']['solveElapsedSecs'],
                            measurement=measurement, measurementSha256=sha(directory / 'measurement.json'))
result = dict(status='passed-fit-calibration-validation', schemaVersion='solvers.preflop-fit-calibration-summary/v1',
              provenance=exp, arms=arms, strategicImprovementEstablished=False, cloudResourcesStarted=False)
(run / 'summary.json').write_bytes(pilot.previous.serialized(result))
print(json.dumps(dict(status=result['status'], summarySha256=sha(run / 'summary.json'),
                     gains={name: [row['gain']['mean'] for row in arm['rows']] for name, arm in arms.items()})))
