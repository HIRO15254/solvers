"""Start the separate finite diagnostic only after phase writers are quiescent."""
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess
import time

PACKAGE = Path('/opt/r1/native-river-deployment01')
PRIOR = 'solvers-r1-vm13-current-phases'
UNIT = 'solvers-r1-vm13-native-river'


def require(ok, message):
    if not ok:
        raise ValueError(message)


def state(unit):
    result = subprocess.run(['systemctl', 'show', unit, '-p', 'LoadState', '-p', 'ActiveState',
                             '-p', 'MainPID', '-p', 'ControlGroup'], capture_output=True, text=True, timeout=10)
    return dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)


def main():
    manifest = json.loads((PACKAGE / 'manifest.json').read_bytes())
    require(manifest['schema'] == 'r1.native-river-deployment/v1', 'wrong package')
    actual = set()
    for path in PACKAGE.rglob('*'):
        require(not path.is_symlink(), 'package link')
        if path.is_file():
            name = path.relative_to(PACKAGE).as_posix()
            actual.add(name)
            if name != 'manifest.json':
                raw = path.read_bytes()
                require(manifest['files'][name] == {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()},
                        'package changed: ' + name)
    require(actual == set(manifest['files']) | {'manifest.json'}, 'package inventory differs')
    old = state(PRIOR)
    require(old.get('ActiveState') in ('inactive', 'failed') and old.get('MainPID') == '0', 'phase unit active')
    cg = old.get('ControlGroup')
    if cg:
        require(cg.startswith('/system.slice/'), 'unexpected prior cgroup')
        for path in Path('/sys/fs/cgroup' + cg).rglob('cgroup.procs'):
            require(not path.read_text().strip(), 'phase child still active')
    new = state(UNIT)
    require(new.get('LoadState') == 'not-found' and new.get('ActiveState') == 'inactive'
            and new.get('MainPID') == '0', 'diagnostic unit already exists')
    plan = json.loads(Path('/opt/r1/current-phase-proof01/plan.json').read_bytes())
    result = json.loads(Path('/opt/r1/current-phase-proof01/result.json').read_bytes())
    require(result['status'] in ('completed', 'failed'), 'prior result not terminal')
    require(Path('/proc/sys/kernel/random/boot_id').read_text().strip() == plan['launch']['boot_id'], 'boot changed')
    work = dt.datetime.fromisoformat(plan['work_deadline_utc']).timestamp()
    require(time.time() + 260 < work, 'full diagnostic and cleanup no longer fit')
    for name in ('native-river-proof01', 'native-river-start01.json'):
        path = Path('/opt/r1') / name
        require(not path.exists() and not path.is_symlink(), 'new output required')
    command = ['systemd-run', '--unit=' + UNIT, '--property=RuntimeMaxSec=240',
        '--property=MemoryMax=12G', '--property=MemorySwapMax=0', '--property=CPUWeight=100',
        '--property=KillMode=control-group', '--property=SendSIGKILL=yes', '--property=TimeoutStopSec=10',
        '/usr/bin/python3', '-B', str(PACKAGE / 'run.py'), '--phase', 'run',
        '--control', '/opt/r1/phase-deployment01/control', '--phase-proof', '/opt/r1/current-phase-proof01',
        '--out', '/opt/r1/native-river-proof01', '--work-deadline-utc', plan['work_deadline_utc']]
    record = {'schema': 'r1.native-river-start/v1', 'dispatch_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
              'instance_id': plan['launch']['instance_id'], 'boot_id': plan['launch']['boot_id'],
              'work_deadline_utc': plan['work_deadline_utc'], 'stop_deadline_utc': plan['stop_deadline_utc'],
              'unit': UNIT, 'prior_unit_state': old, 'command': command,
              'manifest_sha256': hashlib.sha256((PACKAGE / 'manifest.json').read_bytes()).hexdigest()}
    with Path('/opt/r1/native-river-start01.json').open('x') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
    subprocess.run(command, check=True, timeout=30)
    print(json.dumps(record))


if __name__ == '__main__':
    main()
