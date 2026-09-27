"""Derive VM18 package/recovery helpers; does not contact GCP or run native code."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CHANCE = "experiments/hu-postflop-r1/flop-scaling/chance-grain"


def pin(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def main():
    records = {}
    for name in ("install.py", "pack.py", "capture-command.py", "bootstrap.sh", "recover.sh", "analyze-on-cloud.py"):
        source = HERE.parent / ("vm16" if name in ("install.py", "pack.py") else "vm17") / name
        raw = source.read_bytes()
        code = raw.decode().replace("VM16", "VM18").replace("vm16", "vm18").replace("VM17", "VM18").replace("vm17", "vm18")
        code = code.replace("flop-cpu-occupancy", "flop-chance-grain").replace("flop-fused-update", "flop-chance-grain")
        code = code.replace("r1-cpu-occupancy-package/v1", "r1-chance-grain-package/v1")
        code = code.replace("solvers-r1-vm18-fused32", "solvers-r1-vm18-measure32")
        code = code.replace("fused-update-vm18", "chance-grain-vm18")
        if name == "install.py":
            begin = code.index('    for name, relative in (("flop_cloud32_probe.rs"')
            end = code.index('    receipt = ', begin)
            code = code[:begin] + f'''    example = source / "crates/holdem/examples/flop_chance_grain_probe.rs"
    if example.exists():
        raise ValueError("Unexpected preexisting adapter")
    example.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(destination / "{CHANCE}/adapter/solve.rs", example)
''' + code[end:]
        elif name == "pack.py":
            code = code.replace('flop-scaling/cpu-occupancy"', 'flop-scaling/chance-grain"')
            code = code.replace('".patch", ".inc")', '".patch", ".inc", ".log", ".txt")')
            begin = code.index('    for name in ("start.py"')
            end = code.index('    manifest = ', begin)
            code = code[:begin] + '''    for name in ("start.py", "run.sh", "recover.sh", "bootstrap.sh", "install.py", "pack.py",
                 "capture-command.py", "README.md", "control-derivation.json", "prepare-controls.py", "analyze-on-cloud.py"):
        path = HERE / name
        add(path.relative_to(ROOT).as_posix(), path)
''' + code[end:]
        elif name == "recover.sh":
            code = code.replace('/tmp/flop-chance-grain', '/opt/r1/flop-chance-grain')
            code = code.replace('cp /opt/r1/flop-chance-grain-start01.json /opt/r1/bootstrap-complete "$meta/"', '''cp /opt/r1/flop-chance-grain-build-start01.json /opt/r1/bootstrap-complete "$meta/"
if test -f /opt/r1/flop-chance-grain-measure-start01.json; then cp /opt/r1/flop-chance-grain-measure-start01.json "$meta/"; fi
systemctl show solvers-r1-vm18-build2 > "$meta/build-service.log"
journalctl -u solvers-r1-vm18-build2 --no-pager > "$meta/build-journal.log"''')
            code = code.replace("('recovery/wrapper',Path('/opt/r1/flop-chance-grain-wrapper01'))", "('recovery/build-wrapper',Path('/opt/r1/flop-chance-grain-build-wrapper01')),\n               ('recovery/measure-wrapper',Path('/opt/r1/flop-chance-grain-measure-wrapper01'))")
            code = code.replace('test ! -e "$archive"', '''case "$(systemctl show solvers-r1-vm18-build2 --property=ActiveState --value)" in inactive|failed) ;; *) exit 2 ;; esac
test "$(systemctl show solvers-r1-vm18-build2 --property=MainPID --value)" = 0
test ! -e "$archive"''', 1)
        elif name == "analyze-on-cloud.py":
            code = code.replace('flop-scaling/fused-update/timing/analyze.py', 'flop-scaling/chance-grain/analyze.py')
            code = code.replace('/tmp/flop-chance-grain', '/opt/r1/flop-chance-grain')
        generated = code.encode()
        with (HERE / name).open("xb") as stream:
            stream.write(generated)
        records[name] = {"original_path": str(source.relative_to(HERE.parents[3])), "original": pin(raw), "generated": pin(generated)}
    with (HERE / "control-derivation.json").open("x") as stream:
        json.dump(records, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"generated": list(records), "cloud_or_native_execution": False}))


if __name__ == "__main__":
    main()
