#!/usr/bin/env python3
"""Run fixed-cadence curves on the lab machine; outputs stay under OUT."""
import pathlib
import subprocess
import sys
import time

binary, source, out = map(pathlib.Path, sys.argv[1:4])
out.mkdir(parents=True, exist_ok=True)
for tree in ('river_srp', 'turn_srp', 'flop_4bp', 'flop_3bp', 'flop_srp', 'c_gtowb'):
    original = (source / (tree + '.toml')).read_text(encoding='utf-8')
    # Drop entire existing solver/run tables, preserving the spot and tree.
    lines = []
    skip = False
    for line in original.splitlines():
        if line.startswith('['):
            skip = line.startswith('[solver') or line.startswith('[run]')
        if not skip:
            lines.append(line)
    for storage in ('f32', 'i16-f32avg', 'i16', 'i16-noreset'):
        if storage == 'i16-noreset' and tree not in ('river_srp', 'turn_srp', 'flop_srp'):
            continue
        label = tree + '-' + storage
        actual = 'i16' if storage == 'i16-noreset' else storage
        reset = '\n[solver.algorithm]\npow4_reset = false\n' if storage == 'i16-noreset' else ''
        seconds = 360 if tree.startswith('flop') or tree == 'c_gtowb' else 180
        config = '\n'.join(lines) + f'\n[solver]\nstorage = "{actual}"\n{reset}\n[solver.stop]\nmax_iterations = 10000\ncheck_every = 25\n\n[run]\nmax_time = "{seconds}s"\nfinal_checkpoint = false\ncheckpoint_interval = "10h"\n'
        cfg = out / (label + '.toml')
        cfg.write_text(config, encoding='utf-8', newline='\n')
        start = time.time()
        with (out / (label + '.log')).open('w', encoding='utf-8', newline='\n') as log:
            result = subprocess.run([str(binary), 'solve', str(cfg), '--out', str(out / label), '--threads', '16'], stdout=log, stderr=subprocess.STDOUT)
        print(label, result.returncode, round(time.time() - start, 2), flush=True)
