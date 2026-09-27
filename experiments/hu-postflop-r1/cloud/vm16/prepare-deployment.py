"""Generate baseline-only VM16 installer/start controls; never run a workload."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
ADAPTER = "experiments/hu-postflop-r1/flop-scaling/cpu-occupancy/adapter/solve.rs"
ORIGINAL = "experiments/hu-postflop-r1/flop-scaling/flat-ev/cloud32/solve.rs"


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise ValueError("Derivation anchor differs: " + old[:80])
    return text.replace(old, new, 1)


def save(name, original, text, records):
    data = text.encode()
    with (HERE / name).open("xb") as stream:
        stream.write(data)
    records[name] = {"original_sha256": hashlib.sha256(original).hexdigest(),
                     "generated_sha256": hashlib.sha256(data).hexdigest()}


def main():
    records = {}
    old = (ROOT / "experiments/hu-postflop-r1/flop-scaling/worker-scratch/cloud32/install.py").read_bytes()
    text = old.decode().replace("two fresh source trees", "one fresh baseline source tree")
    text = text.replace("r1-worker-scratch-cloud32-package/v1", "r1-cpu-occupancy-package/v1")
    text = replace_once(text, 'sources != manifest["source_pins"] or pin(files["candidate/solver.rs"]) != manifest["candidate"]',
                        'sources != manifest["source_pins"]')
    begin = text.index('    adapter = destination / ')
    end = text.index('    receipt = ', begin)
    text = text[:begin] + f'''    source = destination / "source-baseline"
    shutil.copytree(destination / "source", source)
    for name, relative in (("flop_cloud32_probe.rs", "{ORIGINAL}"),
                           ("flop_cpu_occupancy_probe.rs", "{ADAPTER}")):
        example = source / "crates/holdem/examples" / name
        if example.exists():
            raise ValueError("Unexpected preexisting adapter")
        example.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(destination / relative, example)
''' + text[end:]
    save("install.py", old, text, records)

    old = (HERE.parent / "vm15/start.py").read_bytes()
    text = old.decode().replace("VM15 comparison", "VM16 diagnostic").replace("vm15", "vm16")
    text = text.replace("flop-worker-cloud32", "flop-cpu-occupancy")
    text = text.replace("solvers-r1-vm16-worker32", "solvers-r1-vm16-cpu32")
    text = text.replace("r1-worker-scratch-cloud32-package/v1", "r1-cpu-occupancy-package/v1")
    text = text.replace("r1.worker-scratch-vm16-deployment/v1", "r1.cpu-occupancy-vm16-deployment/v1")
    begin = text.index('    adapter = pin(')
    end = text.index("    return {'manifest':", begin)
    text = text[:begin] + f'''    source = PACKAGE / 'source-baseline'
    expected = dict(manifest['source_pins'])
    expected['crates/holdem/examples/flop_cloud32_probe.rs'] = pin(PACKAGE / '{ORIGINAL}')
    expected['crates/holdem/examples/flop_cpu_occupancy_probe.rs'] = pin(PACKAGE / '{ADAPTER}')
    actual = {{}}
    for path in sorted(source.rglob('*')):
        need(not path.is_symlink(), 'source symlink')
        if path.is_file():
            actual[path.relative_to(source).as_posix()] = pin(path)
    need(actual == expected, 'baseline source membership/content differs')
''' + text[end:]
    text = text.replace("1220 < remaining <= 2400", "600 < remaining <= 960")
    text = text.replace("experiment must fit40min", "experiment must fit16min")
    text = text.replace("remaining > 1220", "remaining > 600")
    save("start.py", old, text, records)
    with (HERE / "deployment-derivation.json").open("x") as target:
        json.dump(records, target, indent=2)
        target.write("\n")
    print(json.dumps({"generated": list(records), "cloud_or_native_execution": False}))


if __name__ == "__main__":
    main()
