#!/usr/bin/env python3
"""Summarize retained curves and benchmark JSON (times on shared lab are not benchmarks)."""
import json
import pathlib
import statistics

root = pathlib.Path(__file__).resolve().parent
pots = {'river_srp': 5.5, 'turn_srp': 5.5, 'flop_4bp': 48.5, 'flop_3bp': 22.5, 'flop_srp': 5.5, 'c_gtowb': 5.5}
thresholds = (0.2, 0.1, 0.05, 0.02, 0.01)
summary = {}
for path in sorted((root / 'results' / 'curves').glob('*/progress.jsonl')):
    label = path.parent.name
    tree = next(t for t in pots if label.startswith(t + '-'))
    points = [json.loads(s) for s in path.read_text(encoding='utf-8').splitlines() if s]
    if not points:
        continue
    # NashConv / 2 / pot * 100 is the solver's stopping metric in % pot.
    values = [p['nash_conv'] * 50 / pots[tree] for p in points]
    targets = {}
    for limit in thresholds:
        found = next(((p['iteration'], p['elapsed_secs']) for p, value in zip(points, values) if value <= limit), None)
        targets[str(limit)] = found
    late = values[len(values) * 4 // 5:]
    summary[label] = {'checks': len(points), 'last_iteration': points[-1]['iteration'], 'last_percent': values[-1], 'minimum_percent': min(values), 'minimum_iteration': points[values.index(min(values))]['iteration'], 'last_fifth_range_percent': [min(late), max(late)], 'targets': targets}
timing = {}
for tree in ('flop_srp', 'flop_3bp', 'c_gtowb'):
    for threads in (16, 32):
        for storage in ('f32', 'i16-f32avg', 'i16'):
            paths = sorted((root / 'results' / 'timing').glob(f'{tree}-{threads}-*-{storage}.json'))
            if not paths:
                continue
            data = [json.loads(p.read_text(encoding='utf-8')) for p in paths]
            timing[f'{tree}-{threads}-{storage}'] = {'samples': len(data), 'secsPerIter': statistics.median(d['secsPerIter'] for d in data), 'evaluateSecs': statistics.median(statistics.median(d['evaluateSecs']) for d in data), 'peakBytes': int(statistics.median(d['peakBytes'] for d in data))}
comparisons = {}
for path in sorted((root / 'results' / 'storage-floor-compare').glob('*-compare.json')):
    label = path.name.removesuffix('-compare.json')
    tree = next(t for t in pots if label.startswith(t + '-'))
    data = json.loads(path.read_text(encoding='utf-8'))
    comparisons[label] = {'percent': [value * 50 / pots[tree] for value in data['nash_conv']], 'mean_strategy_l1': data['mean_strategy_l1'], 'max_ev_delta_bb': data['max_ev_delta'], 'root_ev_delta_bb': [data[key][1] - data[key][0] for key in ('ev_oop', 'ev_ip')]}
with (root / 'results' / 'summary.json').open('w', encoding='utf-8', newline='\n') as f:
    json.dump({'metric': 'NashConv / 2 / starting pot * 100', 'curves': summary, 'timing': timing, 'comparisons': comparisons}, f, ensure_ascii=False, indent=2)
    f.write('\n')
