"""Small invalid-fixture tests only. No Cargo, solver, cloud or network calls."""
import copy
import importlib.util
import io
import json
from pathlib import Path
import struct
import tarfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("final_pipeline_tested_runner", HERE / "run.py")
r = importlib.util.module_from_spec(spec)
spec.loader.exec_module(r)


class MemoryStore:
    def __init__(self):
        self.files = {}
        self.virtual = {}
        self.entries = {}

    def put(self, path, value):
        value = value if isinstance(value, bytes) else json.dumps(value, allow_nan=False).encode()
        self.files[path] = value
        self.entries[path] = self.pin(path)
        return self.entries[path]

    def data(self, path):
        return self.virtual[path] if path in self.virtual else self.files[path]

    def json(self, path):
        return r.decode(self.data(path))

    def pin(self, path):
        return {"path": path, **r.digest(self.data(path))}

    def verify(self, pin):
        r.require(self.pin(pin["path"]) == pin, "memory fixture SHA differs")


def canonical(wall=1.0, ev=3.0):
    return b"r1.codec.canonical/v1\0" + struct.pack("<Q", 3) + b"cfg" + struct.pack("<Q6d", 25, .01, .02, ev, 4., .03, wall) + b"rest"


def progress(ncs=(.1, .03)):
    return [{"iteration": 25 * (i + 1), "expl_p0": n, "expl_p1": 0., "nash_conv": n, "elapsed_secs": float(i + 1)} for i, n in enumerate(ncs)]


def live(rows):
    x = rows[-1]
    return {"iterations": x["iteration"], "nashConv": x["nash_conv"], "explP0": x["expl_p0"], "explP1": x["expl_p1"]}


class ScheduleAndQuality(unittest.TestCase):
    def setUp(self):
        self.protocol = r.read(HERE / "protocol.json")
        self.definition = self.protocol["cases"]["river"]

    def test_counts_and_pair_order(self):
        stages = r.schedule(self.protocol)
        self.assertEqual(len(stages), 156)
        self.assertEqual(sum(s["kind"] == "solve" for s in stages), 24)
        self.assertEqual(sum(s["kind"] == "audit" for s in stages), 24)
        self.assertEqual(sum(s["kind"] in ("decode-all", "read-root", "stream-write") for s in stages), 72)
        self.assertEqual(sum(s["kind"] == "solve" and not s["warmup"] for s in stages), 18)
        self.assertEqual([s["arm"] for s in stages if s["case"] == "river" and s["kind"] == "solve"],
                         ["old", "new", "new", "old", "old", "new", "new", "old"])
        self.assertEqual(len({s["label"] for s in stages}), 156)
        self.assertEqual([s["kind"] for s in stages[:30]], ["census"] * 6 + ["solve"] * 24)

    def test_materially_negative_quality_rejected_without_clamping(self):
        rows = progress((.1, -.001))
        with self.assertRaisesRegex(ValueError, "materially negative"):
            r.trajectory(rows, self.definition, live(rows))
        r.nonnegative_quality([-1e-7, .01], .0099999, 1e-6)
        with self.assertRaisesRegex(ValueError, "materially negative"):
            r.nonnegative_quality([-.001, .02], .019, 1e-6)

    def test_overlay_clippy_and_fresh_build_scope(self):
        self.assertEqual(len(r.build_schedule()), 9)
        plan = {"tools": {"cargo": {"path": "/cargo"}}, "arms": {"old": {"target": "/target"}}}
        for kind, package, example in (("probe-clippy", "cli", "hu_pipeline_probe"), ("codec-clippy", "formats", "sol_codec_bench")):
            argv = r.build_command(plan, {"kind": kind, "arm": "old"})
            self.assertEqual(argv, ["/cargo", "clippy", "--locked", "--release", "-p", package, "--example", example, "--target-dir", "/target", "--", "-D", "warnings"])

    def test_first_passing_check(self):
        rows = progress()
        self.assertEqual(len(r.trajectory(rows, self.definition, live(rows))), 2)
        self.assertNotIn("elapsed_secs", r.trajectory(rows, self.definition, live(rows))[0])

    def test_equal_target_is_not_success(self):
        rows = progress((.1, .04))
        with self.assertRaisesRegex(ValueError, "not reached"):
            r.trajectory(rows, self.definition, live(rows))

    def test_continuation_after_passing_is_rejected(self):
        rows = progress((.03, .02))
        with self.assertRaisesRegex(ValueError, "continued"):
            r.trajectory(rows, self.definition, live(rows))

    def test_cadence_cap_missing_and_nonfinite_rejected(self):
        for rows in ([], progress()[:-1], progress(), progress()):
            altered = copy.deepcopy(rows)
            if altered == progress()[:-1]:
                altered[0]["iteration"] = 500
            elif altered == progress():
                altered[-1]["nash_conv"] = float("nan")
            with self.assertRaises((ValueError, IndexError)):
                r.trajectory(altered, self.definition, live(altered) if altered else {})

    def test_nonfinite_and_duplicate_json_rejected(self):
        for value in (b'{"a":NaN}', b'{"a":1,"a":2}'):
            with self.assertRaises(ValueError):
                r.decode(value)

    def test_suffix_cannot_skip_then_pass_or_omit_stage(self):
        expected = [{"n": i} for i in range(3)]
        valid = [{"stage": s, "status": state} for s, state in zip(expected, ("passed", "failed", "skipped"))]
        r.suffix(valid, "failed", expected, ("ready",))
        for statuses in (("passed", "skipped", "passed"), ("passed", "failed", "failed"), ("passed", "pending", "skipped")):
            with self.assertRaises(ValueError):
                r.suffix([{**row, "status": s} for row, s in zip(valid, statuses)], "failed", expected, ("ready",))
        with self.assertRaises(ValueError):
            r.suffix(valid[:-1], "failed", expected, ("ready",))


class ArtifactChecks(unittest.TestCase):
    def test_only_wall_time_excluded_from_canonical(self):
        self.assertEqual(r.codec_canonical(canonical(1.)), r.codec_canonical(canonical(2.)))
        self.assertNotEqual(r.codec_canonical(canonical()), r.codec_canonical(canonical(ev=3.0001)))
        for data in (b"", canonical()[:25], canonical(float("inf"))):
            with self.assertRaises(ValueError):
                r.codec_canonical(data)

    def test_version_boundary(self):
        data = b"SLVRSOLV" + struct.pack("<H", 4) + bytes(32) + struct.pack("<Q", 25)
        self.assertEqual(r.header(data, b"SLVRSOLV", 4)["iteration"], 25)
        for magic, version in ((b"SLVRCKPT", 4), (b"SLVRSOLV", 3)):
            with self.assertRaises(ValueError):
                r.header(data, magic, version)

    def audit_fixture(self):
        store = MemoryStore()
        plan = {"output": "/proof", "protocol": r.read(HERE / "protocol.json")}
        stage = next(s for s in r.schedule(plan["protocol"]) if s["label"] == "river-b0-old-audit")
        artifact = store.put(r.join(r.run_dir(plan, stage), "solution.sol"), b"solution")
        report = {"schema": "solvers.research.hu-saved-profile-audit/v1", "threads": 1,
                  "artifact": {**artifact, "format_version": 3, "mode": "full", "source_storage": "f32", "blake3": "a" * 64},
                  "value_basis": "subgame_start_utility", "zero_sum_terminal_utility": True, "pot_chips": 20, "effective_stack_chips": 60,
                  "recomputed": {"profile": "stored_quantized", "ev": [0., 0.], "br": [.01, .02], "gains": [.01, .02], "nash_conv": .03},
                  "input_hash_secs": .001, "load_secs": .01, "eval_secs": .02, "pre_save_metadata": {"wall_secs": 4., "iterations": 50},
                  "rake": {"kind": "none"}, "utility": {"kind": "chip-ev"}, "ev_offset": [0., 0.]}
        stdout = "/proof/audit.stdout"
        store.put(stdout, report)
        record = {"elapsed_seconds": .1, "measurement": {}, "outputs": {"stdout": {"path": stdout}}}
        return store, plan, stage, record, report

    def test_saved_quality_comes_from_recomputed_profile(self):
        store, plan, stage, record, report = self.audit_fixture()
        self.assertEqual(r.sample(store, plan, stage, record)["quality"]["nash_conv"], .03)
        report["pre_save_metadata"]["nash_conv"] = 0.
        report["recomputed"].update(br=[.05, 0.], gains=[.05, 0.], nash_conv=.05)
        store.put(record["outputs"]["stdout"]["path"], report)
        with self.assertRaisesRegex(ValueError, "saved profile quality"):
            r.sample(store, plan, stage, record)

    def test_audit_rejects_wrong_version_threads_path_and_timer(self):
        for mutation in (lambda x: x.update(threads=2), lambda x: x["artifact"].update(format_version=4),
                         lambda x: x["artifact"].update(path="/other.sol"), lambda x: x.update(load_secs=5.)):
            store, plan, stage, record, report = self.audit_fixture()
            mutation(report); store.put(record["outputs"]["stdout"]["path"], report)
            with self.assertRaises(ValueError):
                r.sample(store, plan, stage, record)

    def test_saved_negative_gain_rejected_even_with_positive_nc(self):
        store, plan, stage, record, report = self.audit_fixture()
        report["recomputed"].update(br=[-.001, .02], gains=[-.001, .02], nash_conv=.019)
        store.put(record["outputs"]["stdout"]["path"], report)
        with self.assertRaisesRegex(ValueError, "materially negative"):
            r.sample(store, plan, stage, record)

    def test_raw_sweep_still_runs_after_record_pin_mismatch(self):
        directory = Path("retained-fixture")
        with patch.object(Path, "exists", return_value=True), patch.object(r, "retain_record", side_effect=ValueError("stdout pin differs")), patch.object(r, "retain_directory") as sweep:
            with self.assertRaisesRegex(ValueError, "stdout pin differs"):
                r.retain_stage(object(), directory, [])
            sweep.assert_called_once()

    def test_retained_byte_mismatch_rejected(self):
        store = MemoryStore(); pin = store.put("/x", b"abc"); store.files["/x"] = b"abd"
        with self.assertRaises(ValueError):
            store.verify(pin)

    def test_partial_and_complete_canonical_must_share_original_root(self):
        store = MemoryStore()
        live = {"iterations": 25, "nashConv": .03, "explP0": .01, "explP1": .02}
        summary = {"iterations": 25, "nash_conv": .03, "expl_oop": .01, "expl_ip": .02,
                   "ev_oop": 1., "ev_ip": 2., "nodes": 2, "stored_nodes": 1}
        presave = {"iterations": 25, "nash_conv": .03, "expl": [.01, .02], "ev": [1., 2.]}
        rows = [{"stage": {"label": "river-b0-old-solve", "kind": "solve", "case": "river", "arm": "old", "block": 0},
                 "status": "passed", "sample": {"live": live, "trajectory": [], "artifacts": {
                    "checkpoint.ckpt": {"bytes": 1, "sha256": "a"}, "run.toml": {"bytes": 1, "sha256": "b"}}}}]
        for kind, sample in (("summary", {"report": summary}), ("audit", {"pre_save": presave, "quality": {}, "economics": {}})):
            rows.append({"stage": {"label": "river-b0-old-" + kind, "kind": kind, "case": "river", "arm": "old", "block": 0},
                         "status": "passed", "sample": sample})
        for kind, root in (("decode-all", "a"), ("read-root", "b")):
            rows.append({"stage": {"label": "river-b0-old-" + kind, "kind": kind, "case": "river", "arm": "old", "block": 0},
                         "status": "passed", "sample": {"canonical": {}, "metadata": {"meta": presave, "node_count": 2, "stored_nodes": 1, "mode": "Full"},
                                                          "pins": {"root_canonical": {"bytes": 1, "sha256": root}}}})
        with self.assertRaisesRegex(ValueError, "partial/full root"):
            r.groups(store, rows)


class SourceAndMemory(unittest.TestCase):
    def source_fixture(self):
        store = MemoryStore()
        protocol = r.read(HERE / "protocol.json")
        plan = {"arms": {}, "controls": {}, "inputs": {}, "protocol": protocol}
        refs = {"schema": "r1.final-pipeline-source-pins/v1", "arms": {}}
        for key in r.OVERLAYS.values():
            plan["controls"][key] = store.put("/controls/" + key, key.encode())
        for role in ("old", "new"):
            values = {"Cargo.toml": b"cargo", "crates/demo/src/lib.rs": b"production"}
            refs["arms"][role] = {"revision": protocol["revisions"][role], "files": [{"path": k, **r.digest(v)} for k, v in values.items()]}
            values.update({k: v.encode() for k, v in r.OVERLAYS.items()})
            tar = io.BytesIO()
            with tarfile.open(fileobj=tar, mode="w:gz") as out:
                for path, data in values.items():
                    member = tarfile.TarInfo(path); member.size = len(data); out.addfile(member, io.BytesIO(data))
            archive = store.put(f"/{role}/source.tar.gz", tar.getvalue())
            manifest = {"base_commit": protocol["revisions"][role], "archive_bytes": archive["bytes"], "archive_sha256": archive["sha256"],
                        "files": [{"path": k, **r.digest(v)} for k, v in values.items()]}
            plan["arms"][role] = {"source": f"/{role}/source", "manifest": store.put(f"/{role}/manifest", manifest), "archive": archive}
        plan["controls"]["source-pins.json"] = store.put("/controls/source-pins.json", refs)
        for case in protocol["cases"]:
            raw = (HERE / "configs" / (case + ".toml")).read_bytes()
            plan["controls"][f"configs/{case}.toml"] = store.put("/controls/" + case, raw)
            plan["inputs"][case] = store.put("/inputs/" + case, raw)
        return store, plan

    def test_source_and_input_positive_fixture(self):
        store, plan = self.source_fixture()
        r.expected_sources(store, plan)

    def test_reference_revision_or_production_pin_changed(self):
        for which in ("revision", "sha"):
            store, plan = self.source_fixture()
            refs = store.json("/controls/source-pins.json")
            if which == "revision":
                refs["arms"]["old"]["revision"] = "bad"
            else:
                refs["arms"]["old"]["files"][1]["sha256"] = "0" * 64
            plan["controls"]["source-pins.json"] = store.put("/controls/source-pins.json", refs)
            with self.assertRaises(ValueError):
                r.expected_sources(store, plan)

    def test_fixture_cap_or_target_or_thread_changed(self):
        for old, new in ((b"iterations = 400", b"iterations = 500"), (b"target_nash_conv = 0.04", b"target_nash_conv = 0.05"), (b"threads = 1", b"threads = 2")):
            store, plan = self.source_fixture()
            data = store.data("/inputs/river").replace(old, new)
            plan["inputs"]["river"] = store.put("/inputs/river", data)
            plan["controls"]["configs/river.toml"] = store.put("/controls/river", data)
            with self.assertRaises(ValueError):
                r.expected_sources(store, plan)

    def test_extra_cargo_configuration_rejected(self):
        store, plan = self.source_fixture()
        arm = plan["arms"]["old"]
        manifest = store.json(arm["manifest"]["path"])
        values = {}
        with tarfile.open(fileobj=io.BytesIO(store.data(arm["archive"]["path"])), mode="r:gz") as source:
            for member in source:
                values[member.name] = source.extractfile(member).read()
        values[".cargo/config"] = b'[build]\nrustflags=["--cfg", "changed"]\n'
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w:gz") as archive:
            for path, data in values.items():
                member = tarfile.TarInfo(path); member.size = len(data); archive.addfile(member, io.BytesIO(data))
        arm["archive"] = store.put(arm["archive"]["path"], stream.getvalue())
        manifest.update(archive_bytes=arm["archive"]["bytes"], archive_sha256=arm["archive"]["sha256"],
                        files=[{"path": k, **r.digest(v)} for k, v in values.items()])
        arm["manifest"] = store.put(arm["manifest"]["path"], manifest)
        with self.assertRaisesRegex(ValueError, "Cargo configuration"):
            r.expected_sources(store, plan)

    def test_real_source04_foundation_raw_records(self):
        # Read immutable original evidence, not any executable or benchmark.
        archive = HERE.parent / "exact-mass/exact-proof04.tar.gz"
        store = MemoryStore()
        with tarfile.open(archive, "r:gz") as saved:
            members = {member.name: saved.extractfile(member).read() for member in saved if member.isfile()}
            retained = r.decode(members["retention.json"])
            for path, pin in retained["files"].items():
                data = members["payload/" + pin["sha256"]]
                self.assertEqual(r.digest(data), r.content(pin))
                store.put(path, data)
            plan_pin = store.put("/fixture/prior-plan.json", members["plan.json"])
        source_pin = store.put("/fixture/current-source-pins.json", (HERE / "source-pins.json").read_bytes())
        result = r.foundation(store, {"foundation_plan": plan_pin, "controls": {"source-pins.json": source_pin}, "protocol": r.read(HERE / "protocol.json")})
        self.assertEqual(result["tests"]["workspace-tests"], {"summary_count": 56, "passed": 958, "ignored": 31})
        self.assertEqual(result["tests"]["release-oracle"]["passed"], 3)
        self.assertEqual(result["tests"]["release-river-resolve"]["passed"], 1)
        previous = store.json(plan_pin["path"])
        record_path = store.json(previous["arms"]["new"]["validation"]["path"])["stages"][1]["record"]["path"]
        rec = store.json(record_path)
        rec["measurement"]["sample_count"] += 1
        store.files[record_path] = json.dumps(rec).encode()
        with self.assertRaises(ValueError):
            r.foundation(store, {"foundation_plan": plan_pin, "controls": {"source-pins.json": source_pin}, "protocol": r.read(HERE / "protocol.json")})

    def test_memory_floor_is_conservative_and_invalid_rows_are_inconclusive(self):
        def row(native, sampled):
            return {"root_only_observed": True, "memory": {"root_os_peak_resident_bytes": native,
                    "sampled_peak_tree_resident_bytes": sampled, "root_os_peak_source": "wait4.ru_maxrss_linux_kib"}}
        old, new = [row(1000, 900), row(1100, 950), row(1000, 920)], [row(100, 20), row(100, 20), row(100, 20)]
        self.assertAlmostEqual(r.rss_bound(old, new), 100 / 900)
        new[0]["memory"]["root_os_peak_resident_bytes"] = 1000
        self.assertGreater(r.rss_bound(old, new), .9)  # A large inherited floor cannot produce a false pass.
        for mutation in (lambda x: x.update(root_only_observed=False), lambda x: x["memory"].update(sampled_peak_tree_resident_bytes=0),
                         lambda x: x["memory"].update(root_os_peak_resident_bytes=1)):
            changed = copy.deepcopy(new); mutation(changed[0]); self.assertIsNone(r.rss_bound(old, changed))

    def test_same_arm_state_change_rejected(self):
        rows = []
        for i in range(2):
            rows.append({"stage": {"label": f"river-b{i}-old-solve", "kind": "solve", "case": "river", "arm": "old", "block": i},
                         "status": "passed", "sample": {"live": {"iterations": 25}, "trajectory": [], "artifacts": {
                             "checkpoint.ckpt": {"bytes": 1, "sha256": str(i)}, "run.toml": {"bytes": 1, "sha256": "x"}}}})
        with self.assertRaisesRegex(ValueError, "same-arm solve/state"):
            r.groups(MemoryStore(), rows)


if __name__ == "__main__":
    unittest.main(verbosity=2)
