#!/usr/bin/env python3
"""Solve to a common 0.1% pot target, then compare each quantized solution to f32."""
import pathlib
import subprocess
import sys

binary, source, out = map(pathlib.Path, sys.argv[1:4])
out.mkdir(parents=True, exist_ok=True)
for tree in ('river_srp', 'turn_srp', 'flop_4bp', 'flop_3bp', 'flop_srp'):
    original = (source / (tree + '.toml')).read_text(encoding='utf-8')
    lines, skip = [], False
    for line in original.splitlines():
        if line.startswith('['):
            skip = line.startswith('[solver') or line.startswith('[run]')
        if not skip:
            lines.append(line)
    for storage in ('f32', 'i16-f32avg', 'i16'):
        label = tree + '-' + storage
        config = '\n'.join(lines) + f'\n[solver]\nstorage = "{storage}"\n\n[solver.stop]\ntarget = "0.1%pot"\nmax_iterations = 10000\ncheck_every = 25\n\n[run]\nmax_time = "300s"\nfinal_checkpoint = false\ncheckpoint_interval = "10h"\n'
        cfg = out / (label + '.toml')
        cfg.write_text(config, encoding='utf-8', newline='\n')
        with (out / (label + '.log')).open('w', encoding='utf-8', newline='\n') as log:
            r = subprocess.run([str(binary), 'solve', str(cfg), '--out', str(out / label), '--threads', '8'], stdout=log, stderr=subprocess.STDOUT)
        print(label, r.returncode, flush=True)
    for storage in ('i16-f32avg', 'i16'):
        label = tree + '-' + storage
        with (out / (label + '-compare.json')).open('w', encoding='utf-8', newline='\n') as output:
            r = subprocess.run([str(binary), 'compare', str(out / (tree + '-f32') / 'solution.sol'), str(out / label / 'solution.sol')], stdout=output)
        print(label, 'compare', r.returncode, flush=True)
