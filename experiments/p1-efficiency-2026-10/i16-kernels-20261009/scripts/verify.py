"""Validate retained inputs, results, commands and acceptance thresholds."""
import hashlib
import json
from pathlib import Path
import statistics
import tomllib

EXP = Path(__file__).resolve().parents[1]
result = json.loads((EXP / 'result.json').read_text(encoding='utf-8'))
assert len(result['speed']) == 6
for row in result['speed']:
    medians = {}
    for version in ['old', 'new']:
        files = sorted((EXP / 'raw').glob(f'bench_{row["case"]}_{row["storage"]}_*_{version}.json'))
        samples = []
        for path in files:
            data = json.loads(path.read_text(encoding='utf-8'))
            assert data['threads'] == 8 and data['warmupIters'] == 5
            assert data['iters'] == (15 if row['case'] == 'c_flop1' else 30)
            assert data['evalSecs'] == [] and data['storage'] == row['storage']
            command = json.loads(path.with_suffix('.command.json').read_text(encoding='utf-8'))
            assert command['exitCode'] == 0
            samples.append(data['secsPerIter'])
        assert len(samples) >= 3
        medians[version] = statistics.median(samples)
        assert row[version]['median'] == medians[version]
    assert row['ratio'] == medians['new'] / medians['old']
    if row['storage'] != 'f32':
        assert row['ratio'] < 1, row
    else:
        assert abs(row['ratio'] - 1) <= 0.05, row
    # Alternation is required even when another process adds substantial noise.
    old = sorted((EXP / 'raw').glob(f'bench_{row["case"]}_{row["storage"]}_*_old.command.json'))
    previous = 0.0
    for path in old:
        before = json.loads(path.read_text(encoding='utf-8'))
        after = json.loads(Path(str(path).replace('_old.command', '_new.command')).read_text(encoding='utf-8'))
        assert previous < before['startUnix'] < after['startUnix']
        previous = after['startUnix']
assert len(result['convergence']) == 3
for row in result['convergence']:
    config = tomllib.loads((EXP / 'configs' / f'{row["case"]}_{row["storage"]}.toml').read_text(encoding='utf-8'))
    assert config['solver']['storage'] == row['storage']
    assert config['solver']['stop']['target'] == '0.1%pot'
    assert config['solver']['stop']['check_every'] == 10
    assert config['run']['final_checkpoint'] is False
    for version in ['old', 'new']:
        assert row[version] is not None, row
        stem = f'solve_{row["case"]}_{row["storage"]}_{version}'
        progress = [json.loads(line) for line in (EXP / 'raw' / f'{stem}.progress.jsonl').read_text(encoding='utf-8').splitlines()]
        first = next(p for p in progress if p['nash_conv'] / 2 / row['potBB'] <= 0.001)
        assert first == row[version]
        events = (EXP / 'raw' / f'{stem}.events.jsonl').read_text(encoding='utf-8')
        assert 'target-reached' in events
    assert row['ratio'] == row['new']['iteration'] / row['old']['iteration']
    assert row['ratio'] <= 1.03, row
manifest = json.loads((EXP / 'manifest.json').read_text(encoding='utf-8'))
for file, expected in manifest['retainedSha256'].items():
    assert hashlib.sha256((EXP / file).read_bytes()).hexdigest() == expected, file
print('PASS: 6 throughput comparisons, 3 convergence comparisons, alternation, inputs and retained hashes')
