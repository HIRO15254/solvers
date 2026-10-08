import csv
import hashlib
import json
from pathlib import Path
import re
import statistics

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'runs/t24'
for label in ['scalar-v-base', 'own-variant', 'cached-final', 'seat-own', 'final']:
    path = OUT / f'{label}.json'
    if not path.exists():
        continue
    records = json.loads(path.read_text())['records']
    variants = list(dict.fromkeys(r['variant'] for r in records))
    if len(variants) < 2:
        continue
    print(label)
    rows = []
    for name in sorted({r['bench'] for r in records}):
        first, second = [statistics.median(r['median_ns'] for r in records if r['bench'] == name and r['variant'] == v) for v in variants]
        row = dict(bench=name, first_ns=first, second_ns=second, ratio=second / first)
        rows.append(row)
        print(f'{name}: {first:.3f} -> {second:.3f} ns, ratio {second / first:.5f}')
    with (OUT / f'{label}.csv').open('w', newline='') as stream:
        writer = csv.DictWriter(stream, fieldnames=['bench', 'first_ns', 'second_ns', 'ratio'])
        writer.writeheader()
        writer.writerows(rows)

tests = OUT / 'tests.log'
if tests.exists():
    results = re.findall(r'test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored;', tests.read_text(encoding='utf-8'))
    print('workspace totals:', dict(zip(['passed', 'failed', 'ignored'], [sum(int(r[i]) for r in results) for i in range(3)])))

base, new = OUT / 'f64-base', OUT / 'f64-new'
if base.exists() and new.exists():
    comparisons = []
    for original in sorted(base.iterdir()):
        revised = new / original.name
        assert revised.exists(), revised
        assert original.read_bytes() == revised.read_bytes(), original.name
        comparisons.append(dict(name=original.name, bytes=original.stat().st_size,
                                sha256=hashlib.sha256(original.read_bytes()).hexdigest()))
    (OUT / 'f64-identity.json').write_text(json.dumps(comparisons, indent=2))
    print(f'f64 identity: {len(comparisons)} byte-identical evidence files')

hashes = {}
for log in OUT.glob('final-*-*.log'):
    found = re.findall(r'(t(?:18 realistic (?:fold|showdown)|20 realistic siblings|21 realistic opponents)): .*reach hash ([0-9a-f]+)', log.read_text(encoding='utf-8'))
    for label, value in found:
        hashes.setdefault(label, set()).add(value)
for label, values in hashes.items():
    assert len(values) == 1, (label, values)
    print(label, 'identical workload hash', next(iter(values)))
