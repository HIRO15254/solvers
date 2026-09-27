"""Derive a bounded worker-scratch experiment from the retained Cloud32 runner.

No build, solver execution, network operation, or cloud reservation occurs here.
"""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
OLD = HERE.parent / "flat-ev/cloud32"
OUT = HERE / "cloud32"
BASE = "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a"
OLD_CANDIDATE = "ec9e5e6b5230684fce6f2315370ceda0e05fb4346eb7e4ff44ea05e6028aabcd"


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def replace(text, old, new, count=1):
    if text.count(old) != count:
        raise ValueError(f"Anchor count differs: {old[:100]!r}")
    return text.replace(old, new)


def main():
    OUT.mkdir(exist_ok=True)
    candidate = pin((HERE / "solver.rs").read_bytes())["sha256"]
    sources = {}
    outputs = {}
    for name in ("runner.py", "install.py", "analyze.py"):
        raw = (OLD / name).read_bytes()
        sources[name] = pin(raw)
        text = raw.decode().replace("flat-ev", "worker-scratch").replace("flat", "worker").replace("vm14", "vm15")
        text = text.replace(OLD_CANDIDATE, candidate)
        if name != "install.py":
            text = replace(text, "WORKERS = (1, 2, 4, 8, 16, 32)", "WORKERS = (1, 4, 16, 32)")
        if name == "runner.py":
            text = replace(text, 'COMMON = HERE.parent / "timing/run.py"',
                           'COMMON = ROOT / "experiments/hu-postflop-r1/flop-scaling/flat-ev/timing/run.py"')
            text = replace(text, 'GENERIC = HERE.parent / "generic-checks/execution-checks01/verification.json"',
                           'DURABLE = HERE.parent / "durable.py"\nspec = importlib.util.spec_from_file_location("worker_scratch_durable", DURABLE)\ndurable = importlib.util.module_from_spec(spec)\nspec.loader.exec_module(durable)\nsave = durable.atomic_json')
            text = replace(text, '    generic = read(GENERIC)\n    need(generic["status"] == "passed" and generic["source_unchanged"] and generic["candidate_solver"]["sha256"] == SOLVER_PINS["worker"], "Windows generic prerequisite differs")\n', '')
            text = replace(text, 'HERE / "solve.rs", GENERIC,', 'HERE / "solve.rs", DURABLE,')
            text = replace(text, '    save(out / "plan.json", plan)',
                           '    sync_tree(out)\n    durable.sync_directory(out.parent)\n    save(out / "plan.json", plan)')
            text = replace(text, '"matrix_processes": 96', '"matrix_processes": 64')
            start = text.index('def gzip_verified(')
            end = text.index('\ndef retain_state(', start)
            text = text[:start] + '''def sync_tree(root):
    for path in sorted(root.rglob("*")):
        need(not path.is_symlink(), "durable proof symlink")
        if path.is_file():
            durable.sync_file(path)
    for path in sorted((p for p in root.rglob("*") if p.is_dir()), key=lambda p: len(p.parts), reverse=True):
        durable.sync_directory(path)
    durable.sync_directory(root)


def gzip_verified(source, destination, expected):
    return durable.gzip_verified(source, destination, pair(expected))

''' + text[end:]
            text = replace(text, '        os.replace(state, raw)',
                           '        os.replace(state, raw)\n        durable.sync_directory(raw.parent)\n        durable.sync_directory(state.parent)')
            text = replace(text, '        state.unlink()', '        state.unlink()\n        durable.sync_directory(state.parent)')
            text = replace(text, '    save(path, receipt)\n    print(json.dumps({"stage":',
                           '    stage_root = path.parent / item["name"]\n    sync_tree(stage_root)\n    durable.sync_directory(stage_root.parent)\n    save(stage_root / "completed.json", item, once=True)\n    save(path, receipt)\n    print(json.dumps({"stage":')
            text = replace(text, '    retained = retain_state(',
                           '    sync_tree(directory)\n    durable.sync_directory(directory.parent)\n    retained = retain_state(')
            text = replace(text, '            + [{"name": f"smoke-narrow-',
                           '            + [{"name": "tests-worker", "arm": "worker", "kind": "tests", "status": "pending"}]\n            + [{"name": f"smoke-narrow-')
            text = replace(text, '                else:\n                    arm = current["arm"]',
                           '''                elif kind == "tests":
                    current["command"] = [plan["tools"]["cargo"]["path"], "test", "--locked", "--offline", "--release", "-j2",
                                          "--target-dir", str(Path(plan["workspace"]) / "worker-target"),
                                          "-p", "engine", "-p", "holdem", "-p", "cfr-ref", "--tests"]
                    stage(out, plan, current, receipt, destination, 300, Path(plan["sources"]["worker"]["path"]))
                else:
                    arm = current["arm"]''')
            text = replace(text, 'len(built["stages"]) == 7', 'len(built["stages"]) == 8')
            text = replace(text, '                solve(out, plan, binaries, current, receipt, destination, selected["canonical"])',
                           '''                solve(out, plan, binaries, current, receipt, destination, selected["canonical"])
                case_rows = [r for r in rows if r["case"] == current["case"]]
                if all(r["status"] == "completed" for r in case_rows):
                    checkpoint_case(out, plan, receipt, current["case"], case_rows)''')
            text = replace(text, 'len(rows) == 96', 'len(rows) == 64')
            # Kept separate from mutable whole-run manifests. The reader checks
            # this exact schedule before accepting a completed case.
            marker = '\ndef finish_manifest(out):'
            addition = '''
def checkpoint_case(out, plan, receipt, case, rows):
    need(len(rows) == 32 and all(r["status"] == "completed" for r in rows), "case incomplete")
    immutable = []
    for path in sorted(out.rglob("*")):
        if path.is_file() and path.name not in {"plan.json", "build.json", "execution.json", "retained.json"} and "checkpoints" not in path.relative_to(out).parts:
            immutable.append(durable.file_ref(path))
    checkpoint = out / "checkpoints"
    checkpoint.mkdir(exist_ok=True)
    durable.sync_directory(out)
    durable.publish_case(checkpoint / (case + ".json"), case=case,
                         boot_id=plan["host"]["boot_id"],
                         plan_ref=durable.file_ref(out / "plan.json"),
                         build_ref=durable.file_ref(out / "build.json"),
                         files=immutable, stages=rows)

'''
            text = replace(text, marker, '\n' + addition + marker)
        elif name == "analyze.py":
            text = replace(text, 'HERE.parent / "timing/run.py"',
                           'ROOT / "experiments/hu-postflop-r1/flop-scaling/flat-ev/timing/run.py"')
            text = replace(text, 'HERE.parent.parent / "ev-scratch/run.py", ROOT / "tools/run_supervised.py"):',
                           'HERE.parent.parent / "ev-scratch/run.py", ROOT / "tools/run_supervised.py", HERE.parent / "durable.py"):')
            text = replace(text, 'need(len(rows) == 96, "expected 96 matrix rows")',
                           'need(len(rows) == 64, "expected 64 matrix rows")')
            text = replace(text, '== 24, "expected 24 warmup and 72 measured rows"',
                           '== 16, "expected 16 warmup and 48 measured rows"')
            start = text.index('    _, _, generic_path =')
            end = text.index('\n', text.index('"generic prerequisite differs")', start)) + 1
            text = text[:start] + text[end:]
            text = replace(text, '"both_inputs_eight_workers_time_at_most_0_90"', '"both_inputs_sixteen_workers_time_at_most_0_90"')
            text = replace(text, 'if v["workers"] == 8)', 'if v["workers"] == 16)')
            text = replace(text, '"build-worker", "smoke-narrow-', '"build-worker", "tests-worker", "smoke-narrow-')
            text = replace(text, 'build["stages"][3:]', 'build["stages"][4:]')
            text = replace(text, 'item.get("kind") == "build"', 'item.get("kind") in {"build", "tests"}', 2)
            text = replace(text, 'build["stages"][:3]', 'build["stages"][:4]')
            anchor = '    need("release: 1.97.0" in build["rustc_version"]'
            insertion = '''    test_item = build["stages"][3]
    need(test_item["kind"] == "tests" and test_item["arm"] == "worker", "candidate tests missing")
    need(test_item["command"] == [plan["tools"]["cargo"]["path"], "test", "--locked", "--offline", "--release", "-j2",
                                  "--target-dir", str(PurePosixPath(plan["workspace"]) / "worker-target"),
                                  "-p", "engine", "-p", "holdem", "-p", "cfr-ref", "--tests"], "candidate test command differs")
'''
            text = replace(text, anchor, insertion + anchor)
            text = replace(text, '"matrix_processes": 96, "warmup": 24, "measured": 72',
                           '"matrix_processes": 64, "warmup": 16, "measured": 48')
        outputs[name] = text.encode()
    for name in ("solve.rs", "prepare.py", "provenance.json", "adapter.patch"):
        outputs[name] = (OLD / name).read_bytes()
        sources[name] = pin(outputs[name])
    for name, data in outputs.items():
        (OUT / name).write_bytes(data)
    record = {"schema": "r1.worker-scratch-cloud-derivation/v1", "baseline": BASE,
              "candidate": candidate, "sources": sources, "outputs": {n: pin(d) for n, d in outputs.items()},
              "scope": "Generated only; no compile, test, solve or cloud operations"}
    (OUT / "derivation.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(record))


if __name__ == "__main__":
    main()
