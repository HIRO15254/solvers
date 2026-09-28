"""Derive small cloud dispatchers from the retained VM19 controls; no execution."""
from pathlib import Path
import hashlib
import json

HERE = Path(__file__).resolve().parent
OLD = HERE.parent / 'vm19'


def once(text, old, new):
    assert text.count(old) == 1, old
    return text.replace(old, new, 1)


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def main():
    inputs = {n: (OLD / n).read_bytes() for n in ('start.py', 'run.sh', 'status.py')}
    start = inputs['start.py'].decode().replace('flop-cpu-profile', 'sparse-rank').replace('cloud/vm19', 'cloud/vm21').replace('solvers-r1-vm19-', 'solvers-r1-vm21-')
    # The original installer still keeps its unused profile example in source-baseline.
    start = once(start, "    return {'manifest': pin(PACKAGE / 'manifest.json'),", '''    overlay = json.loads((PACKAGE / 'overlay-manifest.json').read_bytes())
    applied = json.loads((PACKAGE / 'overlay-installation.json').read_bytes())
    need(overlay['schema'] == 'r1-sparse-rank-groups-overlay/v1'
         and overlay['base_manifest'] == pin(PACKAGE / 'manifest.json')
         and applied['schema'] == 'r1-sparse-rank-groups-overlay-installation/v1'
         and applied['manifest'] == pin(PACKAGE / 'overlay-manifest.json')
         and applied['destination'] == str(PACKAGE), 'overlay binding differs')
    need(not (set(overlay['files']) & set(manifest['files'])), 'overlay collision')
    for name, expected_pin in overlay['files'].items():
        rel = PurePosixPath(name)
        need(not rel.is_absolute() and '..' not in rel.parts and '\\\\' not in name, 'unsafe overlay path')
        need(pin(PACKAGE / rel) == expected_pin, 'overlay changed: ' + name)
    return {'overlay_manifest': pin(PACKAGE / 'overlay-manifest.json'),
            'overlay_installation': pin(PACKAGE / 'overlay-installation.json'),
            'manifest': pin(PACKAGE / 'manifest.json'),''')
    start = once(start, "    parser.add_argument('--launch-attempted-at')", "    parser.add_argument('--launch-attempted-at')\n    parser.add_argument('--highcpu-armed-at')\n    parser.add_argument('--highcpu-stop-deadline-utc')")
    start = once(start, "    if args.phase == 'measure' and args.deadline_utc is None:\n        args.deadline_utc = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(seconds=900)).isoformat()", """    if args.phase == 'measure':
        need(args.highcpu_armed_at and args.highcpu_stop_deadline_utc, 'fixed highCPU phase required')
        armed, high_stop = utc(args.highcpu_armed_at), utc(args.highcpu_stop_deadline_utc)
        need(479 < (high_stop - armed).total_seconds() <= 480, '8 minute phase STOP differs')
        need(armed <= dt.datetime.now(dt.timezone.utc) and high_stop <= utc(args.stop_deadline_utc), 'phase outside lifecycle')
        if args.deadline_utc is None:
            args.deadline_utc = min(dt.datetime.now(dt.timezone.utc) + dt.timedelta(seconds=360), high_stop - dt.timedelta(seconds=20)).isoformat()""")
    start = once(start, "        need(890 < remaining <= 900 and (stop - deadline).total_seconds() >= 900,\n             'measurement requires full15min plus15min recovery')", "        need(260 < remaining <= 360 and deadline <= high_stop - dt.timedelta(seconds=20),\n             'measurement requires >260..360 seconds within fixed highCPU STOP')")
    start = once(start, "else 880", "else 250")
    start = once(start, "str(remaining - 20)", "str(remaining - 10)")
    start = once(start, "args.launch_attempted_at, args.stop_deadline_utc]", "args.launch_attempted_at, args.stop_deadline_utc, args.highcpu_armed_at or '-', args.highcpu_stop_deadline_utc or '-']")
    start = once(start, "'r1.cpu-profile-vm19-deployment/v1'", "'r1.sparse-rank-vm21-deployment/v1'")
    start = once(start, "'launch_attempted_at': args.launch_attempted_at,", "'launch_attempted_at': args.launch_attempted_at,\n              'highcpu_armed_at': args.highcpu_armed_at, 'highcpu_stop_deadline_utc': args.highcpu_stop_deadline_utc,")
    run = inputs['run.sh'].decode().replace('flop-cpu-profile', 'sparse-rank').replace('flop-scaling/cpu-profile', 'flop-scaling/sparse-rank-groups').replace('cloud/vm19', 'cloud/vm21').replace('r1.cpu-profile-vm19-wrapper/v1', 'r1.sparse-rank-vm21-wrapper/v1')
    run = once(run, 'test "$#" -eq 4', 'test "$#" -eq 6')
    run = run.replace('"$package/source-baseline"', '"$package/source"')
    run = once(run, 'export RUSTFLAGS=\'-C target-cpu=x86-64-v3 -C force-frame-pointers=yes -C debuginfo=line-tables-only\'', "export RUSTFLAGS='-C target-cpu=x86-64-v3'")
    run = once(run, '    --perf "/usr/lib/linux-tools/$(uname -r)/perf" --launch-attempted-at', '    --launch-attempted-at')
    run = once(run, 'measure-prepare --out "$proof" --measurement-deadline-utc "$2" \\', 'measure-prepare --out "$proof" --measurement-deadline-utc "$2" \\\n    --highcpu-armed-at "$5" --highcpu-stop-deadline-utc "$6" \\')
    status = inputs['status.py'].decode().replace('VM19', 'VM21').replace('flop-cpu-profile', 'sparse-rank').replace('solvers-r1-vm19-', 'solvers-r1-vm21-')
    outputs = {'start.py': start.encode(), 'run.sh': run.encode(), 'status.py': status.encode()}
    for name, raw in outputs.items():
        with (HERE / name).open('xb') as stream:
            stream.write(raw)
    record = {'source': {n: pin(v) for n, v in inputs.items()}, 'outputs': {n: pin(v) for n, v in outputs.items()},
              'deriver': pin(Path(__file__).read_bytes()), 'cloud_execution': False, 'native_execution': False}
    with (HERE / 'dispatch-derivation.json').open('x') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
    print(json.dumps(record))


if __name__ == '__main__':
    main()
