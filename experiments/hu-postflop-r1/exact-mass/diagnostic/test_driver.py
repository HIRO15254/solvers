"""Small synthetic supervisor/portable-check tests. No build or solver."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock

HERE = Path(__file__).resolve().parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


driver = load("diagnostic_driver_tests", HERE / "run_diagnostic.py")
fixtures = load("original_proof_fixture", HERE.parent / "test_run.py")


def counters():
    rows = [[0] * 10 for _ in range(7)]
    rows[0] = [4, 40, 30, 1, 2, 1, 1, 0, 2, 1]
    return {"schema": "r1.mass-gate-diagnostic/v1", "callers": driver.instrument.CALLERS,
            "fields": driver.instrument.FIELDS, "counts": rows}


class Tests(unittest.TestCase):
    def test_real_candidate_source_patch_and_portable_archive_binding(self):
        archive = Path(os.environ.get("R1_CANDIDATE_ARCHIVE", r"E:\codex-work\solvers\r1-exact-new01\source-candidate.tar.gz"))
        manifest = Path(os.environ.get("R1_CANDIDATE_MANIFEST", HERE.parent / "source-new01/source-candidate-manifest.json"))
        with tempfile.TemporaryDirectory(prefix="diagnostic-source-") as directory:
            root = Path(directory) / "copy"
            driver.instrument.instrument(archive, manifest, root)
            out = root / "run"
            out.mkdir()
            store = driver.Store(out, create=True)
            source_archive = root / "instrumented-source.tar.gz"
            rows = driver.read(root / "instrumented-source-manifest.json")["files"]
            with tarfile.open(source_archive, "w:gz") as tar:
                for row in rows:
                    tar.add(root / "source" / row["path"], arcname=row["path"], recursive=False)
            plan = {"candidate_archive": store.add(archive), "candidate_manifest": store.add(manifest),
                    "source_archive": store.add(source_archive), "source_manifest": store.add(root / "instrumented-source-manifest.json"),
                    "patch": store.add(root / "instrumentation.patch"), "provenance": store.add(root / "provenance.json"),
                    "controls": {"instrument.py": store.add(HERE / "instrument.py")}}
            self.assertEqual(len(driver.source_bytes(store, plan)), 368)
            bad_patch = root / "bad.patch"
            bad_patch.write_bytes(b"not the pinned patch")
            plan["patch"] = store.add(bad_patch)
            with self.assertRaisesRegex(ValueError, "patch bytes differ"):
                driver.source_bytes(store, plan)

    def test_counter_identities_and_rejections(self):
        report = {"mass_diagnostics": counters()}
        self.assertEqual(driver.diagnostic_counts(report), report["mass_diagnostics"])
        for index in (0, 8, 9):
            bad = copy.deepcopy(report)
            bad["mass_diagnostics"]["counts"][0][index] += 99
            with self.assertRaisesRegex(ValueError, "identities"):
                driver.diagnostic_counts(bad)
        bad = copy.deepcopy(report)
        bad["mass_diagnostics"]["counts"][0][0] = True
        with self.assertRaisesRegex(ValueError, "invalid"):
            driver.diagnostic_counts(bad)

    def test_supervised_commands_keep_all_frozen_targets(self):
        plan = {"protocol": driver.read(HERE.parent / "protocol.json"), "tools": {"cargo": {"path": "/cargo"}, "rustc": {"path": "/rustc"}},
                "target": "/fresh-target", "binary_path": "/fresh-target/release/examples/hu_scaling_bench", "output": "/diagnostic",
                "inputs": {name: {"path": "/input/" + name} for name in ("river", "turn", "flop", "narrow-river")}}
        self.assertEqual(driver.stage_command(plan, "toolchain"), ["/rustc", "-Vv"])
        self.assertEqual(driver.stage_command(plan, "release-example"), ["/cargo", "build", "--release", "-p", "cli", "--example", "hu_scaling_bench", "--target-dir", "/fresh-target"])
        for case, definition in plan["protocol"]["cases"].items():
            argv = driver.stage_command(plan, case)
            for flag, value in (("--iterations", definition["iterations"]), ("--target-nash-conv", definition["target_nash_conv"]), ("--check-every", definition["check_every"]), ("--threads", 1)):
                self.assertEqual(argv[argv.index(flag) + 1], str(value))

    def make_proof(self, out):
        p = fixtures.Proof(out)
        baseline_plan, baseline_state = copy.deepcopy(p.plan), copy.deepcopy(p.state)
        binary = p.put("/diagnostic-target/release/examples/hu_scaling_bench", b"instrumented binary")
        validation = p.store.json(p.plan["arms"]["new"]["validation"]["path"])
        tools = {name: validation["tools"][name] for name in ("cargo", "rustc", "rustdoc")}
        tools["python"] = p.python
        controls = {name: p.put(str(path.resolve()), path.read_bytes()) for name, path in
                    (("driver", HERE / "run_diagnostic.py"), ("instrument.py", HERE / "instrument.py"), ("exact-run.py", driver.CORE), ("showdown-run.py", driver.SHARED))}
        arm = p.plan["arms"]["new"]
        for pin in p.plan["inputs"].values():
            p.put(pin["path"], p.store.data(pin["path"]))
        plan = {"schema": "r1.mass-gate-diagnostic-plan/v1", "source": arm["source"], "output": "/diagnostic", "target": "/diagnostic-target",
                "deadline_utc": p.plan["deadline_utc"], "host": p.host, "tools": tools, "controls": controls,
                "protocol": p.protocol, "protocol_pin": p.plan["controls"]["protocol.json"],
                "candidate_manifest": arm["manifest"], "candidate_archive": arm["archive"],
                "source_manifest": arm["manifest"], "source_archive": arm["archive"],
                "patch": p.put("/instrument.patch", b"synthetic patch"), "provenance": p.put("/provenance.json", b"{}"),
                "supervisor": p.plan["supervisor"], "baseline_plan": p.put("/baseline/plan.json", fixtures.encoded(baseline_plan)),
                "baseline_result": p.put("/baseline/result.json", fixtures.encoded(baseline_state)), "references": {},
                "inputs": p.plan["inputs"], "binary_path": binary["path"]}
        state = {"schema": "r1.mass-gate-diagnostic-result/v1", "status": "completed", "same_candidate_outputs_exact": True, "binary": binary, "stages": []}
        for label, limit in [("toolchain", 30), ("release-example", 1200), *[(case, 300) for case in p.protocol["cases"]]]:
            case_stage = label in p.protocol["cases"]
            pins = driver.fixed_pins(plan) + ([binary, plan["inputs"][label]] if case_stage else [])
            if not case_stage:
                stdout = b"release: 1.97.0\nhost: x86_64-unknown-linux-gnu\n" if label == "toolchain" else b"compiled\n"
                record_pin = p.record("/diagnostic/stages/" + label + "/supervisor", driver.stage_command(plan, label), plan["source"], pins, stdout, limit)
            else:
                entry = next(row for row in baseline_state["stages"] if row["stage"]["label"] == label + "-b1-new")
                old_dir = "/run/stages/" + label + "-b1-new/bench/"
                new_dir = "/diagnostic/stages/" + label + "/bench/"
                report = p.store.json(old_dir + "result.json")
                plan["references"][label] = {"report": p.store.pin(old_dir + "result.json"), "sample": entry["sample"]}
                report["mass_diagnostics"] = counters()
                p.put(new_dir + "result.json", fixtures.encoded(report))
                for name in ("canonical.bin", "state.bin", "config.original.toml", "config.normalized.toml"):
                    p.put(new_dir + name, p.store.data(old_dir + name))
                old_record = p.store.json(entry["record"]["path"])
                record_pin = p.record("/diagnostic/stages/" + label + "/supervisor", driver.stage_command(plan, label), plan["source"], pins,
                                      fixtures.encoded(report), limit, p.store.data(old_record["outputs"]["stderr"]["path"]))
            row = {"label": label, "status": "passed", "timeout_seconds": limit, "record": record_pin,
                   "identity_pins": pins, "source_after_verified": True, "host_after": p.host}
            if case_stage:
                row["diagnostics"] = counters()
            state["stages"].append(row)
        return p, plan, state

    def persist(self, p, plan, state):
        p.put("/diagnostic/plan.json", fixtures.encoded(plan))
        p.store.flush()
        (p.out / "plan.json").write_bytes(fixtures.encoded(plan))
        (p.out / "result.json").write_bytes(fixtures.encoded(state))

    def test_portable_helper_integration_and_baseline_counter_identity_tampering(self):
        with tempfile.TemporaryDirectory(prefix="diagnostic-proof-") as directory:
            p, plan, state = self.make_proof(Path(directory))
            self.persist(p, plan, state)
            # Archive/patch binding has separate real-candidate tests. Here the
            # source layer is mocked explicitly to exercise supervisor/sample
            # schema composition without creating another historical archive.
            with mock.patch.object(driver, "source_bytes", return_value={"tools/run_supervised.py": b"supervisor"}):
                self.assertEqual(driver.check(p.out)["cases_exact"], 4)
                bad = copy.deepcopy(plan)
                bad["references"]["river"]["sample"]["iterations"] += 1
                self.persist(p, bad, state)
                with self.assertRaisesRegex(ValueError, "baseline sample binding"):
                    driver.check(p.out)
                bad_state = copy.deepcopy(state)
                bad_state["stages"][-1]["diagnostics"]["counts"][0][0] += 1
                self.persist(p, plan, bad_state)
                with self.assertRaisesRegex(ValueError, "saved counters"):
                    driver.check(p.out)
                bad_state = copy.deepcopy(state)
                bad_state["stages"][-1]["identity_pins"] = []
                self.persist(p, plan, bad_state)
                with self.assertRaisesRegex(ValueError, "identity list"):
                    driver.check(p.out)


if __name__ == "__main__":
    unittest.main(verbosity=2)
