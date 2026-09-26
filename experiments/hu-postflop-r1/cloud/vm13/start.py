"""Dispatch one immutable, bounded current-phase unit on the reserved VM."""
from __future__ import annotations
import argparse
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import urllib.request

sys.dont_write_bytecode = True
PACKAGE = Path('/opt/r1/phase-deployment01')
UNIT = 'solvers-r1-vm13-current-phases'


def require(ok, message):
    if not ok:
        raise ValueError(message)


def utc(value):
    parsed = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    require(parsed.utcoffset() == dt.timedelta(0), 'explicit UTC required')
    return parsed


def deadlines(created, work, stop, now):
    created, work, stop = map(utc, (created, work, stop))
    require(created <= now < work < stop, 'invalid deadline chronology')
    require((stop-created).total_seconds() <= 3600 and
            (work-created).total_seconds() <= 2700 and
            (stop-work).total_seconds() >= 900, 'deadline bounds violated')
    remaining = int((work-now).total_seconds())
    require(remaining > 1860, 'two fresh builds no longer fit the work window')
    return remaining - 20


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--instance-created-utc', required=True)
    parser.add_argument('--instance-id', required=True)
    parser.add_argument('--work-deadline-utc', required=True)
    parser.add_argument('--stop-deadline-utc', required=True)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('trusted_phase_install', PACKAGE / 'install.py')
    installer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(installer)
    manifest = installer.verify_package(PACKAGE)
    deadlines(args.instance_created_utc, args.work_deadline_utc, args.stop_deadline_utc,
              dt.datetime.now(dt.timezone.utc))
    require(os.cpu_count() == 4 and len(os.sched_getaffinity(0)) == 4, 'four CPUs and full affinity required')
    bootstrap = Path('/opt/r1/bootstrap-complete')
    boot_time = next(int(line.split()[1]) for line in Path('/proc/stat').read_text().splitlines()
                     if line.startswith('btime '))
    require(bootstrap.is_file() and not bootstrap.is_symlink() and bootstrap.stat().st_mtime >= boot_time,
            'current-boot bootstrap completion required')
    for name in ('current-phase-proof01', 'current-phase-work01', 'current-phase-start01.json',
                 'current-phase-launch01.json'):
        path = Path('/opt/r1') / name
        require(not path.exists() and not path.is_symlink(), 'output must be fresh: ' + name)
    state = subprocess.run(['systemctl', 'show', UNIT, '-p', 'LoadState', '-p', 'ActiveState', '-p', 'MainPID'],
                           capture_output=True, text=True, timeout=10, check=False)
    fields = dict(line.split('=', 1) for line in state.stdout.splitlines() if '=' in line)
    require(fields.get('LoadState') == 'not-found' and fields.get('ActiveState') == 'inactive'
            and fields.get('MainPID') == '0', 'unit already exists or state unavailable')
    request = urllib.request.Request('http://metadata.google.internal/computeMetadata/v1/instance/id',
                                     headers={'Metadata-Flavor': 'Google'})
    with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=5) as response:
        require(response.headers.get('Metadata-Flavor') == 'Google', 'invalid metadata response')
        instance_id = response.read(128).decode().strip()
    require(instance_id.isdecimal() and instance_id == args.instance_id, 'instance identity differs')
    runtime = deadlines(args.instance_created_utc, args.work_deadline_utc, args.stop_deadline_utc,
                        dt.datetime.now(dt.timezone.utc))
    command = ['systemd-run', '--unit=' + UNIT, '--property=RuntimeMaxSec=' + str(runtime),
               '--property=MemoryMax=12G', '--property=MemorySwapMax=0', '--property=CPUWeight=100',
               '--property=TimeoutStopSec=10', '--property=KillMode=control-group', '--property=SendSIGKILL=yes',
               '/usr/bin/python3', '-B', str(PACKAGE / 'run.py')]
    record = {'schema': 'r1.current-phases-start/v1', 'command': command, 'unit': UNIT,
              'instance_created_utc': args.instance_created_utc, 'instance_id': instance_id,
              'work_deadline_utc': args.work_deadline_utc, 'stop_deadline_utc': args.stop_deadline_utc,
              'runtime_max_seconds': runtime, 'dispatch_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
              'boot_id': Path('/proc/sys/kernel/random/boot_id').read_text().strip(),
              'logical_cpus': os.cpu_count(), 'affinity': sorted(os.sched_getaffinity(0)),
              'manifest': installer.pin(PACKAGE / 'manifest.json'), 'bootstrap': installer.pin(bootstrap),
              'source_revision': manifest['source_revision']}
    with Path('/opt/r1/current-phase-start01.json').open('x') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    subprocess.run(command, check=True, timeout=30)
    print(json.dumps(record))


if __name__ == '__main__':
    main()
