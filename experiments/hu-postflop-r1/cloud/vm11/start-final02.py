"""One explicit deployment correction after proof01 failed before measurement."""
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess

deadline = '2026-09-26T21:34:51Z'
wrapper = Path('/tmp/run-final02.sh')
previous = json.loads(Path('/opt/r1/final-proof01/result.json').read_bytes())
assert previous['status'] == 'failed'
assert all(row['status'] == 'skipped' for row in previous['stages'])
assert not Path('/opt/r1/final-proof02').exists()
assert not Path('/opt/r1/target/final-old02').exists()
assert not Path('/opt/r1/target/final-new02').exists()
remaining = int((dt.datetime.fromisoformat(deadline.replace('Z', '+00:00')) - dt.datetime.now(dt.timezone.utc)).total_seconds())
assert 1800 < remaining < 3 * 3600
command = ['systemd-run', '--unit=solvers-r1-vm11-final02', f'--property=RuntimeMaxSec={remaining}',
           '--property=MemoryMax=12G', '--property=MemorySwapMax=0', '--property=TimeoutStopSec=15',
           '--property=KillMode=control-group', '--property=CPUWeight=100', '/bin/bash', str(wrapper), deadline]
record = {'schema': 'r1.final-pipeline-deployment-correction/v1', 'time_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
          'previous_output': '/opt/r1/final-proof01', 'previous_status': previous['status'],
          'measurement_samples_observed_before_correction': 0,
          'change': 'Materialize the unit CPU controller before prepare; all controls and both production sources unchanged; fresh build targets and a new proof directory.',
          'deadline_utc': deadline, 'command': command,
          'boot_id': Path('/proc/sys/kernel/random/boot_id').read_text().strip(),
          'wrapper_sha256': hashlib.sha256(wrapper.read_bytes()).hexdigest()}
with Path('/opt/r1/final-deployment02.json').open('x') as out:
    json.dump(record, out, indent=2)
    out.write('\n')
subprocess.run(command, check=True)
print(json.dumps(record))
