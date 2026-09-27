"""Read only compact VM18 phase/service status and bounded log tails."""
import json
from pathlib import Path
import subprocess

PREFIX = Path('/opt/r1')
proof = PREFIX / 'flop-chance-grain-proof01'
result = {}
for phase, suffix in [('build', 'build2'), ('measure', 'measure32')]:
    done = subprocess.run(['systemctl', 'show', 'solvers-r1-vm18-' + suffix,
                           '-p', 'ActiveState', '-p', 'MainPID', '-p', 'Result'],
                          capture_output=True, text=True, timeout=5)
    item = {'service_exit': done.returncode, 'service': done.stdout.strip()}
    path = proof / ('build-execution.json' if phase == 'build' else 'execution.json')
    if path.exists():
        data = json.loads(path.read_text())
        item['status'] = data['status']
        item['stages'] = [{'name': r['name'], 'status': r['status'],
                          **{k: r[k] for k in ('started_at', 'ended_at', 'error') if k in r}}
                         for r in data['stages'] if r['status'] != 'pending'][-5:]
        item['counts'] = {s: sum(r['status'] == s for r in data['stages'])
                          for s in ('completed', 'running', 'failed', 'skipped', 'pending')}
        item['error'] = data.get('error')
        live = [r for r in data['stages'] if r['status'] in ('running', 'failed')]
        if live:
            log = proof / live[-1]['name'] / 'supervisor.stderr.log'
            if log.exists():
                with log.open('rb') as f:
                    f.seek(max(0, log.stat().st_size - 4096))
                    item['stage_stderr_tail'] = f.read().decode(errors='replace')[-2000:]
    wrapper = PREFIX / ('flop-chance-grain-' + phase + '-wrapper01')
    finish = wrapper / 'finish.json'
    if finish.exists():
        data = json.loads(finish.read_text())
        item['wrapper_finish'] = {k: data[k] for k in ('exit_code', 'last_phase', 'ended_at')}
    if not path.exists() and wrapper.exists():
        item['wrapper_files'] = [p.name for p in wrapper.iterdir()]
        for log in wrapper.glob('*.stderr.log'):
            with log.open('rb') as f:
                f.seek(max(0, log.stat().st_size - 1200))
                item[log.name] = f.read().decode(errors='replace')
    result[phase] = item
source = proof / 'source-baseline.tar.gz'
if source.exists():
    import hashlib
    result['source_archive'] = {'bytes': source.stat().st_size, 'sha256': hashlib.sha256(source.read_bytes()).hexdigest()}
print(json.dumps(result, indent=2))
