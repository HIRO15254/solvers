"""Read-only raw capture of the second explicitly bounded service."""
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
from pathlib import Path
import os
import subprocess

here = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('launch_capture', here / 'collect-launch-runtime.py')
config = importlib.util.module_from_spec(spec)
spec.loader.exec_module(config)
env = os.environ.copy()
env.update(CLOUDSDK_PYTHON='C:/Python313/python.exe', CLOUDSDK_ENCODING='utf-8', PYTHONIOENCODING='utf-8')
records = []
commands = {
    'deployment02': 'cat /opt/r1/final-deployment02.json',
    'service02': 'systemctl show solvers-r1-vm11-final02 --property=Id,ActiveState,SubState,ExecMainPID,ExecMainStartTimestamp,InvocationID,ExecMainStatus,Result,MemoryMax,MemorySwapMax,CPUWeight,RuntimeMaxUSec,KillMode,TimeoutStopUSec',
}
for label, command in commands.items():
    argv = [config.GCLOUD, *config.SSH, '--command=' + command]
    started = datetime.now(timezone.utc).isoformat()
    process = subprocess.run(argv, capture_output=True, timeout=90, env=env)
    row = {'label': label, 'argv': argv, 'started_at_utc': started,
           'ended_at_utc': datetime.now(timezone.utc).isoformat(), 'exit_code': process.returncode}
    for kind, data in (('stdout', process.stdout), ('stderr', process.stderr)):
        path = here / f'{label}.{kind}.log'
        with path.open('xb') as out:
            out.write(data)
        row[kind] = {'path': path.name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
    records.append(row)
with (here / 'capture-final02.json').open('x', encoding='utf-8', newline='\n') as out:
    json.dump({'schema': 'r1.vm-read-only-runtime/v1', 'records': records}, out, indent=2)
    out.write('\n')
print(json.dumps([{'label': row['label'], 'exit_code': row['exit_code']} for row in records]))
