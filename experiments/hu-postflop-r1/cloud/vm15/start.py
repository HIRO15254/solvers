"""Verify the installed package, then dispatch one finite VM15 comparison."""
import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import time
import urllib.request

PACKAGE = Path('/opt/r1/flop-worker-cloud32-package')
HERE = PACKAGE / 'experiments/hu-postflop-r1/cloud/vm15'
UNIT = 'solvers-r1-vm15-worker32'
PREFIX = Path('/opt/r1')
TOOLCHAIN = PREFIX / 'rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin'


def need(ok, message):
    if not ok:
        raise ValueError(message)


def pin(path):
    need(path.is_file() and not path.is_symlink(), 'regular input required: ' + str(path))
    raw = path.read_bytes()
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def verify():
    manifest = json.loads((PACKAGE / 'manifest.json').read_text())
    need(manifest['schema'] == 'r1-worker-scratch-cloud32-package/v1', 'package schema differs')
    installation = json.loads((PACKAGE / 'installation.json').read_text())
    need(installation['manifest'] == pin(PACKAGE / 'manifest.json'), 'installation binding differs')
    for name, expected in manifest['files'].items():
        path = PurePosixPath(name)
        need(not path.is_absolute() and '..' not in path.parts and '\\' not in name, 'unsafe manifest path')
        actual = PACKAGE / name
        need(all(not p.is_symlink() for p in [actual, *actual.parents]), 'input symlink')
        need(pin(actual) == expected, 'package input changed: ' + name)
    adapter = pin(PACKAGE / 'experiments/hu-postflop-r1/flop-scaling/worker-scratch/cloud32/solve.rs')
    for arm in ('baseline', 'worker'):
        source = PACKAGE / ('source-' + arm)
        expected = dict(manifest['source_pins'])
        expected['crates/holdem/examples/flop_cloud32_probe.rs'] = adapter
        if arm == 'worker':
            expected['crates/engine/src/solver.rs'] = manifest['candidate']
        actual = {}
        for path in sorted(source.rglob('*')):
            need(not path.is_symlink(), 'source symlink')
            if path.is_file():
                actual[path.relative_to(source).as_posix()] = pin(path)
        need(actual == expected, arm + ' source membership/content differs')
    return {'manifest': pin(PACKAGE / 'manifest.json'), 'installation': pin(PACKAGE / 'installation.json'),
            'source_revision': manifest['source_revision'], 'package_files': len(manifest['files'])}


def utc(value):
    parsed = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    need(parsed.utcoffset() == dt.timedelta(0), 'explicit UTC required')
    return parsed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verify-only', action='store_true')
    parser.add_argument('--deadline-utc')
    parser.add_argument('--stop-deadline-utc')
    args = parser.parse_args()
    inputs = verify()
    if args.verify_only:
        print(json.dumps(inputs))
        return
    need(args.deadline_utc and args.stop_deadline_utc, 'both deadlines required')
    deadline, stop = utc(args.deadline_utc), utc(args.stop_deadline_utc)
    remaining = deadline.timestamp() - time.time()
    need(1220 < remaining <= 2400 and (stop - deadline).total_seconds() >= 900,
         'experiment must fit40min and leave15min before fixed cloud STOP')
    need(os.cpu_count() == 32 and len(os.sched_getaffinity(0)) == 32, '32 CPUs/full affinity required')
    bootstrap = PREFIX / 'bootstrap-complete'
    need(bootstrap.is_file() and not bootstrap.is_symlink(), 'bootstrap completion missing')
    for name in ('flop-worker-cloud32-work01', 'flop-worker-cloud32-proof01',
                 'flop-worker-cloud32-wrapper01', 'flop-worker-cloud32-start01.json'):
        path = PREFIX / name
        need(not path.exists() and not path.is_symlink(), 'fresh path required: ' + name)
    state = subprocess.run(['systemctl', 'show', UNIT, '-p', 'LoadState', '-p', 'ActiveState', '-p', 'MainPID'],
                           capture_output=True, text=True, timeout=15, check=False)
    values = dict(line.split('=', 1) for line in state.stdout.splitlines() if '=' in line)
    need(values.get('LoadState') == 'not-found' and values.get('MainPID') == '0'
         and values.get('ActiveState') == 'inactive', 'unit already exists/state unavailable')
    request = urllib.request.Request('http://metadata.google.internal/computeMetadata/v1/instance/id',
                                     headers={'Metadata-Flavor': 'Google'})
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(request, timeout=5) as response:
        need(response.headers.get('Metadata-Flavor') == 'Google', 'metadata response differs')
        instance_id = response.read(128).decode().strip()
    need(instance_id.isdecimal(), 'instance identity missing')
    remaining = int(deadline.timestamp() - time.time())
    need(remaining > 1220, 'deadline consumed during preflight')
    command = ['systemd-run', '--unit=' + UNIT, '--property=RuntimeMaxSec=' + str(remaining - 20),
               '--property=MemoryMax=12G', '--property=MemorySwapMax=0', '--property=CPUWeight=100',
               '--property=TimeoutStopSec=10', '--property=KillMode=control-group', '--property=SendSIGKILL=yes',
               '/bin/bash', str(HERE / 'run.sh'), args.deadline_utc]
    record = {'schema': 'r1.worker-scratch-vm15-deployment/v1', 'time_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
              'command': command, 'deadline_utc': args.deadline_utc, 'cloud_stop_deadline_utc': args.stop_deadline_utc,
              'boot_id': Path('/proc/sys/kernel/random/boot_id').read_text().strip(), 'instance_id': instance_id,
              'logical_cpus': os.cpu_count(), 'affinity': sorted(os.sched_getaffinity(0)), 'inputs': inputs,
              'bootstrap': pin(bootstrap), 'tools': {n: pin(TOOLCHAIN / n) for n in ('cargo', 'rustc')}}
    with (PREFIX / 'flop-worker-cloud32-start01.json').open('x') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    subprocess.run(command, check=True, timeout=30)
    print(json.dumps(record))


if __name__ == '__main__':
    main()
