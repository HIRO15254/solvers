"""Small in-memory protocol/validator fixtures; no solver/compiler/cloud calls."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("tested_focused_memory", HERE / "run.py")
r = importlib.util.module_from_spec(spec)
spec.loader.exec_module(r)


class Store:
    def __init__(self):
        self.files = {}
    def put(self, path, value):
        self.files[path] = value if isinstance(value, bytes) else json.dumps(value).encode()
        return {"path": path, **r.base.digest(self.files[path])}
    def data(self, path):
        return self.files[path]
    def json(self, path):
        return r.base.decode(self.files[path])
    def verify(self, pin):
        r.require(r.base.digest(self.files[pin["path"]]) == r.content(pin), "pin changed")


def fixture():
    store = Store()
    plan = {"output": "/proof", "memory_protocol": r.read(HERE / "protocol.json"),
            "protocol": r.read(HERE.parent / "final-pipeline/protocol.json"),
            "inputs": {"river": {"path": "/inputs/river.toml"}}}
    state = {"native": store.put("/proof/native-rss", b"native"), "binaries": {"old": {"cli": {"path": "/old/solvers"}}}}
    directory = "/proof/stages/calibration/calibration"
    report = {"schema": "r1.native-rss-calibration/v1", "status": "passed", "launcher": state["native"],
              "parent_allocation_bytes": 268435456, "parent_vmrss_kib": 280000,
              "parent_vmrss_kib_after": 280000, "elapsed_seconds": .5, "cases": []}
    for label, size, peak in (("small", 1048576, 3000), ("large", 67108864, 66000)):
        child = [state["native"]["path"], "--allocate", str(size)]
        native = {"schema": "r1.native-rss/v1", "source": "wait4.ru_maxrss_linux_kib", "argv": child,
                  "child_pid": 123, "exit_code": 0, "signaled": False, "term_signal": None,
                  "ru_maxrss_kib": peak, "elapsed_seconds": .1, "launcher_before_fork": {"vmrss_kib": 2000}}
        report_path = directory + "/" + label + ".native.json"
        case = {"case": label, "allocation_bytes": size, "launcher_returncode": 0,
                "command": [state["native"]["path"], "--report", report_path, "--", *child],
                "parent_vmrss_kib_before": 280000, "native": native, "elapsed_seconds": .2,
                "native_report": store.put(report_path, native)}
        for key in ("stdout", "stderr"):
            case[key] = store.put(directory + "/" + label + "." + key, b"")
        report["cases"].append(case)
    store.put(directory + "/result.json", report)
    return store, plan, state, report


class Tests(unittest.TestCase):
    def test_schedule_and_order(self):
        protocol = r.read(HERE.parent / "final-pipeline/protocol.json")
        stages = r.schedule(protocol)
        self.assertEqual(len(stages), 99)
        self.assertEqual([x["kind"] for x in stages[:3]], ["cc-version", "compile", "calibration"])
        self.assertEqual([x["kind"] for x in stages[3:27]], ["solve"] * 24)
        self.assertEqual(stages[3:27], [x for x in r.base.schedule(protocol) if x["kind"] == "solve"])
        self.assertEqual(sum(not x["warmup"] for x in stages if x["kind"] == "solve"), 18)
        self.assertEqual(len({x["label"] for x in stages}), 99)

    def test_good_calibration(self):
        store, plan, state, report = fixture()
        self.assertEqual(r.calibration_result(store, plan, state, {"label": "calibration"}, {"elapsed_seconds": .6}), report)

    def test_calibration_status_alone_is_not_enough(self):
        for mutate in (lambda q: q.update(parent_vmrss_kib=10), lambda q: q.update(parent_vmrss_kib_after=10),
                       lambda q: q.update(parent_allocation_bytes=1), lambda q: q.update(elapsed_seconds=61),
                       lambda q: q["cases"].reverse(), lambda q: q["cases"][0].update(parent_vmrss_kib_before=10)):
            store, plan, state, report = fixture()
            mutate(report)
            store.put("/proof/stages/calibration/calibration/result.json", report)
            with self.assertRaises(ValueError):
                r.calibration_result(store, plan, state, {"label": "calibration"}, {"elapsed_seconds": 100})

    def test_calibration_native_raw_pin_and_numeric_gates(self):
        for peak in (0, 40000, 200000):
            store, plan, state, report = fixture()
            case = report["cases"][0]
            case["native"]["ru_maxrss_kib"] = peak
            case["native_report"] = store.put(case["native_report"]["path"], case["native"])
            store.put("/proof/stages/calibration/calibration/result.json", report)
            with self.assertRaises(ValueError):
                r.calibration_result(store, plan, state, {"label": "calibration"}, {"elapsed_seconds": .6})
        store, plan, state, report = fixture()
        store.files[report["cases"][0]["native_report"]["path"]] = b"changed"
        with self.assertRaises(ValueError):
            r.calibration_result(store, plan, state, {"label": "calibration"}, {"elapsed_seconds": .6})

    def test_small_launcher_and_large_response_required(self):
        for mutate in (lambda n: n["launcher_before_fork"].update(vmrss_kib=200000),
                       lambda n: n.update(ru_maxrss_kib=40000), lambda n: n.update(ru_maxrss_kib=100000)):
            store, plan, state, report = fixture()
            case = report["cases"][1]
            mutate(case["native"])
            case["native_report"] = store.put(case["native_report"]["path"], case["native"])
            store.put("/proof/stages/calibration/calibration/result.json", report)
            with self.assertRaises(ValueError):
                r.calibration_result(store, plan, state, {"label": "calibration"}, {"elapsed_seconds": .6})

    def native_fixture(self):
        store, plan, state, _ = fixture()
        stage = {"label": "river-b1-old-solve", "kind": "solve", "case": "river", "arm": "old", "block": 1, "warmup": False}
        native = {"schema": "r1.native-rss/v1", "source": "wait4.ru_maxrss_linux_kib", "argv": r.base.command(plan, state, stage),
                  "child_pid": 123, "exit_code": 0, "signaled": False, "term_signal": None, "ru_maxrss_kib": 10000,
                  "elapsed_seconds": .1, "launcher_before_fork": {"vmrss_kib": 2000}}
        store.put("/proof/stages/river-b1-old-solve/native.json", native)
        store.put("/samples", b'{"pids":[100,123]}\n{"pids":[]}\n')
        return store, plan, state, stage, native, {"pid": 100, "elapsed_seconds": .2, "outputs": {"samples": {"path": "/samples"}}}

    def test_native_counter_bound_to_actual_child(self):
        store, plan, state, stage, native, record = self.native_fixture()
        self.assertEqual(r.native_result(store, plan, stage, record, state), native)
        for edit in (lambda x: x.update(child_pid=100), lambda x: x.update(exit_code=1),
                     lambda x: x.update(ru_maxrss_kib=0), lambda x: x.update(argv=["other"])):
            altered = copy.deepcopy(native)
            edit(altered)
            store.put("/proof/stages/river-b1-old-solve/native.json", altered)
            with self.assertRaises(ValueError):
                r.native_result(store, plan, stage, record, state)

    def test_unexpected_process_rejected(self):
        store, plan, state, stage, _, record = self.native_fixture()
        store.put("/samples", b'{"pids":[100,123,999]}\n')
        with self.assertRaisesRegex(ValueError, "unexpected subprocess"):
            r.native_result(store, plan, stage, record, state)

    def test_ratio_uses_max_min_native_only_and_excludes_warmup(self):
        protocol = r.read(HERE.parent / "final-pipeline/protocol.json")
        plan = {"protocol": protocol, "memory_protocol": r.read(HERE / "protocol.json")}
        state = {"stages": []}
        for stage in r.schedule(protocol):
            if stage["kind"] != "solve":
                continue
            peak = 999999 if stage["warmup"] else ([100, 200, 300] if stage["arm"] == "old" else [80, 80, 95])[stage["block"] - 1]
            state["stages"].append({"stage": stage, "status": "passed", "sample": {"native_child": {"ru_maxrss_kib": peak}, "memory": {"sampled_peak": 1}}})
        report = r.summary(plan, state)
        for case in protocol["cases"]:
            self.assertEqual(report["cases"][case]["max_new_over_min_old"], .95)
            self.assertFalse(report["cases"][case]["screen"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
