"""Pure bounded contract tests: no Linux host calls, Cargo, solve or archive scan."""
from pathlib import Path
import copy
import importlib.util
import json
import types
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent

def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value

run = module("grain_test_run", HERE / "run.py")
analyze = module("grain_test_analyze", HERE / "analyze.py")
adapter = module("grain_test_adapter", HERE / "adapter/prepare.py")

class Contracts(unittest.TestCase):
    def test_matrix_and_canonical_order(self):
        rows = run.schedule()
        self.assertEqual(len(rows), 38)
        self.assertEqual(len({r["name"] for r in rows}), 38)
        self.assertEqual([r["kind"] for r in rows[:6]], ["smoke"]*4+["canonical"]*2)
        self.assertEqual([(r["workers"],r["depth"]) for r in rows[:4]], [(1,2),(1,1),(32,2),(32,1)])
        matrix = rows[6:]
        self.assertEqual(sum(r["warmup"] for r in matrix), 8)
        for case in run.CASES:
            for workers in run.WORKERS:
                for depth in run.DEPTHS:
                    self.assertEqual([r["round"] for r in matrix if (r["case"],r["workers"],r["depth"]) == (case,workers,depth)], [0,1,2,3])
        self.assertEqual([(r["workers"],r["depth"]) for r in matrix[4:8]], [(32,1),(32,2),(16,1),(16,2)])
        self.assertEqual({run.canonical_key(r) for r in rows}, {"smoke-narrow","narrow","expanded"})

    def test_inverse_adapter_and_quality_reset(self):
        original = adapter.BASE.read_bytes()
        self.assertEqual(adapter.pin(original)["sha256"], adapter.BASE_SHA)
        generated = adapter.derive(original.decode())
        self.assertEqual(adapter.inverse(generated).encode(), original)
        self.assertEqual((HERE/"adapter/solve.rs").read_bytes(), generated.encode())
        self.assertEqual(adapter.pin(generated.encode())["sha256"], run.CPU_ADAPTER_SHA)
        self.assertEqual(generated.count("chance_depth: depth"), 1)
        self.assertEqual(generated.count("chance_depth: 2"), 1)
        self.assertLess(generated.index("chance_depth: 2"), generated.index("let quality_cpu_started"))
        self.assertLess(generated.index('out.join("grain.json")'), generated.index("let cfr_cpu_started"))
        self.assertIn("let depth: u32", generated)

    def test_strict_json_and_supervisor_real_contract(self):
        for text in ('{"x":1,"x":2}', '{"x":NaN}', '{"x":Infinity}', '{"x":1e999}'):
            with self.assertRaises(ValueError): run.loads(text)
        path = HERE.parents[1]/"cloud/vm14/original-toolchain-supervisor.json"
        record = run.read(path)
        run.terminal(record)
        for value in (None, "timeout", "failed"):
            bad = copy.deepcopy(record);bad["stop_reason"] = value
            with self.assertRaises(ValueError): run.terminal(bad)

    def test_v3_flags_and_os_usable_gate(self):
        e = {"returncode":0,"stderr":"","stdout":"x86-64-v3 (supported, searched)\n",
             "per_cpu_flags":{"0":sorted(run.V3_FLAGS),"1":sorted(run.V3_FLAGS)}}
        run.validate_features(e,[0,1])
        for change in ("avx", "xsave", "abm", "bmi2"):
            bad = copy.deepcopy(e);bad["per_cpu_flags"]["1"].remove(change)
            with self.assertRaises(ValueError):run.validate_features(bad,[0,1])
        bad=copy.deepcopy(e);bad["stdout"]="x86-64-v3\n"
        with self.assertRaises(ValueError):run.validate_features(bad,[0,1])
        self.assertNotIn("osxsave", run.V3_FLAGS)

    def test_explicit_portable_environment(self):
        tools={"rustc":{"path":"/tool/rustc"}}
        env=run.environment(tools)
        self.assertEqual(env["RUSTFLAGS"], "-C target-cpu=x86-64-v3")
        self.assertEqual(env["CARGO_BUILD_JOBS"], "2")
        with patch.dict(run.os.environ, {"CARGO_ENCODED_RUSTFLAGS":"-Ctarget-cpu=native", "CARGO_PROFILE_RELEASE_OPT_LEVEL":"0", "RUSTC_WRAPPER":"x", "CARGO_BUILD_TARGET":"other"},clear=True):
            run.clean_environment({"environment":env})
            self.assertEqual(dict(run.os.environ),env)
        self.assertEqual(run.phase_limits("build")["memory_limit_bytes"],4*1024**3)
        self.assertEqual(run.phase_limits("measure")["memory_limit_bytes"],8*1024**3)

    def test_required_native_tests_missing_or_duplicate_rejected(self):
        good="\n".join("test "+n+" ... ok" for n in run.TEST_NAMES)
        run.validate_tests(good)
        for bad in ("\n".join(good.splitlines()[1:]), good+"\n"+good.splitlines()[0]):
            with self.assertRaises(ValueError):run.validate_tests(bad)

    def test_observation_scope_and_depth(self):
        row=run.schedule()[0]
        grain={"schema":"r1.flop-chance-grain-observation/v1","case":"narrow","cfr_depth":2,"quality_depth":2,"min_children":12,
               "eligible_chance_nodes_by_depth":[2,100],"eligible_child_edges_by_depth":[90,4400],
               "counts_scope":"one structural root traversal; not tasks, iterations or seat passes"}
        run.validate_grain(row,grain)
        for key,value in (("quality_depth",1),("cfr_depth",1),("counts_scope","tasks")):
            bad=grain|{key:value}
            with self.assertRaises(ValueError):run.validate_grain(row,bad)

    def groups(self):
        groups=[]
        for case in run.CASES:
            for depth in run.DEPTHS:
                for workers in run.WORKERS:
                    cfr=.90 if depth==1 and workers==32 else 1.
                    metrics={"cfr_wall_seconds":analyze.stats([cfr]*3),"quality_7_walks_wall_seconds":analyze.stats([1.]*3),
                             "cfr_plus_quality_wall_seconds":analyze.stats([cfr+1.]*3),"root_os_peak_resident_bytes":analyze.stats([100]*3)}
                    groups.append({"case":case,"depth":depth,"workers":workers,"metrics":metrics})
        return groups

    def test_predeclared_screen_complete_and_each_guard(self):
        self.assertEqual(analyze.summarize_groups(self.groups())["performance_screen"],"passed")
        for metric,values,workers in (("cfr_wall_seconds",[.96]*3,32),("cfr_wall_seconds",[1.04]*3,16),
                                      ("quality_7_walks_wall_seconds",[1.06]*3,16),("root_os_peak_resident_bytes",[111]*3,16),
                                      ("quality_7_walks_wall_seconds",[.9,1.,1.04],16)):
            groups=self.groups();next(g for g in groups if (g["case"],g["depth"],g["workers"])==("narrow",1,workers))["metrics"][metric]=analyze.stats(values)
            self.assertEqual(analyze.summarize_groups(groups)["performance_screen"],"rejected")
        with self.assertRaises(ValueError):analyze.summarize_groups(self.groups()[:-1])

    def test_partial_schedule_cannot_summarize(self):
        with self.assertRaises(ValueError):analyze.summarize([{**r,"status":"completed"} for r in run.schedule()[:-1]])

    def test_build_and_measure_receipts_separate(self):
        writes=[]
        with patch.object(run.durable,"atomic_json",side_effect=lambda p,v: writes.append((str(p),v))):
            run.persist(Path("proof"),{"receipt_file":"build-execution.json","prepared_plan":{"ignored":1}})
            run.persist(Path("proof"),{"receipt_file":"execution.json","prepared_plan":{"ignored":1}})
        self.assertTrue(writes[0][0].endswith("build-execution.json"))
        self.assertTrue(writes[1][0].endswith("execution.json"))
        self.assertNotIn("prepared_plan",writes[0][1])

    def test_reboot_binding_rejects_same_boot_or_other_instance(self):
        original={"phase":"build","plan_file":"plan.json","host":{"boot_id":"build-boot","instance_id":"123"},
                  "limits":run.phase_limits("build"),"created_at":"2026-09-27T00:00:00+00:00", "deadline_utc":"2026-09-27T00:20:00+00:00",
                  "deadline_monotonic":1000,"sources":{"baseline":{"files":{"source":"pin"}}}}
        files={n:{"bytes":1,"sha256":n} for n in ("plan.json","build.json","build-execution.json")}
        measure={**original,"phase":"measure","plan_file":"measurement.json","host":{"boot_id":"measurement-boot","instance_id":"123"},
                 "limits":run.phase_limits("measure"),"created_at":"2026-09-27T00:25:00+00:00","deadline_utc":"2026-09-27T00:45:00+00:00",
                 "deadline_monotonic":2000,"build_plan":files["plan.json"],"build_receipt":files["build.json"],"build_execution":files["build-execution.json"]}
        executed={"ended_at":"2026-09-27T00:19:00+00:00"}
        e=types.SimpleNamespace(plan=original,files=files,root=Path("proof"))
        with patch.object(analyze,"read",return_value=measure),patch.object(analyze,"verify_host"):
            analyze.verify_measurement(e,executed)
        self.assertEqual(e.plan,measure)
        for fields in ({"boot_id":"build-boot","instance_id":"123"},{"boot_id":"measurement-boot","instance_id":"999"}):
            e.plan=original
            with patch.object(analyze,"read",return_value=measure|{"host":fields}),patch.object(analyze,"verify_host"):
                with self.assertRaises(ValueError):analyze.verify_measurement(e,executed)
        e.plan=original
        with patch.object(analyze,"read",return_value=measure|{"sources":{}}),patch.object(analyze,"verify_host"):
            with self.assertRaises(ValueError):analyze.verify_measurement(e,executed)

if __name__ == "__main__":
    unittest.main()
