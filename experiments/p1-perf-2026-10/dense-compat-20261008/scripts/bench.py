import ctypes
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
SCRATCH = Path(r'C:\Users\PC_User\AppData\Local\Temp\claude\C--Users-PC-User-orca-workspaces-solvers-cisco\7b8f3bb9-2812-4052-b020-84dc5d1829b4\scratchpad')
kernel = ctypes.WinDLL('kernel32', use_last_error=True)
kernel.GetCurrentProcess.restype = ctypes.c_void_p
kernel.GetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_size_t)]
kernel.SetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
process_mask, system_mask = ctypes.c_size_t(), ctypes.c_size_t()
handle = kernel.GetCurrentProcess()
assert kernel.GetProcessAffinityMask(handle, ctypes.byref(process_mask), ctypes.byref(system_mask))
mask = process_mask.value & -process_mask.value
assert kernel.SetProcessAffinityMask(handle, mask)
os.environ['RAYON_NUM_THREADS'] = '1'
variants = sys.argv[1].split(',')
label = sys.argv[2]
pattern = sys.argv[3] if len(sys.argv) > 3 else r'^(kernels(_wide)?/(fold|showdown)_f32|kernels_realistic/(t18_fold|t18_showdown|t20_siblings|t21_opponent_add))$'
records = []
for repeat in range(3):
    for variant in variants:
        exe = SCRATCH / f't24-bench-{variant}.exe'
        args = [str(exe), '--bench', pattern, '--noplot']
        log = ROOT / 'runs/t24' / f'{label}-{repeat + 1}-{variant}.log'
        print(f'{label}: repeat {repeat + 1} {variant}, affinity {mask:#x}', flush=True)
        with log.open('w', encoding='utf-8') as stream:
            subprocess.run(args, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, check=True)
        import re
        output = log.read_text(encoding='utf-8')
        names = re.findall(r'Benchmarking (\S+): Analyzing', output)
        for name in names:
            estimate_file = ROOT / 'target/criterion' / name / 'new/estimates.json'
            estimate = json.loads(estimate_file.read_text())
            records.append(dict(variant=variant, repeat=repeat + 1, bench=name,
                                median_ns=estimate['median']['point_estimate']))
        (ROOT / 'runs/t24' / f'{label}.json').write_text(json.dumps(dict(affinity_mask=mask, args=args, records=records), indent=2))
for name in sorted({r['bench'] for r in records}):
    print(name, {v: statistics.median(r['median_ns'] for r in records if r['bench'] == name and r['variant'] == v) for v in variants}, flush=True)
