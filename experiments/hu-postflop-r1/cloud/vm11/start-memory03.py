"""One new focused-memory campaign inside VM11's unchanged reservation/deadline."""
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess

deadline = '2026-09-26T21:20:00Z'
wrapper = Path('/tmp/run-memory03.sh')
control = Path('/opt/r1/final-control/focused-memory')
manifest = json.loads(Path('/tmp/memory-controls03.json').read_bytes())
assert manifest['schema'] == 'r1.focused-memory-deployment-inputs/v1'
for row in manifest['files']:
    path = Path(row['remote_path'])
    raw = path.read_bytes()
    assert len(raw) == row['bytes']
    assert hashlib.sha256(raw).hexdigest() == row['sha256'], str(path)
for unit in ('solvers-r1-vm11-final', 'solvers-r1-vm11-final02'):
    properties = subprocess.check_output(['systemctl', 'show', unit, '-p', 'MainPID', '-p', 'ActiveState'], text=True)
    values = dict(line.split('=', 1) for line in properties.splitlines())
    assert values['MainPID'] == '0' and values['ActiveState'] in ('inactive', 'failed'), values
assert json.loads(Path('/opt/r1/final-proof02/result.json').read_bytes())['status'] == 'completed'
assert json.loads((control / 'protocol.json').read_bytes())['deadline_ceiling_utc'] == deadline
assert not Path('/opt/r1/focused-memory03').exists()
boot = Path('/proc/sys/kernel/random/boot_id').read_text().strip()
assert boot == '7d1d9913-8d5d-420b-934e-2129e2ce25e6', boot
remaining = int((dt.datetime.fromisoformat(deadline.replace('Z', '+00:00')) - dt.datetime.now(dt.timezone.utc)).total_seconds())
assert 900 < remaining < 3 * 3600
command = ['systemd-run', '--unit=solvers-r1-vm11-memory03', f'--property=RuntimeMaxSec={remaining}',
           '--property=MemoryMax=12G', '--property=MemorySwapMax=0', '--property=TimeoutStopSec=15',
           '--property=KillMode=control-group', '--property=CPUWeight=100', '/bin/bash', str(wrapper), deadline]
record = {'schema': 'r1.focused-memory-deployment/v1', 'time_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
          'reference_output': '/opt/r1/final-proof02', 'proof02_memory_screen_revised': False,
          'deadline_utc': deadline, 'cloud_stop_deadline_utc': '2026-09-26T21:54:51Z', 'command': command,
          'boot_id': boot, 'reservation_delta_usd': 0, 'held_total_usd': 35, 'authorized_limit_usd': 40,
          'inputs': manifest, 'wrapper_sha256': hashlib.sha256(wrapper.read_bytes()).hexdigest()}
with Path('/opt/r1/memory-deployment03.json').open('x') as out:
    json.dump(record, out, indent=2)
    out.write('\n')
subprocess.run(command, check=True)
print(json.dumps(record))
