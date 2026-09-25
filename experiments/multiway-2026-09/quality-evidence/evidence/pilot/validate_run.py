"""Bind pilot outputs to frozen compiled inputs, budgets, and retained references."""
from datetime import datetime, timedelta
import hashlib
import json
from pathlib import Path
import re
import subprocess
import zipfile

run = Path(__file__).resolve().parent

def read(path):
    return json.loads(Path(path).read_text(encoding='utf-8-sig'))

def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def check(path, digest):
    assert re.fullmatch('[0-9a-f]{64}', digest) and sha(path) == digest, str(path)

exp = read(run / 'experiment.json')
check(run / 'experiment-preexecution.json', exp['preexecutionSha256'])
before = read(run / 'experiment-preexecution.json')
mutable = {'status', 'completedUtc', 'preexecutionSha256'}
assert {k: v for k, v in exp.items() if k not in mutable} == {k: v for k, v in before.items() if k not in mutable}
assert [case['name'] for case in exp['cases']] == ['ordinary', 'enumerated']
assert (exp['sweeps'], exp['threads'], exp['memory'], exp['timeoutSeconds']) == (8192, 8, '8GiB', 600)
assert exp['diagnostic'] == dict(fitTraversalsPerSeat=8192, fitSeed=2601, heldOutSamples=32768, heldOutSeeds=[2701, 2702])
assert exp['cloudResourcesStarted'] is False and exp['broaderGoalComplete'] is False
for field in ('config', 'plan', 'runner', 'summarizer', 'summarizerDependency', 'summarizerTests', 'previousExperiment'):
    check(exp[field], exp[field + 'Sha256'])
for name, digest in exp['sidecars'].items():
    check(run / name, digest)
impl = exp['implementation']
for field in ('binary', 'sourceManifest', 'sourceZip', 'verification'):
    check(impl[field], impl[field + 'Sha256'])
manifest = read(impl['sourceManifest'])
assert manifest['baseRevision'] == exp['baseRevision']
files = {entry['path']: entry['sha256'] for entry in manifest['files']}
assert len(files) == len(manifest['files']) == 180
with zipfile.ZipFile(impl['sourceZip']) as archive:
    assert len(archive.namelist()) == len(files) and set(archive.namelist()) == set(files)
    for path, digest in files.items():
        assert hashlib.sha256(archive.read(path)).hexdigest() == digest
verification = read(impl['verification'])
assert verification['status'] == 'passed' and verification['sourceFiles'] == len(files)
for field in ('sourceManifest', 'sourceZip', 'binary'):
    assert verification[field + 'Sha256'] == impl[field + 'Sha256']
expected = {
    'cargo fmt --all --check', 'cargo clippy --workspace --all-targets -- -D warnings',
    'cargo clippy -p cli --examples --features research-draw-abstraction,research-regret-sampling -- -D warnings',
    'cargo test --workspace',
    'cargo test -p cli --examples --features research-draw-abstraction,research-regret-sampling',
    'cargo test -p multiway --features research-average-sampling,research-regret-sampling --lib',
    'cargo build --release -p cli --example mw_checkpoint_audit --features research-regret-sampling',
}
assert len(verification['checks']) == 7 and {item['command'] for item in verification['checks']} == expected
for item in verification['checks']:
    assert item['exitCode'] == 0
    check(item['log'], item['sha256'])
    log = Path(item['log']).read_text(encoding='utf-8-sig')
    assert 'test result: FAILED' not in log and not re.search(r'^error(?:\[|:)', log, re.M)
    if item['command'].startswith('cargo test '):
        rows = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', log)
        assert rows and len(rows) == item['suites']
        assert [sum(int(row[i]) for row in rows) for i in range(3)] == [item['passed'], 0, item['ignored']]
previous_end = datetime.fromisoformat(exp['createdUtc'])
measurements = []
extras = ['--preflop-deviation-fit-traversals', '8192', '--preflop-deviation-fit-seed', '2601',
          '--preflop-deviation-samples', '32768', '--preflop-deviation-seeds', '2701,2702']
for case in exp['cases']:
    for field in ('job', 'oldJob', 'oldOutput', 'oldMeasurement'):
        check(case[field], case[field + 'Sha256'])
    old_measurement = read(case['oldMeasurement'])
    assert old_measurement['exitCode'] == 0 and old_measurement['timedOut'] is False
    assert old_measurement['stdoutSha256'] == case['oldOutputSha256']
    job = read(case['job'])
    args = read(case['oldJob'])['arguments'] + extras
    assert job['arguments'] == args and job['timeoutSeconds'] == 600
    assert job['validationReport'] == exp['validationReport']
    directory = run / case['name']
    measured = read(directory / 'measurement.json')
    raw = read(directory / 'stdout.json')
    assert measured['schemaVersion'] == 'solvers.checkpoint-audit-measurement/v1'
    assert measured['exitCode'] == 0 and measured['timedOut'] is False and measured['timeoutSeconds'] == 600
    assert measured['arguments'] == args and measured['sourceRevision'] == exp['baseRevision']
    assert Path(measured['binary']).resolve() == Path(impl['binary']).resolve()
    assert Path(measured['job']).resolve() == Path(case['job']).resolve()
    assert measured['validationReport'] == exp['validationReport']
    for field, digest in dict(binary=impl['binarySha256'], sourceManifest=impl['sourceManifestSha256'],
                              config=exp['configSha256'], job=case['jobSha256']).items():
        assert measured[field + 'Sha256'] == digest
        if field != 'job':
            assert job[field + 'Sha256'] == digest
    check(directory / 'stdout.json', measured['stdoutSha256'])
    assert 0 < measured['wallSeconds'] < 600 and measured['observedPeakWorkingSetBytes'] > 0
    start = datetime.fromisoformat(measured['startedUtc'])
    assert start >= previous_end
    previous_end = start + timedelta(seconds=measured['wallSeconds'])
    new = raw['preflopDeviation']
    phase_sum = (raw['constructionElapsedSecs'] + raw['freshTraining']['solveElapsedSecs']
                 + raw['preflopSupportCensus']['elapsedSecs'] + raw['deviatorTraining']['elapsedSecs']
                 + sum(item['elapsedSecs'] for item in raw['evaluations'])
                 + new['fitElapsedSecs'] + sum(item['elapsedSecs'] for item in new['heldOut']))
    assert 0 < phase_sum <= measured['wallSeconds'] + 0.05
    measurements.append(dict(name=case['name'], wallSeconds=measured['wallSeconds'],
                             constructionSeconds=raw['constructionElapsedSecs'],
                             solveSeconds=raw['freshTraining']['solveElapsedSecs'],
                             fitSeconds=new['fitElapsedSecs'],
                             heldOutSeconds=[item['elapsedSecs'] for item in new['heldOut']],
                             peakWorkingSetBytes=measured['observedPeakWorkingSetBytes'],
                             stdoutSha256=measured['stdoutSha256'], measurementSha256=sha(directory / 'measurement.json')))
subprocess.run(['python', exp['summarizer'], '--ordinary', str(run / 'ordinary/stdout.json'),
                '--enumerated', str(run / 'enumerated/stdout.json'),
                '--old-ordinary', exp['cases'][0]['oldOutput'], '--old-enumerated', exp['cases'][1]['oldOutput'],
                '--out', str(run / 'summary.json')], check=True)
result = dict(status='passed-pilot-validation', sourceFiles=len(files), verification=verification,
              measurements=measurements, strategicImprovementEstablished=False,
              cloudResourcesStarted=False, preexecutionSha256=exp['preexecutionSha256'],
              summarySha256=sha(run / 'summary.json'))
(run / 'validation-checks.json').write_text(json.dumps(result, indent=2, sort_keys=True, allow_nan=False) + '\n', encoding='utf-8', newline='\n')
print(json.dumps(dict(status=result['status'], summarySha256=result['summarySha256'])))
