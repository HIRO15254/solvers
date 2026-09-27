"""Derive finite VM19 transport/service controls; no native work or cloud calls."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent


def pin(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def once(text, old, new):
    if text.count(old) != 1:
        raise ValueError("Control anchor differs: " + old)
    return text.replace(old, new, 1)


def main():
    records = {}
    for name in ("install.py", "pack.py", "capture-command.py", "recover.sh", "analyze-on-cloud.py",
                 "start.py", "run.sh", "status.py", "split-on-cloud.py", "check-download.py"):
        source = HERE.parent / "vm18" / name
        raw = source.read_bytes()
        code = raw.decode().replace("VM18", "VM19").replace("vm18", "vm19")
        code = code.replace("flop-chance-grain", "flop-cpu-profile").replace("chance-grain", "cpu-profile")
        code = code.replace("flop_chance_grain_probe", "flop_cpu_profile_probe")
        if name == "start.py":
            code = once(code, "dt.timedelta(seconds=1200)", "dt.timedelta(seconds=900)")
            code = once(code, "(stop - deadline).total_seconds() == 2400", "(stop - deadline).total_seconds() == 1500")
            code = code.replace("launch+20min within original60min STOP", "launch+20min within original45min STOP")
            code = once(code, "1190 < remaining <= 1200", "890 < remaining <= 900")
            code = code.replace("full20min plus15min recovery", "full15min plus15min recovery")
            code = once(code, "480 if args.phase == 'build' else 1180", "480 if args.phase == 'build' else 880")
            old = "    need(instance_id.isdecimal(), 'instance identity missing')\n"
            new = old + """    request = urllib.request.Request('http://metadata.google.internal/computeMetadata/v1/instance/machine-type',
                                     headers={'Metadata-Flavor': 'Google'})
    with opener.open(request, timeout=5) as response:
        need(response.headers.get('Metadata-Flavor') == 'Google', 'machine metadata response differs')
        machine = response.read(1024).decode().strip().rsplit('/', 1)[-1]
    need(machine == ('e2-standard-2' if args.phase == 'build' else 'e2-highcpu-32'), 'E2-only machine differs')
"""
            code = once(code, old, new)
            code = once(code, "'logical_cpus': cpus,", "'machine_type': machine, 'logical_cpus': cpus,")
        elif name == "run.sh":
            code = once(code, "-C target-cpu=x86-64-v3'", "-C target-cpu=x86-64-v3 -C force-frame-pointers=yes -C debuginfo=line-tables-only'")
        elif name == "pack.py":
            code = once(code, "from pathlib import Path", "from pathlib import Path, PurePosixPath")
            code = once(code, "sorted(baseline.items())", "sorted(baseline.items(), key=lambda item: PurePosixPath(item[0]).parts)")
            code = once(code, '".inc", ".log", ".txt")', '".inc", ".in", ".log", ".txt")')
        elif name == "recover.sh":
            code = once(code, 'cp /opt/r1/flop-cpu-profile-build-start01.json /opt/r1/bootstrap-complete "$meta/"',
                        'cp /opt/r1/bootstrap-complete "$meta/"\nif test -f /opt/r1/flop-cpu-profile-build-start01.json; then cp /opt/r1/flop-cpu-profile-build-start01.json "$meta/"; fi')
        encoded = code.encode()
        with (HERE / name).open("xb") as stream:
            stream.write(encoded)
        records[name] = {"original": {"path": "../vm18/" + name, **pin(raw)}, "generated": pin(encoded)}
    with (HERE / "control-derivation.json").open("x") as stream:
        json.dump(records, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"generated": list(records), "native_or_cloud_execution": False}))


if __name__ == "__main__":
    main()
