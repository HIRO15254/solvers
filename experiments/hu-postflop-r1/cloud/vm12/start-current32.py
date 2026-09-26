"""Start one bounded current32 unit after the caller's explicit resize/reboot."""
from __future__ import annotations
import argparse
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import urllib.request

sys.dont_write_bytecode = True

PACKAGE = Path('/opt/r1/current-deployment01')
UNIT = 'solvers-r1-vm12-current32'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def utc(text):
    value = dt.datetime.fromisoformat(text.replace('Z', '+00:00'))
    require(value.utcoffset() == dt.timedelta(0), 'explicit UTC deadline required')
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--deadline-utc', required=True)
    parser.add_argument('--stop-deadline-utc', required=True)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('trusted_current32_installer', PACKAGE / 'install-inputs.py')
    installer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(installer)
    inputs = installer.verify_installed(PACKAGE)
    deadline, stop = utc(args.deadline_utc), utc(args.stop_deadline_utc)
    now = dt.datetime.now(dt.timezone.utc)
    remaining = (deadline - now).total_seconds()
    require(1220 < remaining <= 3600 and (stop - deadline).total_seconds() >= 900,
            'deadline must fit build, be within one hour, and reserve at least 15 min before STOP')
    require(os.cpu_count() == 32 and len(os.sched_getaffinity(0)) == 32, '32 CPUs/full affinity required')
    bootstrap = Path('/opt/r1/bootstrap-complete')
    boot_time = next(int(line.split()[1]) for line in Path('/proc/stat').read_text().splitlines() if line.startswith('btime '))
    require(bootstrap.is_file() and not bootstrap.is_symlink() and bootstrap.stat().st_mtime >= boot_time,
            'current-boot bootstrap completion required')
    for name in ('current32-proof01', 'target/current32-proof01', 'current32-start01.json',
                 'current32-verification01.json', 'current32-verification01.stderr.log', 'current32-verification01.exit'):
        path = Path('/opt/r1') / name
        require(not path.exists() and not path.is_symlink(), 'fresh deployment output required: ' + name)
    state = subprocess.run(['systemctl', 'show', UNIT, '-p', 'LoadState', '-p', 'ActiveState', '-p', 'MainPID'],
                           check=False, capture_output=True, text=True, timeout=15)
    values = dict(line.split('=', 1) for line in state.stdout.splitlines() if '=' in line)
    require(values.get('LoadState') == 'not-found' and values.get('MainPID') == '0'
            and values.get('ActiveState') == 'inactive', 'unit already exists or state unavailable')
    request = urllib.request.Request('http://metadata.google.internal/computeMetadata/v1/instance/id',
                                     headers={'Metadata-Flavor': 'Google'})
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(request, timeout=5) as response:
        require(response.headers.get('Metadata-Flavor') == 'Google', 'metadata response differs')
        instance_id = response.read(128).decode().strip()
    require(instance_id.isdecimal(), 'instance ID missing')
    # Reserve 20 seconds for dispatch and unit termination inside the fixed deadline.
    remaining = int(deadline.timestamp() - time.time())
    require(remaining > 1220, 'deadline consumed during preflight')
    command = ['systemd-run', '--unit=' + UNIT, '--property=RuntimeMaxSec=' + str(remaining - 20),
               '--property=MemoryMax=12G', '--property=MemorySwapMax=0', '--property=CPUWeight=100',
               '--property=TimeoutStopSec=10', '--property=KillMode=control-group', '--property=SendSIGKILL=yes',
               '/bin/bash', str(PACKAGE / 'run-current32.sh'), args.deadline_utc]
    record = {'schema': 'r1.current32-deployment/v1', 'time_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
              'command': command, 'deadline_utc': args.deadline_utc, 'cloud_stop_deadline_utc': args.stop_deadline_utc,
              'boot_id': Path('/proc/sys/kernel/random/boot_id').read_text().strip(), 'instance_id': instance_id,
              'logical_cpus': os.cpu_count(), 'affinity': sorted(os.sched_getaffinity(0)),
              'bootstrap_sha256': installer.digest(bootstrap.read_bytes())[1], 'inputs': inputs}
    with Path('/opt/r1/current32-start01.json').open('x') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    subprocess.run(command, check=True, timeout=30)
    print(json.dumps(record))


if __name__ == '__main__':
    main()
