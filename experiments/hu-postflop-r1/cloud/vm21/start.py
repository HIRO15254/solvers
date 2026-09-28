"""Verify one installed package and dispatch one bounded build or measurement."""
import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import time
import urllib.request

PREFIX = Path('/opt/r1')
PACKAGE = PREFIX / 'sparse-rank-package'
HERE = PACKAGE / 'experiments/hu-postflop-r1/cloud/vm21'
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
    need(manifest['schema'] == 'r1-cpu-profile-package/v1', 'package schema differs')
    installation = json.loads((PACKAGE / 'installation.json').read_text())
    need(installation['manifest'] == pin(PACKAGE / 'manifest.json'), 'installation binding differs')
    for name, expected in manifest['files'].items():
        path = PurePosixPath(name)
        need(not path.is_absolute() and '..' not in path.parts and '\\' not in name, 'unsafe manifest path')
        actual = PACKAGE / name
        need(all(not p.is_symlink() for p in [actual, *actual.parents]), 'input symlink')
        need(pin(actual) == expected, 'package input changed: ' + name)
    source = PACKAGE / 'source-baseline'
    expected = dict(manifest['source_pins'])
    expected['crates/holdem/examples/flop_cpu_profile_probe.rs'] = pin(PACKAGE / 'experiments/hu-postflop-r1/flop-scaling/cpu-profile/adapter/solve.rs')
    actual = {}
    for path in sorted(source.rglob('*')):
        need(not path.is_symlink(), 'source symlink')
        if path.is_file():
            actual[path.relative_to(source).as_posix()] = pin(path)
    need(actual == expected, 'source membership/content differs')
    overlay = json.loads((PACKAGE / 'overlay-manifest.json').read_bytes())
    applied = json.loads((PACKAGE / 'overlay-installation.json').read_bytes())
    need(overlay['schema'] == 'r1-sparse-rank-groups-overlay/v1'
         and overlay['base_manifest'] == pin(PACKAGE / 'manifest.json')
         and applied['schema'] == 'r1-sparse-rank-groups-overlay-installation/v1'
         and applied['manifest'] == pin(PACKAGE / 'overlay-manifest.json')
         and applied['destination'] == str(PACKAGE), 'overlay binding differs')
    need(not (set(overlay['files']) & set(manifest['files'])), 'overlay collision')
    for name, expected_pin in overlay['files'].items():
        rel = PurePosixPath(name)
        need(not rel.is_absolute() and '..' not in rel.parts and '\\' not in name, 'unsafe overlay path')
        need(pin(PACKAGE / rel) == expected_pin, 'overlay changed: ' + name)
    return {'overlay_manifest': pin(PACKAGE / 'overlay-manifest.json'),
            'overlay_installation': pin(PACKAGE / 'overlay-installation.json'),
            'manifest': pin(PACKAGE / 'manifest.json'), 'installation': pin(PACKAGE / 'installation.json'),
            'source_revision': manifest['source_revision'], 'package_files': len(manifest['files'])}


def utc(value):
    parsed = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    need(parsed.utcoffset() == dt.timedelta(0), 'explicit UTC required')
    return parsed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verify-only', action='store_true')
    parser.add_argument('--phase', choices=['build', 'measure'])
    parser.add_argument('--deadline-utc')
    parser.add_argument('--stop-deadline-utc')
    parser.add_argument('--launch-attempted-at')
    parser.add_argument('--highcpu-armed-at')
    parser.add_argument('--highcpu-stop-deadline-utc')
    args = parser.parse_args()
    inputs = verify()
    if args.verify_only:
        print(json.dumps(inputs))
        return
    need(args.phase and args.stop_deadline_utc and args.launch_attempted_at, 'phase, launch and original STOP required')
    if args.phase == 'measure':
        need(args.highcpu_armed_at and args.highcpu_stop_deadline_utc, 'fixed highCPU phase required')
        armed, high_stop = utc(args.highcpu_armed_at), utc(args.highcpu_stop_deadline_utc)
        need(479 < (high_stop - armed).total_seconds() <= 480, '8 minute phase STOP differs')
        need(armed <= dt.datetime.now(dt.timezone.utc) and high_stop <= utc(args.stop_deadline_utc), 'phase outside lifecycle')
        if args.deadline_utc is None:
            args.deadline_utc = min(dt.datetime.now(dt.timezone.utc) + dt.timedelta(seconds=360), high_stop - dt.timedelta(seconds=20)).isoformat()
    need(args.deadline_utc, 'build deadline required')
    deadline, stop = utc(args.deadline_utc), utc(args.stop_deadline_utc)
    launch = utc(args.launch_attempted_at)
    need(2699 < (stop - launch).total_seconds() <= 2700, 'original45min STOP differs')
    remaining = deadline.timestamp() - time.time()
    cpus = 2 if args.phase == 'build' else 32
    unit = 'solvers-r1-vm21-' + ('build2' if args.phase == 'build' else 'measure32')
    need(os.cpu_count() == cpus and len(os.sched_getaffinity(0)) == cpus, 'CPU count/full affinity differs')
    if args.phase == 'build':
        need(480 < remaining <= 1200 and (stop - deadline).total_seconds() == 1500,
             'build deadline must be launch+20min within original45min STOP')
        for name in ('sparse-rank-work01', 'sparse-rank-proof01'):
            need(not (PREFIX / name).exists(), 'fresh build path required')
    else:
        need(260 < remaining <= 360 and deadline <= high_stop - dt.timedelta(seconds=20),
             'measurement requires >260..360 seconds within fixed highCPU STOP')
        prior = json.loads((PREFIX / 'sparse-rank-build-start01.json').read_text())
        need(prior['cloud_stop_deadline_utc'] == args.stop_deadline_utc, 'original STOP differs')
        need(prior['launch_attempted_at'] == args.launch_attempted_at, 'original launch differs')
        need(prior['boot_id'] != Path('/proc/sys/kernel/random/boot_id').read_text().strip(), 'measurement must follow resize/reboot')
        need(prior['inputs'] == inputs, 'package changed between boots')
        proof = json.loads((PREFIX / 'sparse-rank-proof01/build.json').read_text())
        need(proof['status'] == 'completed', 'successful build receipt required')
    bootstrap = PREFIX / 'bootstrap-complete'
    need(bootstrap.is_file() and not bootstrap.is_symlink(), 'bootstrap completion missing')
    record_path = PREFIX / ('sparse-rank-' + args.phase + '-start01.json')
    need(not record_path.exists() and not record_path.is_symlink(), 'phase already attempted')
    state = subprocess.run(['systemctl', 'show', unit, '-p', 'LoadState', '-p', 'ActiveState', '-p', 'MainPID'],
                           capture_output=True, text=True, timeout=15, check=True)
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
    request = urllib.request.Request('http://metadata.google.internal/computeMetadata/v1/instance/machine-type',
                                     headers={'Metadata-Flavor': 'Google'})
    with opener.open(request, timeout=5) as response:
        need(response.headers.get('Metadata-Flavor') == 'Google', 'machine metadata response differs')
        machine = response.read(1024).decode().strip().rsplit('/', 1)[-1]
    need(machine == ('e2-standard-2' if args.phase == 'build' else 'e2-highcpu-32'), 'E2-only machine differs')
    if args.phase == 'measure':
        need(prior['instance_id'] == instance_id, 'instance changed between phases')
    remaining = int(deadline.timestamp() - time.time())
    need(remaining > (480 if args.phase == 'build' else 250), 'deadline consumed during preflight')
    command = ['systemd-run', '--unit=' + unit, '--property=RuntimeMaxSec=' + str(remaining - 10),
               '--property=MemoryMax=' + ('6G' if cpus == 2 else '12G'), '--property=MemorySwapMax=0',
               '--property=CPUWeight=100', '--property=TimeoutStopSec=10', '--property=KillMode=control-group',
               '--property=SendSIGKILL=yes', '/bin/bash', str(HERE / 'run.sh'), args.phase, args.deadline_utc,
               args.launch_attempted_at, args.stop_deadline_utc, args.highcpu_armed_at or '-', args.highcpu_stop_deadline_utc or '-']
    record = {'schema': 'r1.sparse-rank-vm21-deployment/v1', 'phase': args.phase,
              'time_utc': dt.datetime.now(dt.timezone.utc).isoformat(), 'command': command,
              'deadline_utc': args.deadline_utc, 'cloud_stop_deadline_utc': args.stop_deadline_utc,
              'launch_attempted_at': args.launch_attempted_at,
              'highcpu_armed_at': args.highcpu_armed_at, 'highcpu_stop_deadline_utc': args.highcpu_stop_deadline_utc,
              'boot_id': Path('/proc/sys/kernel/random/boot_id').read_text().strip(), 'instance_id': instance_id,
              'machine_type': machine, 'logical_cpus': cpus, 'affinity': sorted(os.sched_getaffinity(0)), 'inputs': inputs,
              'bootstrap': pin(bootstrap), 'tools': {n: pin(TOOLCHAIN / n) for n in ('cargo', 'rustc')}}
    with record_path.open('x') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    fd = os.open(PREFIX, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)
    subprocess.run(command, check=True, timeout=30)
    print(json.dumps(record))


if __name__ == '__main__':
    main()
