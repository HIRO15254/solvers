"""Retain and align the 200/500-iteration i16 convergence probes."""
import csv
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def read_progress(path):
    return [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines()]


if __name__ == '__main__':
    output = []
    for budget in [200, 500]:
        for label in ['old', 'new']:
            source = ROOT / 'runs/p1-t1' / f'i16-{label}-{budget}'
            for filename in ['progress.jsonl', 'run.json']:
                (HERE / f'i16-{label}-{budget}-{filename}').write_bytes((source / filename).read_bytes())
        old, new = [read_progress(HERE / f'i16-{label}-{budget}-progress.jsonl') for label in ['old', 'new']]
        assert [x['iteration'] for x in old] == [x['iteration'] for x in new]
        for a, b in zip(old, new):
            output.append({'budget': budget, 'iteration': a['iteration'],
                           'oldNashConv': a['nash_conv'], 'newNashConv': b['nash_conv'],
                           'newOverOld': b['nash_conv'] / a['nash_conv']})
    with (HERE / 'i16-progress.csv').open('w', newline='', encoding='utf-8') as f:
        writer = csv.DictWriter(f, fieldnames=list(output[0]))
        writer.writeheader()
        writer.writerows(output)
    print(json.dumps(output, indent=2))
