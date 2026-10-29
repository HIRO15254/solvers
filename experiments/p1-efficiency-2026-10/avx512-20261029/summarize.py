#!/usr/bin/env python3
"""Summarize collected reports, retaining every raw measurement."""
import json
import statistics
import struct
import sys
from collections import defaultdict
from pathlib import Path

groups = defaultdict(list)
for row in json.loads(Path(sys.argv[1]).read_text(encoding='utf-8'))['runs']:
    groups[(row['case'], row['storage'], row['threads'], row['variant'])].append(row['report'])
for case, storage, threads in sorted({key[:3] for key in groups}):
    fields = []
    bits = set()
    for variant in ('x86-64-v3', 'x86-64-v4', 'native'):
        reports = groups[(case, storage, threads, variant)]
        if len(reports) != 3:
            raise ValueError(f'incomplete: {case} {storage} {threads} {variant}: {len(reports)}')
        it = statistics.median(r['secsPerIter'] for r in reports)
        ev = statistics.median(statistics.mean(r['evaluateSecs']) for r in reports)
        fields.append(f'{it:.5f} / {ev:.5f}')
        bits.update(struct.pack('!d', r['nashConv']) for r in reports)
    print(f'| {case} | {storage} | {threads} | ' + ' | '.join(fields) + f' | {"一致" if len(bits) == 1 else "不一致"} |')
