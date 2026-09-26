"""Inside-unit entry: record actual systemd limits, then exec the finite runner."""
from __future__ import annotations
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
PACKAGE = Path('/opt/r1/phase-deployment01')


def main():
    spec = importlib.util.spec_from_file_location('trusted_phase_runner', PACKAGE / 'control/current-phases/runner.py')
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    start = json.loads(Path('/opt/r1/current-phase-start01.json').read_bytes())
    unit = start['unit']
    runner.require(unit == 'solvers-r1-vm13-current-phases', 'unexpected unit')
    result = subprocess.run(['/usr/bin/systemctl', 'show', unit, '--no-pager',
        '--property=RuntimeMaxUSec,ExecMainStartTimestamp,MainPID,ActiveState'],
        capture_output=True, text=True, check=True, timeout=10, env={**os.environ, 'TZ': 'UTC', 'LC_ALL': 'C'})
    fields = dict(line.split('=', 1) for line in result.stdout.splitlines())
    runner.require(fields['MainPID'] == str(os.getpid()) and fields['ActiveState'] == 'active',
                   'entry must be the active unit main process')
    # systemctl prints a UTC C-locale timestamp with one-second precision.
    started = dt.datetime.strptime(fields['ExecMainStartTimestamp'], '%a %Y-%m-%d %H:%M:%S UTC').replace(tzinfo=dt.timezone.utc)
    runtime = runner.systemd_seconds(fields['RuntimeMaxUSec'])
    runner.require(runtime == start['runtime_max_seconds'], 'systemd runtime differs from dispatch')
    runner.require(runtime + started.timestamp() <= runner.timestamp(start['work_deadline_utc']),
                   'unit can outlive work deadline')
    boot = Path('/proc/sys/kernel/random/boot_id').read_text().strip()
    runner.require(boot == start['boot_id'], 'boot changed after dispatch')
    launch = {key: start[key] for key in ('instance_created_utc', 'work_deadline_utc', 'stop_deadline_utc',
                                         'runtime_max_seconds', 'boot_id', 'unit', 'instance_id')}
    launch.update(schema='r1.current-phases-launch/v1', unit_started_utc=started.isoformat(),
                  unit_start_resolution_seconds=1, runtime_max_systemd=fields['RuntimeMaxUSec'],
                  raw_systemd=fields)
    path = Path('/opt/r1/current-phase-launch01.json')
    with path.open('x') as stream:
        json.dump(launch, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    toolchain = '/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/'
    command = ['/usr/bin/python3', '-B', str(PACKAGE / 'control/current-phases/runner.py'),
        '--source', str(PACKAGE / 'source'), '--workspace', '/opt/r1/current-phase-work01',
        '--out', '/opt/r1/current-phase-proof01', '--reference-proof', str(PACKAGE / 'reference/final-proof02'),
        '--cargo', toolchain + 'cargo', '--rustc', toolchain + 'rustc', '--cc', '/usr/bin/gcc',
        '--supervisor', str(PACKAGE / 'control/tools/run_supervised.py'), '--cargo-home', '/opt/r1/cargo',
        '--rustup-home', '/opt/r1/rustup', '--work-deadline-utc', start['work_deadline_utc'],
        '--stop-deadline-utc', start['stop_deadline_utc'], '--launch-record', str(path)]
    os.execv(command[0], command)


if __name__ == '__main__':
    main()
