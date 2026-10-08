from pathlib import Path
import subprocess, json, hashlib
root = Path(__file__).resolve().parent
probe = root / 'artifact-probe.exe'
def run(variant, cfg):
    stem = f'{variant}-{cfg.stem}'
    directory = root / ('solve-' + stem)
    assert not directory.exists(), directory
    with (root / (stem + '.log')).open('w', encoding='utf8') as log:
        subprocess.run([str(root / f'solvers-{variant}.exe'), 'solve', str(cfg), '--out', str(directory)], stdout=log, stderr=subprocess.STDOUT, check=True)
    prefix = root / stem
    subprocess.run([str(probe), str(directory/'solution.sol'), str(directory/'checkpoint.ckpt'), str(prefix)], check=True)
    rows = [json.loads(line) for line in (directory/'progress.jsonl').read_text().splitlines() if line.strip()]
    for row in rows: row.pop('elapsed_secs')
    progress = json.dumps(rows, sort_keys=True, separators=(',', ':')).encode()
    (root/(stem+'.progress-metrics.json')).write_bytes(progress)
    print('Solved', stem, flush=True)
    return {'sol': prefix.with_suffix('.sol-payload').read_bytes(), 'state': prefix.with_suffix('.checkpoint-state').read_bytes(), 'progress': progress,
        'header': (directory/'checkpoint.ckpt').read_bytes()[:50], 'rows': len(rows)}
results = []
for cfg in sorted(root.glob('*-f64-*.toml')):
    a, b = run('head', cfg), run('new', cfg)
    for field in ['sol', 'state', 'progress', 'header']: assert a[field] == b[field], (cfg.name, field)
    results.append({'case':cfg.name,'precision':'f64','compared_to':'f918dc5','sol_payload_equal':True,'checkpoint_arenas_equal':True,'checkpoint_header_equal':True,'progress_equal':True,'progress_rows':a['rows'], 'sol_sha256':hashlib.sha256(a['sol']).hexdigest(), 'state_sha256':hashlib.sha256(a['state']).hexdigest()})
for storage in ['f32','i16','i16-f32avg']:
    a = run('new', root/f'turn-f32-{storage}-1.toml')
    b = run('new', root/f'turn-f32-{storage}-4.toml')
    for field in ['sol','state','progress']: assert a[field] == b[field], (storage, field)
    results.append({'case':f'turn-f32-{storage}-1-vs-4','precision':'f32','sol_payload_equal':True,'checkpoint_arenas_equal':True,'progress_equal':True,'progress_rows':a['rows'],'sol_sha256':hashlib.sha256(a['sol']).hexdigest(),'state_sha256':hashlib.sha256(a['state']).hexdigest()})
report={'profile':'debug; CARGO_INCREMENTAL=0','exclusions':['sol.meta.wall_secs','progress.elapsed_secs','sol.config_toml operational threads line for cross-thread comparison'],'results':results}
(root/'artifact-comparison.json').write_text(json.dumps(report, indent=2))
print(f'PASS: {len(results)} comparisons; 12 HEAD/new f64, 3 new f32 across threads', flush=True)
