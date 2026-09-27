"""Derive finite VM17 controls from retained VM16 controls; no cloud/native execution."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
OLD = HERE.parent / "vm16"
FUSED = "experiments/hu-postflop-r1/flop-scaling/fused-update"
ADAPTER = "experiments/hu-postflop-r1/flop-scaling/cpu-occupancy/adapter/solve.rs"


def pin(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def main():
    records = {}
    for name in ("recover.sh", "fetch-dependencies.sh", "bootstrap.sh", "run.sh", "start.py", "install.py", "analyze-on-cloud.py", "launch.ps1", "pack.py"):
        original = (OLD / name).read_bytes()
        text = original.decode().replace("VM16", "VM17").replace("vm16", "vm17")
        text = text.replace("flop-cpu-occupancy", "flop-fused-update")
        text = text.replace("r1-20260927-16", "r1-20260927-17")
        text = text.replace("r1-cpu-occupancy-package/v1", "r1-fused-update-package/v1")
        text = text.replace("cpu-occupancy-vm17", "fused-update-vm17")
        text = text.replace("solvers-r1-vm17-cpu32", "solvers-r1-vm17-fused32")
        if name == "run.sh":
            text = text.replace("controls=$package/experiments/hu-postflop-r1/flop-scaling/cpu-occupancy", "controls=$package/" + FUSED + "/timing")
            text = text.replace('--source "$package/source-baseline"', '--baseline-source "$package/source-baseline" --candidate-source "$package/source-candidate"')
            text = text.replace(' --taskset /usr/bin/taskset', '')
        if name == "fetch-dependencies.sh":
            text = text.replace("# Baseline source only. Fetch; native build follows resize.", "# Both arms share Cargo.lock. Fetch only; fresh native builds follow resize.")
        if name == "analyze-on-cloud.py":
            text = text.replace("flop-scaling/cpu-occupancy/analyze.py", "flop-scaling/fused-update/timing/analyze.py")
        if name == "install.py":
            text = text.replace("one fresh baseline source tree", "two fresh source trees")
            begin = text.index('    source = destination / "source-baseline"')
            end = text.index('    receipt = ', begin)
            text = text[:begin] + f'''    for arm in ("baseline", "candidate"):
        source = destination / ("source-" + arm)
        shutil.copytree(destination / "source", source)
        example = source / "crates/holdem/examples/flop_cpu_occupancy_probe.rs"
        if example.exists():
            raise ValueError("Unexpected preexisting adapter")
        example.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(destination / "{ADAPTER}", example)
        if arm == "candidate":
            for relative, artifact in manifest["candidate_overrides"].items():
                target = source / relative
                if relative in manifest["source_pins"]:
                    if pin(target.read_bytes()) != manifest["source_pins"][relative]:
                        raise ValueError("Original override source differs")
                elif target.exists():
                    raise ValueError("Preexisting candidate fixture")
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(destination / artifact, target)
''' + text[end:]
        if name == "start.py":
            begin = text.index("    source = PACKAGE / 'source-baseline'")
            end = text.index("    return {'manifest':", begin)
            text = text[:begin] + f'''    overrides = manifest['candidate_overrides']
    need(set(overrides) == {{'crates/engine/src/solver.rs', 'crates/engine/src/storage.rs',
                            'crates/engine/src/lib.rs', 'crates/engine/tests/fused_update.rs'}}, 'candidate override membership differs')
    for arm in ('baseline', 'candidate'):
        source = PACKAGE / ('source-' + arm)
        expected = dict(manifest['source_pins'])
        expected['crates/holdem/examples/flop_cpu_occupancy_probe.rs'] = pin(PACKAGE / '{ADAPTER}')
        if arm == 'candidate':
            expected.update({{name: pin(PACKAGE / artifact) for name, artifact in overrides.items()}})
        actual = {{}}
        for path in sorted(source.rglob('*')):
            need(not path.is_symlink(), 'source symlink')
            if path.is_file():
                actual[path.relative_to(source).as_posix()] = pin(path)
        need(actual == expected, arm + ' source membership/content differs')
''' + text[end:]
        if name == "pack.py":
            text = text.replace('DIAGNOSTIC = ROOT / "experiments/hu-postflop-r1/flop-scaling/cpu-occupancy"', 'DIAGNOSTIC = ROOT / "' + FUSED + '"')
            text = text.replace('".patch", ".inc")', '".patch", ".inc", ".log")')
            text = text.replace('    shared = [', '    shared = ["' + ADAPTER + '",\n')
            text = text.replace('"control-derivation.json", "deployment-derivation.json",\n                 "prepare-controls.py", "prepare-deployment.py", "analyze-on-cloud.py")', '"control-derivation.json", "prepare-controls.py", "analyze-on-cloud.py")')
            text = text.replace('    manifest = {', f'''    overrides = {{"crates/engine/src/" + name: "{FUSED}/candidate/" + name
                 for name in ("solver.rs", "storage.rs", "lib.rs")}}
    overrides["crates/engine/tests/fused_update.rs"] = "{FUSED}/tests/fused_update.rs"
    for artifact in overrides.values():
        if artifact not in files:
            raise ValueError("Candidate artifact absent")
    manifest = {{''')
            text = text.replace('"source_pins": source_pins, "files":', '"source_pins": source_pins, "candidate_overrides": overrides, "files":')
        if name == "install.py":
            text = text.replace("    return files, manifest\n", '    if manifest.get("candidate_overrides") != {\'crates/engine/src/solver.rs\': \'experiments/hu-postflop-r1/flop-scaling/fused-update/candidate/solver.rs\', \'crates/engine/src/storage.rs\': \'experiments/hu-postflop-r1/flop-scaling/fused-update/candidate/storage.rs\', \'crates/engine/src/lib.rs\': \'experiments/hu-postflop-r1/flop-scaling/fused-update/candidate/lib.rs\', \'crates/engine/tests/fused_update.rs\': \'experiments/hu-postflop-r1/flop-scaling/fused-update/tests/fused_update.rs\'}:\n        raise ValueError("Candidate override paths differ from fixed mapping")\n' + "    return files, manifest\n")
        if name == "start.py":
            text = text.replace("    need(set(overrides) == {'crates/engine/src/solver.rs', 'crates/engine/src/storage.rs',\n                            'crates/engine/src/lib.rs', 'crates/engine/tests/fused_update.rs'}, 'candidate override membership differs')\n", "    need(overrides == {'crates/engine/src/solver.rs': 'experiments/hu-postflop-r1/flop-scaling/fused-update/candidate/solver.rs', 'crates/engine/src/storage.rs': 'experiments/hu-postflop-r1/flop-scaling/fused-update/candidate/storage.rs', 'crates/engine/src/lib.rs': 'experiments/hu-postflop-r1/flop-scaling/fused-update/candidate/lib.rs', 'crates/engine/tests/fused_update.rs': 'experiments/hu-postflop-r1/flop-scaling/fused-update/tests/fused_update.rs'}, 'candidate override mapping differs')\n")
        generated = text.encode()
        with (HERE / name).open("xb") as stream:
            stream.write(generated)
        records[name] = {"original": pin(original), "generated": pin(generated)}
    with (HERE / "control-derivation.json").open("x") as stream:
        json.dump(records, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"generated": list(records), "cloud_or_native_execution": False}))


if __name__ == "__main__":
    main()
