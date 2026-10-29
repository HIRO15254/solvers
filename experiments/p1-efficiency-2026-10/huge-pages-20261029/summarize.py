#!/usr/bin/env python3
"""Summarize retained p1_bench, time and smaps samples (no rounding in JSON)."""
import json
import pathlib
import statistics
import sys

raw = pathlib.Path(sys.argv[1])
expected_rounds = int(sys.argv[3]) if len(sys.argv) > 3 else 3
groups = {}
for path in sorted(raw.glob('*.json')):
    name = path.stem
    config, storage, threads, mode, round_no = name.split('-')
    report = json.loads(path.read_text(encoding='utf-8'))
    time = dict(line.split('=', 1) for line in (raw / (name + '.time')).read_text().splitlines())
    smaps = (raw / (name + '.smaps')).read_text().splitlines()
    huge = [int(line.split()[1]) for line in smaps if line.startswith('AnonHugePages:')]
    if not huge:
        raise ValueError(f'{name}: no AnonHugePages samples')
    row = {'round': int(round_no[1:]), 'secsPerIter': report['secsPerIter'],
           'evaluateSecs': statistics.mean(report['evaluateSecs']), 'peakBytes': report['peakBytes'],
           'anonHugePeakKiB': max(huge), 'minorFaults': int(time['minorFaults']),
           'majorFaults': int(time['majorFaults']), 'systemSecs': float(time['systemSecs'])}
    groups.setdefault((config, storage, threads, mode), []).append(row)
summary = []
for (config, storage, threads, mode), rows in sorted(groups.items()):
    if len(rows) != expected_rounds:
        raise ValueError(f'{config} {storage} {threads} {mode}: {len(rows)} rounds')
    medians = {key: statistics.median(row[key] for row in rows) for key in rows[0] if key != 'round'}
    summary.append({'config': config, 'storage': storage, 'threads': int(threads[1:]),
                    'mode': mode, 'median': medians, 'rounds': rows})
with open(sys.argv[2], 'w', encoding='utf-8', newline='\n') as f:
    json.dump(summary, f, indent=2, ensure_ascii=False)
    f.write('\n')
