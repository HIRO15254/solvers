"""Derive VM16 recovery controls from retained VM15 controls, without execution."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
PREVIOUS = HERE.parent / "vm15"


def pin(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def main():
    records = {}
    for name in ("capture-command.py", "recover.sh", "fetch-dependencies.sh"):
        original = (PREVIOUS / name).read_bytes()
        text = original.decode().replace("VM15", "VM16").replace("vm15", "vm16")
        text = text.replace("flop-worker-cloud32", "flop-cpu-occupancy")
        text = text.replace("worker-scratch-vm16", "cpu-occupancy-vm16")
        text = text.replace("solvers-r1-vm16-worker32", "solvers-r1-vm16-cpu32")
        if name == "recover.sh":
            text = text.replace('cp /opt/r1/flop-cpu-occupancy-start01.json /opt/r1/bootstrap-complete "$meta/"', 'cp /opt/r1/flop-cpu-occupancy-start01.json /opt/r1/bootstrap-complete "$meta/"\nfor suffix in json receipt.json stdout.log stderr.log; do\n  result=/tmp/flop-cpu-occupancy-analysis01.$suffix\n  if test -f "$result"; then cp "$result" "$meta/"; fi\ndone')
            # Preserve >=256 MiB allowance for SDK and other outgoing traffic.
            text = text.replace("limit=1024**3-len(encoded)-256", "limit=256*1024**2-len(encoded)-256")
            text = text.replace("exceed1GiB", "exceed256MiB")
            text = text.replace("len(side)>1024**3", "len(side)>256*1024**2")
            text = text.replace("Publication exceeds1GiB", "Publication exceeds256MiB")
        if name == "fetch-dependencies.sh":
            text = text.replace("# Both arms have the identical lockfile. Fetch only; native builds follow resize.",
                                "# Baseline source only. Fetch; native build follows resize.")
        generated = text.encode()
        with (HERE / name).open("xb") as target:
            target.write(generated)
        records[name] = {"original": pin(original), "generated": pin(generated)}
    with (HERE / "control-derivation.json").open("x") as target:
        json.dump(records, target, indent=2)
        target.write("\n")
    print(json.dumps({"generated": list(records), "cloud_or_native_execution": False}))


if __name__ == "__main__":
    main()
