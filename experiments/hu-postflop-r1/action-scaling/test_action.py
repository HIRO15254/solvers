"""Small synthetic evidence tests; no solver, Cargo, cloud or performance run."""
import datetime as dt
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

HERE = Path(__file__).resolve().parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


run = load("action_test_runner", HERE / "run.py")
checker = load("action_test_checker", HERE / "verify-retained.py")
fixtures = load("range_evidence_fixtures", HERE.parent / "range-scaling/test_retained.py")


def archive_dependencies(store, original, validation):
    """Put trusted helpers in the synthetic source archive, as on the real VM."""
    files = {name: store.raw(run.join(original["source"], name)) for name in original["source_files"]}
    for path in run.dependencies().values():
        files[path.relative_to(HERE.parents[2]).as_posix()] = path.read_bytes()
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
            store.put(run.join(original["source"], name), data)
    archive_pin = store.put(validation["source_archive"]["path"], buffer.getvalue())
    manifest = store.read(validation["source_manifest"]["path"])
    original["source_files"] = {name: run.portable.retention.fingerprint(io.BytesIO(data)) for name, data in files.items()}
    manifest.update(archive_bytes=archive_pin["bytes"], archive_sha256=archive_pin["sha256"],
                    files=[{"path": name, **pin} for name, pin in original["source_files"].items()])
    manifest_pin = store.put(validation["source_manifest"]["path"], manifest)
    validation.update(source_manifest=manifest_pin, source_archive=archive_pin)
    replacements = {pin["path"]: pin for pin in (manifest_pin, archive_pin)}
    for stage in validation["stages"]:
        record = store.read(stage["record"]["path"])
        for field in ("identity_before", "identity_after"):
            record[field] = [replacements.get(pin["path"], pin) for pin in record[field]]
        stage["record"] = store.put(stage["record"]["path"], record)
    original["pins"]["manifest"] = manifest_pin
    plan_pin = store.put("/offline/run/plan.json", original)
    frozen = store.read("/offline/run/frozen.json")
    frozen["plan"] = plan_pin
    store.put("/offline/run/frozen.json", frozen)


def proof(*, helpers_in_archive=False):
    store, build_result_path = fixtures.validation_proof("release-build-only")
    original = store.read("/offline/run/plan.json")
    protocol = run.helper.read(HERE / "protocol.json")
    validation = store.read(build_result_path)
    if helpers_in_archive:
        archive_dependencies(store, original, validation)
    validation.update(boot_id=original["host"]["boot_id"])
    validation["cgroup"]["allowed_cpus"] = list(range(32))
    store.put(build_result_path, validation)
    pins = {name: store.put(run.join(original["source"], path.relative_to(HERE.parents[2]).as_posix())
                            if helpers_in_archive else "/offline/scripts/" + name, path.read_bytes())
            for name, path in run.dependencies().items()}
    pins.update(python=original["pins"]["python"], baseline_frozen=store.identity("/offline/run/frozen.json"),
                baseline_plan=store.identity("/offline/run/plan.json"), input=original["inputs"]["river"])
    pins.update(baseline_binary=original["pins"]["binary"], baseline_manifest=original["pins"]["manifest"],
                baseline_archive=validation["source_archive"], baseline_input=original["inputs"]["river"])
    for role in run.ROLES:
        pins.update({role + "_" + name: pin for name, pin in {
            "manifest": validation["source_manifest"], "archive": validation["source_archive"],
            "compiled_binary": validation["binary"], "build_result": store.identity(build_result_path),
            "build_record": validation["stages"][-1]["record"], "supervisor": original["pins"]["supervisor"],
            "binary": store.put("/offline/action/" + role + "-hu_scaling_bench", store.raw(validation["binary"]["path"]))}.items()})
    containment = {"effective_memory_max_bytes": 12 * 1024**3, "effective_cpu_quota": None, "ancestors": [{"memory.swap.max": "0"}]}
    plan = {"schema": "r1.action-scaling-plan/v1", "output": "/offline/action", "protocol": protocol,
            "host": original["host"], "initial_containment": containment, "deadline_utc": protocol["deadline_utc_latest"],
            "sources": {role: {"root": original["source"]} for role in run.ROLES}, "pins": pins}
    plan_pin = store.put(plan["output"] + "/plan.json", plan)
    example = store.read("/offline/run/stages/river-b0-compact-1/bench/result.json")
    state = {"schema": "r1.action-scaling-result/v1", "status": "completed", "stages": [], "skipped": []}
    report_plan = run.sample_plan(plan)
    for index, stage in enumerate(run.stages(protocol)):
        report = json.loads(json.dumps(example))
        report.update(threads=stage["threads"], iterations=1000)
        seconds = 10 / stage["threads"] * (0.8 if stage["role"] == "new" and stage["threads"] > 1 else 1)
        report["timing"]["run_seconds"] = seconds
        directory = run.join(plan["output"], "stages", stage["label"])
        for name in ("canonical.bin", "state.bin"):
            store.put(run.join(directory, "bench", name), store.raw("/offline/run/stages/river-b0-compact-1/bench/" + name))
        store.put(run.join(directory, "bench", "result.json"), report)
        store.put(run.join(directory, "bench", "config.original.toml"), store.raw(pins["input"]["path"]))
        events = [{"event": "phase", "phase": phase, "status": status, "unix_ms": offset}
                  for phase in run.helper.PHASES for status, offset in (("started", 1000), ("completed", 1100))]
        path = run.join(directory, "supervisor.json")
        _, record = fixtures.record(store, path, run.command(plan, stage), plan["output"], [*pins.values(), plan_pin],
                                    json.dumps(report).encode(), "\n".join(map(json.dumps, events)).encode())
        start = dt.datetime(2026, 9, 26, 3, tzinfo=dt.timezone.utc) + dt.timedelta(seconds=index * 30)
        record.update(started_at=start.isoformat(), ended_at=(start + dt.timedelta(seconds=20)).isoformat())
        record_pin = store.put(path, record)
        with run.portable.portable_runner(store, report_plan) as VPath:
            sample = run.helper.sample_report(report_plan, stage, VPath(directory), record)
        state["stages"].append({"stage": stage, "status": "passed", "supervisor_exit": 0, "record": record_pin,
                                "host_before": plan["host"], "host_after": plan["host"], "containment_before": containment,
                                "containment_after": containment, "sample": sample})
    state["summary"] = run.summarize(protocol, state["stages"])
    store.put(run.join(plan["output"], "result.json"), state)
    return store, plan


def bind_baseline(store, plan, baseline):
    pins = plan["pins"]
    pins["baseline_plan"] = store.put(pins["baseline_plan"]["path"], baseline)
    frozen = store.read(pins["baseline_frozen"]["path"])
    frozen["plan"] = pins["baseline_plan"]
    pins["baseline_frozen"] = store.put(pins["baseline_frozen"]["path"], frozen)
    pins.update(baseline_binary=baseline["pins"]["binary"], baseline_manifest=baseline["pins"]["manifest"],
                baseline_input=baseline["inputs"]["river"])
    pins["baseline_archive"] = store.identity(run.join(str(run.PurePosixPath(pins["baseline_manifest"]["path"]).parent),
                                                      "source-candidate.tar.gz"))


def historical_baseline(store, plan):
    baseline = store.read(plan["pins"]["baseline_plan"]["path"])
    baseline["host"]["boot_id"] = "previous-terminated-boot"
    baseline["pins"]["binary"] = store.put("/offline/history/hu_scaling_bench", b"different historical build bytes")
    baseline["pins"]["manifest"] = store.put("/offline/history/source-candidate-manifest.json",
                                             store.raw(plan["pins"]["old_manifest"]["path"]))
    store.put("/offline/history/source-candidate.tar.gz", store.raw(plan["pins"]["old_archive"]["path"]))
    baseline["inputs"]["river"] = store.put("/offline/history/river.toml", store.raw(plan["pins"]["input"]["path"]))
    bind_baseline(store, plan, baseline)
    return baseline


class ActionTests(unittest.TestCase):
    def test_previous_boot_and_different_baseline_binary_are_accepted(self):
        store, plan = proof()
        baseline = historical_baseline(store, plan)
        self.assertNotEqual(baseline["host"]["boot_id"], plan["host"]["boot_id"])
        self.assertNotEqual(run.content(plan["pins"]["baseline_binary"]), run.content(plan["pins"]["old_binary"]))
        run.check_pins(store, plan)

    def test_historical_source_archive_and_config_must_match(self):
        for field in ("manifest", "archive", "input"):
            with self.subTest(field=field):
                store, plan = proof()
                baseline = historical_baseline(store, plan)
                pin = plan["pins"]["baseline_" + field]
                if field == "manifest":
                    manifest = store.read(pin["path"])
                    manifest["base_commit"] = "d" * 40
                    baseline["pins"]["manifest"] = store.put(pin["path"], manifest)
                elif field == "archive":
                    changed = store.put(pin["path"], store.raw(pin["path"]) + b"changed archive bytes")
                    manifest = store.read(baseline["pins"]["manifest"]["path"])
                    manifest.update(archive_bytes=changed["bytes"], archive_sha256=changed["sha256"])
                    baseline["pins"]["manifest"] = store.put(baseline["pins"]["manifest"]["path"], manifest)
                elif field == "input":
                    baseline["inputs"]["river"] = store.put(pin["path"], store.raw(pin["path"]) + b"\n# changed config bytes")
                bind_baseline(store, plan, baseline)
                with self.assertRaisesRegex(ValueError, "baseline.*(source|archive|input|config)|original.*(source|archive|input|config)"):
                    run.check_pins(store, plan)

    def test_live_store_adapter_does_not_recurse(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "raw.bin"
            path.write_bytes(b"live backend bytes")
            store = run.LiveStore()
            pin = store.identity(path)
            with run.portable.portable_runner(store, {"pins": {"python": {"path": "/unused-python"}}}):
                self.assertEqual(run.helper.identity(path), pin)
                run.helper.verify(pin)

    def test_schedule_has_48_and_balanced_adjacent_pairs(self):
        schedule = run.stages(run.helper.read(HERE / "protocol.json"))
        self.assertEqual(len(schedule), 48)
        self.assertEqual(sum(row["warmup"] for row in schedule), 12)
        first_roles = []
        for index in range(0, 48, 2):
            first, second = schedule[index:index + 2]
            self.assertEqual(first["threads"], second["threads"])
            self.assertEqual(first["block"], second["block"])
            self.assertNotEqual(first["role"], second["role"])
            if not first["warmup"]:
                first_roles.append(first["role"])
        self.assertEqual(first_roles.count("old"), 9)
        self.assertEqual(first_roles.count("new"), 9)

    def test_complete_raw_proof_and_summary(self):
        store, plan = proof()
        report = run.check(store, plan["output"])
        self.assertEqual(report["descriptive_guard"], "pass")
        self.assertEqual(report["threads"]["32"]["new_strictly_faster_pairs"], 3)
        self.assertEqual(report["threads"]["1"]["new_over_old"], 1)

    def test_actual_retention_container_works_without_original_paths(self):
        store, plan = proof()
        # Keep only source/action/validation bytes: unrelated baseline sample outputs need not be retained.
        payloads = {path: raw for path, raw in store.virtual.items() if "/offline/run/stages/" not in path and path != "/offline/run/result.json"}
        with tempfile.TemporaryDirectory() as directory:
            retained = fixtures.retained(Path(directory) / "collector", payloads)
            result = checker.verify([retained])
            self.assertEqual(len(result["runs"]), 1)
            self.assertEqual(result["runs"][0]["summary"]["descriptive_guard"], "pass")

    def test_archive_only_source_helpers_are_hydrated_before_pin_checks(self):
        store, plan = proof(helpers_in_archive=True)
        source_prefix = plan["sources"]["new"]["root"] + "/"
        payloads = {path: raw for path, raw in store.virtual.items()
                    if not path.startswith(source_prefix) and "/offline/run/stages/" not in path
                    and path != "/offline/run/result.json"}
        self.assertTrue(all(plan["pins"][name]["path"].startswith(source_prefix) for name in run.dependencies()))
        with tempfile.TemporaryDirectory() as directory:
            retained = fixtures.retained(Path(directory) / "collector", payloads)
            unhydrated = checker.run.portable.Store([retained])
            self.assertFalse(any(path.startswith(source_prefix) for path in unhydrated.paths))
            with self.assertRaisesRegex(ValueError, "required bytes not retained"):
                checker.run.check(unhydrated, plan["output"])
            result = checker.verify([retained])
            self.assertEqual(result["runs"][0]["summary"]["descriptive_guard"], "pass")

    def test_warmup_excluded_and_both_guards_enforced(self):
        store, plan = proof()
        state = store.read(run.join(plan["output"], "result.json"))
        for entry in state["stages"]:
            if entry["stage"]["warmup"]:
                entry["sample"]["timing"]["run_seconds"] = 1e9
        self.assertEqual(run.summarize(plan["protocol"], state["stages"])["descriptive_guard"], "pass")
        for entry in state["stages"]:
            if entry["stage"]["role"] == "new" and entry["stage"]["threads"] == 1:
                entry["sample"]["timing"]["run_seconds"] *= 2
        self.assertEqual(run.summarize(plan["protocol"], state["stages"])["descriptive_guard"], "miss")

    def test_saved_summary_tamper_rejected(self):
        store, plan = proof()
        path = run.join(plan["output"], "result.json")
        state = store.read(path)
        state["summary"]["descriptive_guard"] = "miss"
        store.put(path, state)
        with self.assertRaisesRegex(ValueError, "saved summary"):
            run.check(store, plan["output"])

    def test_fixed_baseline_iterations_and_order_enforced(self):
        store, plan = proof()
        path = run.join(plan["output"], "result.json")
        state = store.read(path)
        state["stages"][:2] = state["stages"][:2][::-1]
        store.put(path, state)
        with self.assertRaisesRegex(ValueError, "schedule differs"):
            run.check(store, plan["output"])
        frozen = store.read(plan["pins"]["baseline_frozen"]["path"])
        frozen["iterations"]["river"] = 999
        plan["pins"]["baseline_frozen"] = store.put(plan["pins"]["baseline_frozen"]["path"], frozen)
        with self.assertRaisesRegex(ValueError, "fixed River count"):
            run.check_pins(store, plan)

    def test_wrong_executable_rejected(self):
        store, plan = proof()
        path = run.join(plan["output"], "result.json")
        state = store.read(path)
        entry = state["stages"][0]
        record = store.read(entry["record"]["path"])
        record["resolved_argv"][0] = plan["pins"]["python"]["path"]
        entry["record"] = store.put(entry["record"]["path"], record)
        store.put(path, state)
        with self.assertRaisesRegex(ValueError, "sample command"):
            run.check(store, plan["output"])

    def test_failed_prefix_and_skipped_suffix_have_no_guard(self):
        store, plan = proof()
        path = run.join(plan["output"], "result.json")
        state = store.read(path)
        state.update(status="failed", stages=state["stages"][:2], error={"message": "deadline"},
                     skipped=[{"stage": stage, "reason": "deadline"} for stage in run.stages(plan["protocol"])[2:]])
        del state["summary"]
        store.put(path, state)
        result = run.check(store, plan["output"])
        self.assertEqual(result["descriptive_guard"], "not_evaluated")
        self.assertEqual(result["skipped_samples"], 46)


if __name__ == "__main__":
    unittest.main()
