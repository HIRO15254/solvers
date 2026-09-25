"""Small process tests; no solver, build, cloud resource or third-party package."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
import unittest
import uuid
from contextlib import contextmanager
from unittest import mock


MODULE = Path(__file__).parents[1] / "run_supervised.py"
SPEC = importlib.util.spec_from_file_location("run_supervised", MODULE)
supervisor = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(supervisor)
TEST_TEMP_ROOT = Path(__file__).parents[2] / ".cache/tool-tests"
SUPPORTED = os.name == "nt" or sys.platform == "linux"


@contextmanager
def fixture():
    TEST_TEMP_ROOT.mkdir(parents=True, exist_ok=True)
    root = TEST_TEMP_ROOT / f"supervisor-{uuid.uuid4().hex}"
    root.mkdir()
    try:
        yield root
    finally:
        assert root.resolve().is_relative_to(TEST_TEMP_ROOT.resolve())
        shutil.rmtree(root)


def command(root, code, *options, timeout="3", grace="0.3"):
    return [sys.executable, str(MODULE), "--record", str(root / "record.json"),
            "--cwd", str(root), "--timeout-seconds", timeout,
            "--grace-seconds", grace, "--kill-wait-seconds", "2",
            "--poll-seconds", "0.025", *options, "--", sys.executable, "-c", code]


def invoke(root, code, *options, **kwargs):
    result = subprocess.run(command(root, code, *options, **kwargs),
                            capture_output=True, text=True, timeout=15, shell=False)
    path = root / "record.json"
    record = json.loads(path.read_text(encoding="utf-8")) if path.exists() else None
    return result, record


def alive(pid):
    if os.name == "nt":
        api = supervisor.WindowsAPI()
        handle = api.k.OpenProcess(0x100000, False, pid)  # SYNCHRONIZE only.
        if not handle:
            return False
        import _winapi
        try:
            return _winapi.WaitForSingleObject(handle, 0) == 258
        finally:
            api.k.CloseHandle(handle)
    path = Path(f"/proc/{pid}/stat")
    try:
        return path.read_bytes().rsplit(b")", 1)[1].split()[0] not in {b"Z", b"X"}
    except FileNotFoundError:
        return False


def wait_until(predicate, timeout=5):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.025)
    raise AssertionError("condition was not reached before deadline")


# Both the parent and its child install a handler before publishing their PID.
# Short sleeps let Python dispatch SIGBREAK too (a long Windows sleep need not).
TREE_SCRIPT = r'''
import os, pathlib, signal, subprocess, sys, time
mode, role = sys.argv[1:]
def stopped(number, frame):
    pathlib.Path(role + ".stopped").write_text("graceful")
    raise SystemExit(0)
signal.signal(signal.SIGINT, signal.SIG_IGN if mode != "cooperate" else stopped)
if hasattr(signal, "SIGBREAK"):
    signal.signal(signal.SIGBREAK, signal.SIG_IGN if mode != "cooperate" else stopped)
pathlib.Path(role + ".pid").write_text(str(os.getpid()))
if role == "parent":
    flags = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP if mode == "detached" else 0
    subprocess.Popen([sys.executable, __file__, mode, "child"], shell=False, creationflags=flags)
while True:
    time.sleep(0.01)
'''


class AtomicRecordTests(unittest.TestCase):
    def test_initial_record_is_exclusive_and_complete(self):
        with fixture() as root:
            path = root / "record.json"
            supervisor.atomic_json(path, {"state": "initial"}, initial=True)
            with self.assertRaises(FileExistsError):
                supervisor.atomic_json(path, {"state": "wrong"}, initial=True)
            self.assertEqual(json.loads(path.read_text()), {"state": "initial"})
            supervisor.atomic_json(path, {"state": "final"})
            self.assertEqual(json.loads(path.read_text()), {"state": "final"})
            self.assertEqual(list(root.glob("*.tmp")), [])

    def test_replacement_failure_leaves_previous_record(self):
        with fixture() as root:
            path = root / "record.json"
            supervisor.atomic_json(path, {"state": "initial"}, initial=True)
            with mock.patch.object(supervisor.os, "replace", side_effect=OSError("disk failure")):
                with self.assertRaises(OSError):
                    supervisor.atomic_json(path, {"state": "final"})
            self.assertEqual(json.loads(path.read_text()), {"state": "initial"})
            self.assertEqual(list(root.glob("*.tmp")), [])


@unittest.skipUnless(SUPPORTED, "process containment is implemented on Windows/Linux")
class ProcessTests(unittest.TestCase):
    def test_normal_exit_hashes_logs_and_records_os_peak(self):
        with fixture() as root:
            payload = root / "config.txt"
            payload.write_text("example", encoding="utf-8")
            result, record = invoke(root, "import sys; print('stdout'); print('stderr', file=sys.stderr)",
                                    "--identity-file", str(payload))
            self.assertEqual(result.returncode, 0, (result.stderr, record))
            self.assertEqual(record["state"], "completed")
            self.assertEqual(record["stop_reason"], "completed")
            self.assertTrue(record["cleanup_complete"])
            self.assertTrue(record["identity_unchanged"])
            self.assertGreater(record["measurement"]["root_os_peak_resident_bytes"], 0)
            self.assertFalse(record["shell"])
            self.assertEqual(record["errors"], [])
            self.assertEqual((root / "record.stdout.log").read_text().strip(), "stdout")
            self.assertEqual((root / "record.stderr.log").read_text().strip(), "stderr")
            for output in record["outputs"].values():
                self.assertEqual(output, supervisor.identity(Path(output["path"])))

    def test_child_failure_is_distinct_from_supervisor_failure(self):
        with fixture() as root:
            result, record = invoke(root, "raise SystemExit(7)")
            self.assertEqual(result.returncode, 1, record)
            self.assertEqual(record["child_exit_code"], 7)
            self.assertEqual(record["stop_reason"], "child_failed")
            self.assertTrue(record["cleanup_complete"])

    def test_argv_metacharacters_are_literal(self):
        with fixture() as root:
            argv = command(root, "import json,sys; print(json.dumps(sys.argv[1:]))")
            expected = ["two words", "quote\"and'quote", "$(nope); & | > marker", "日本語", "trailing\\"]
            result = subprocess.run(argv + expected, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            actual = json.loads((root / "record.stdout.log").read_text())
            self.assertEqual(actual, expected)
            self.assertFalse((root / "marker").exists())

    def test_timeout_gracefully_stops_parent_and_descendant(self):
        self.check_tree_stop("cooperate", forced=False)

    def test_timeout_kills_uncooperative_parent_and_descendant(self):
        self.check_tree_stop("ignore", forced=True)

    @unittest.skipUnless(os.name == "nt", "Windows Job includes detached descendants")
    def test_windows_job_kills_detached_descendant(self):
        self.check_tree_stop("detached", forced=True)

    def check_tree_stop(self, mode, forced):
        with fixture() as root:
            script = root / "tree.py"
            script.write_text(TREE_SCRIPT, encoding="utf-8")
            code = f"import runpy,sys; sys.argv=[{str(script)!r},{mode!r},'parent']; runpy.run_path(sys.argv[0],run_name='__main__')"
            result, record = invoke(root, code, timeout="0.7", grace="0.5")
            self.assertEqual(result.returncode, 124, (result.stderr, record))
            self.assertEqual(record["stop_reason"], "timeout")
            self.assertEqual(record["forced"], forced, record)
            self.assertTrue(record["cleanup_complete"], record)
            self.assertEqual(record["errors"], [], record)
            for role in ("parent", "child"):
                pid = int((root / f"{role}.pid").read_text())
                self.assertFalse(alive(pid), (role, pid, record))
                self.assertEqual((root / f"{role}.stopped").exists(), not forced)

    def test_tiny_memory_limit_triggers_finite_cleanup(self):
        with fixture() as root:
            result, record = invoke(root, "import time; data=bytearray(8*1024*1024); time.sleep(30)",
                                    "--memory-limit-bytes", "1")
            self.assertEqual(result.returncode, 125, record)
            self.assertEqual(record["stop_reason"], "tree_memory_limit")
            self.assertTrue(record["cleanup_complete"], record)
            self.assertGreater(record["measurement"]["sampled_peak_tree_resident_bytes"], 1)
            self.assertLess(record["elapsed_seconds"], 5)

    def test_disk_reserve_rejects_before_spawning(self):
        with fixture() as root:
            result, record = invoke(root, "from pathlib import Path; Path('executed').touch()",
                                    "--disk-reserve-bytes", str(2**63))
            self.assertEqual(result.returncode, 125, record)
            self.assertEqual(record["stop_reason"], "disk_reserve")
            self.assertNotIn("pid", record)
            self.assertFalse((root / "executed").exists())

    def test_memory_reserve_rejects_before_spawning(self):
        with fixture() as root:
            result, record = invoke(root, "raise SystemExit(0)", "--min-free-memory-bytes", str(2**63))
            self.assertEqual(result.returncode, 125, record)
            self.assertEqual(record["stop_reason"], "host_memory_reserve")
            self.assertNotIn("pid", record)

    def test_identity_change_quarantines_result_without_overwriting_stop_reason(self):
        with fixture() as root:
            source = root / "source.txt"
            source.write_text("before")
            result, record = invoke(root, "from pathlib import Path; Path('source.txt').write_text('after')",
                                    "--identity-file", str(source))
            self.assertEqual(result.returncode, 2, record)
            self.assertFalse(record["identity_unchanged"])
            self.assertEqual(record["stop_reason"], "completed")
            self.assertEqual(record["state"], "supervisor_error")

    def test_timeout_reason_survives_missing_identity_file(self):
        with fixture() as root:
            source = root / "source.txt"
            source.write_text("before")
            code = "import time; from pathlib import Path; Path('source.txt').unlink(); time.sleep(30)"
            result, record = invoke(root, code, "--identity-file", str(source), timeout="0.5")
            self.assertEqual(result.returncode, 2, record)
            self.assertEqual(record["stop_reason"], "timeout")
            self.assertTrue(record["cleanup_complete"])
            self.assertTrue(any(error["where"] == "identity_after" for error in record["errors"]))

    def test_existing_log_is_never_overwritten_or_executed(self):
        with fixture() as root:
            log = root / "record.stdout.log"
            log.write_text("existing")
            result, record = invoke(root, "from pathlib import Path; Path('executed').touch()")
            self.assertEqual(result.returncode, 2, record)
            self.assertEqual(log.read_text(), "existing")
            self.assertFalse((root / "executed").exists())
            self.assertEqual(record["stop_reason"], "supervisor_error")

    def test_missing_executable_has_final_failure_record(self):
        with fixture() as root:
            argv = command(root, "pass")
            argv[argv.index("--") + 1:] = [str(root / "missing-executable")]
            result = subprocess.run(argv, capture_output=True, text=True, timeout=10)
            record = json.loads((root / "record.json").read_text(encoding="utf-8"))
            self.assertEqual(result.returncode, 2)
            self.assertEqual(record["state"], "supervisor_error")
            self.assertNotIn("pid", record)

    def test_monitor_failure_forces_cleanup(self):
        with fixture() as root:
            args = supervisor.parser().parse_args(command(root, "import time; time.sleep(30)")[2:])
            args.command.pop(0)
            backend = supervisor.WindowsProcess if os.name == "nt" else supervisor.LinuxProcess
            original = backend.sample
            calls = 0

            def fail_once(instance):
                nonlocal calls
                calls += 1
                if calls == 1:
                    raise OSError("injected monitor failure")
                return original(instance)

            with mock.patch.object(backend, "sample", fail_once):
                code = supervisor.supervise(args)
            record = json.loads((root / "record.json").read_text(encoding="utf-8"))
            self.assertEqual(code, 2, record)
            self.assertTrue(record["forced"])
            self.assertTrue(record["cleanup_complete"])
            self.assertFalse(alive(record["pid"]))

    def test_record_write_failure_after_spawn_forces_cleanup(self):
        with fixture() as root:
            args = supervisor.parser().parse_args(command(root, "import time; time.sleep(30)")[2:])
            args.command.pop(0)
            original = supervisor.atomic_json
            calls = 0

            def fail_running(path, record, **kwargs):
                nonlocal calls
                calls += 1
                if calls == 2:
                    raise OSError("injected record write failure")
                return original(path, record, **kwargs)

            with mock.patch.object(supervisor, "atomic_json", fail_running):
                code = supervisor.supervise(args)
            record = json.loads((root / "record.json").read_text(encoding="utf-8"))
            self.assertEqual(code, 2, record)
            self.assertTrue(record["forced"])
            self.assertTrue(record["cleanup_complete"])
            self.assertFalse(alive(record["pid"]))

    def test_parent_exit_does_not_leave_a_background_child(self):
        with fixture() as root:
            script = root / "tree.py"
            script.write_text(TREE_SCRIPT, encoding="utf-8")
            code = f"import subprocess,sys; subprocess.Popen([sys.executable,{str(script)!r},'ignore','child'])"
            result, record = invoke(root, code, timeout="5")
            self.assertEqual(result.returncode, 1, record)
            self.assertEqual(record["stop_reason"], "descendants_after_root_exit")
            self.assertTrue(record["forced"])
            self.assertTrue(record["cleanup_complete"])
            self.assertFalse(alive(int((root / "child.pid").read_text())))

    def test_unavailable_metrics_reject_before_spawn(self):
        with fixture() as root:
            args = supervisor.parser().parse_args(command(root, "raise SystemExit(0)")[2:])
            args.command.pop(0)
            backend = supervisor.WindowsProcess if os.name == "nt" else supervisor.LinuxProcess
            with mock.patch.object(backend, "preflight", side_effect=OSError("metrics unavailable")):
                code = supervisor.supervise(args)
            record = json.loads((root / "record.json").read_text(encoding="utf-8"))
            self.assertEqual(code, 2, record)
            self.assertEqual(record["state"], "supervisor_error")
            self.assertNotIn("pid", record)

    @unittest.skipUnless(os.name == "nt", "Windows kill-on-close guarantee")
    def test_windows_job_kills_descendants_when_supervisor_is_killed(self):
        with fixture() as root:
            script = root / "tree.py"
            script.write_text(TREE_SCRIPT, encoding="utf-8")
            code = f"import runpy,sys; sys.argv=[{str(script)!r},'ignore','parent']; runpy.run_path(sys.argv[0],run_name='__main__')"
            process = subprocess.Popen(command(root, code, timeout="30"), shell=False,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                       creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                wait_until(lambda: (root / "child.pid").exists())
                parent = int((root / "parent.pid").read_text())
                child = int((root / "child.pid").read_text())
                self.assertTrue(alive(parent))
                self.assertTrue(alive(child))
                process.kill()
                process.wait(timeout=5)
                wait_until(lambda: not alive(parent) and not alive(child))
                record = json.loads((root / "record.json").read_text(encoding="utf-8"))
                self.assertEqual(record["state"], "running")
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)

    @unittest.skipUnless(os.name == "nt", "Windows suspended launch")
    def test_windows_job_assignment_failure_never_executes_workload(self):
        with fixture() as root:
            args = supervisor.parser().parse_args(command(
                root, "from pathlib import Path; Path('executed').touch()"
            )[2:])
            args.command.pop(0)
            original = supervisor.WindowsAPI.__init__

            def refuse_assignment(api):
                original(api)
                api.k.AssignProcessToJobObject = lambda job, process: False

            with mock.patch.object(supervisor.WindowsAPI, "__init__", refuse_assignment):
                code = supervisor.supervise(args)
            record = json.loads((root / "record.json").read_text(encoding="utf-8"))
            self.assertEqual(code, 2, record)
            self.assertFalse((root / "executed").exists())
            self.assertTrue(record["cleanup_complete"])
            self.assertFalse(alive(record["pid"]))


if __name__ == "__main__":
    unittest.main()
