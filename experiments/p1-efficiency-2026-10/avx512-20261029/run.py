#!/usr/bin/env python3
"""Run rotated ISA benchmarks on one otherwise idle AVX-512 machine."""
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
CONFIGS = ROOT / 'experiments/p1-efficiency-2026-10/memory-estimate-20261029/configs'
VARIANTS = ['x86-64-v3', 'x86-64-v4', 'native']
CASES = [('flop_srp', 'f32'), ('flop_srp', 'i16'), ('flop_3bp', 'f32'),
         ('flop_3bp', 'i16'), ('c_gtowb', 'i16')]
runs = []
for round_index in range(3):
    order = VARIANTS[round_index:] + VARIANTS[:round_index]
    for threads in (16, 8):
        for case, storage in CASES:
            for variant in order:
                exe = f'/work/avx-{variant}/release/examples/p1_bench'
                command = [exe, str(CONFIGS / f'{case}.toml'), '--threads', str(threads),
                           '--warmup', '2', '--iters', '6' if case == 'c_gtowb' else '10',
                           '--evals', '2', '--storage', storage]
                result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=True)
                report = json.loads(result.stdout.strip().splitlines()[-1])
                runs.append({'round': round_index, 'variant': variant, 'case': case,
                             'threads': threads, 'storage': storage, 'report': report})
                with open('/work/avx-results.json', 'w', encoding='utf-8', newline='\n') as f:
                    json.dump({'runs': runs}, f, indent=1)
                    f.write('\n')
                print(round_index, threads, case, storage, variant, report['secsPerIter'],
                      report['evaluateSecs'], report['nashConv'], flush=True)
