"""Run one bounded experiment, retaining logs and explicit resource measurements.

CPython 3.11+ on Windows or Linux; standard library only. This is a research
runner, not a solver quality validator or a replacement for a Linux cgroup.
"""

from __future__ import annotations

import argparse
import contextlib
import ctypes
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import tempfile
import time


SCHEMA = "solvers.supervised-run/v1"


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def identity(path: Path) -> dict:
    """Hash bytes, checking for mutation during hashing (not a source snapshot)."""
    path = path.resolve(strict=True)
    before = path.stat()
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    after = path.stat()
    stamp = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
    if stamp(before) != stamp(after):
        raise RuntimeError(f"file changed while hashing: {path}")
    return {"path": str(path), "sha256": digest.hexdigest(), "bytes": after.st_size}


def atomic_json(path: Path, value: dict, *, initial: bool = False) -> None:
    """A complete, fsynced file is linked exclusively or atomically replaced."""
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
            json.dump(value, stream, ensure_ascii=False, indent=2, allow_nan=False)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        if initial:
            # Unlike an exists()/replace() pair this cannot overwrite another run.
            # Fail explicitly on a filesystem without hard links.
            os.link(temporary, path)
        else:
            os.replace(temporary, path)
        if os.name == "posix":
            directory_fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(temporary)


def resolve_executable(name: str, cwd: Path) -> Path:
    if os.path.dirname(name):
        found = Path(name)
        if not found.is_absolute():
            found = cwd / found
    else:
        # Resolve PATH relative entries against the child's actual cwd.
        search = os.pathsep.join(
            str((cwd / entry).resolve()) if not os.path.isabs(entry) else entry
            for entry in os.get_exec_path()
        )
        match = shutil.which(name, path=search)
        if match is None:
            raise FileNotFoundError(f"executable not found: {name}")
        found = Path(match)
    found = found.resolve(strict=True)
    if not found.is_file():
        raise ValueError(f"executable is not a file: {found}")
    if os.name == "nt" and found.suffix.lower() in {".bat", ".cmd"}:
        raise ValueError("batch files are unsupported; invoke a native executable directly")
    return found


def linux_memory() -> dict:
    fields = {}
    for line in Path("/proc/meminfo").read_text(encoding="ascii").splitlines():
        key, _, rest = line.partition(":")
        if key in {"MemTotal", "MemAvailable"}:
            number, unit = rest.split()
            if unit != "kB":
                raise RuntimeError(f"unsupported /proc/meminfo unit: {unit}")
            fields[key] = int(number) * 1024
    if set(fields) != {"MemTotal", "MemAvailable"}:
        raise RuntimeError("Linux MemAvailable/MemTotal metrics are unavailable")
    return {"total_bytes": fields["MemTotal"], "available_bytes": fields["MemAvailable"]}


class LinuxProcess:
    containment = {
        "kind": "posix_session_process_group",
        "limitations": [
            "setsid/setpgid can escape this process group; use a systemd/cgroup wrapper",
            "supervisor SIGKILL, host failure or power loss does not kill the group",
            "resident memory is sampled /proc RSS; brief descendants may be missed",
            "summed RSS can double-count shared pages; this is not cgroup memory.current",
            "wait4 root peak can include reaped descendant high-water marks; it is not a simultaneous tree peak",
        ],
    }

    @staticmethod
    def preflight() -> dict:
        if not hasattr(os, "wait4") or not Path("/proc/self/statm").is_file():
            raise RuntimeError("Linux wait4 and readable /proc metrics are required")
        Path("/proc/self/stat").read_text(encoding="ascii")
        return linux_memory()

    def __init__(self, argv: list[str], cwd: Path, stdout, stderr):
        self.process = subprocess.Popen(
            argv, cwd=cwd, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
            shell=False, start_new_session=True, close_fds=True,
        )
        self.pid = self.process.pid
        self.root_peak = None
        self.root_peak_source = None
        self.reaped = False

    def start(self) -> None:
        pass  # Popen establishes the session before exec.

    def poll(self):
        if not self.reaped:
            pid, status, usage = os.wait4(self.pid, os.WNOHANG)
            if pid:
                self.process.returncode = os.waitstatus_to_exitcode(status)
                self.root_peak = int(usage.ru_maxrss) * 1024
                self.root_peak_source = "wait4.ru_maxrss_linux_kib"
                self.reaped = True
        return self.process.returncode

    def sample(self) -> dict:
        rss = 0
        members = []
        for item in Path("/proc").iterdir():
            if not item.name.isdigit():
                continue
            try:
                # comm may contain spaces and ')' characters.
                fields = (item / "stat").read_bytes().rsplit(b")", 1)[1].split()
                if int(fields[2]) != self.pid or fields[0] in {b"Z", b"X"}:
                    continue
                member_rss = int((item / "statm").read_text(encoding="ascii").split()[1])
                rss += member_rss * os.sysconf("SC_PAGE_SIZE")
                members.append(int(item.name))
            except (FileNotFoundError, ProcessLookupError):
                continue  # Exited between enumerating and reading.
            except PermissionError as error:
                # /proc mounted with hidepid cannot establish complete membership.
                raise RuntimeError(f"cannot inspect /proc process {item.name}: {error}") from error
        return {
            "pids": sorted(members), "tree_resident_bytes": rss,
            "root_os_peak_resident_bytes": self.root_peak,
            "root_os_peak_source": self.root_peak_source,
            "job_os_peak_commit_bytes": None,
        }

    def graceful(self) -> dict:
        try:
            os.killpg(self.pid, signal.SIGINT)
        except ProcessLookupError:
            return {"method": "SIGINT_process_group", "delivered": False, "already_empty": True}
        return {"method": "SIGINT_process_group", "delivered": True}

    def kill(self) -> None:
        try:
            os.killpg(self.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

    def close(self) -> None:
        self.poll()


class WindowsAPI:
    """The public Job/PSAPI interfaces; process creation uses CPython's wrapper."""

    def __init__(self):
        from ctypes import wintypes as w

        size = ctypes.c_size_t
        ull = ctypes.c_ulonglong

        class BasicLimits(ctypes.Structure):
            _fields_ = [
                ("PerProcessUserTimeLimit", ctypes.c_longlong),
                ("PerJobUserTimeLimit", ctypes.c_longlong), ("LimitFlags", w.DWORD),
                ("MinimumWorkingSetSize", size), ("MaximumWorkingSetSize", size),
                ("ActiveProcessLimit", w.DWORD), ("Affinity", size),
                ("PriorityClass", w.DWORD), ("SchedulingClass", w.DWORD),
            ]

        class IoCounters(ctypes.Structure):
            _fields_ = [(name, ull) for name in (
                "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
                "ReadTransferCount", "WriteTransferCount", "OtherTransferCount",
            )]

        class ExtendedLimits(ctypes.Structure):
            _fields_ = [
                ("BasicLimitInformation", BasicLimits), ("IoInfo", IoCounters),
                ("ProcessMemoryLimit", size), ("JobMemoryLimit", size),
                ("PeakProcessMemoryUsed", size), ("PeakJobMemoryUsed", size),
            ]

        class ProcessMemory(ctypes.Structure):
            _fields_ = [("cb", w.DWORD), ("PageFaultCount", w.DWORD)] + [
                (name, size) for name in (
                    "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
                    "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
                    "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage",
                )
            ]

        class MemoryStatus(ctypes.Structure):
            _fields_ = [("dwLength", w.DWORD), ("dwMemoryLoad", w.DWORD)] + [
                (name, ull) for name in (
                    "ullTotalPhys", "ullAvailPhys", "ullTotalPageFile", "ullAvailPageFile",
                    "ullTotalVirtual", "ullAvailVirtual", "ullAvailExtendedVirtual",
                )
            ]

        self.w = w
        self.ExtendedLimits = ExtendedLimits
        self.ProcessMemory = ProcessMemory
        self.MemoryStatus = MemoryStatus
        self.k = ctypes.WinDLL("kernel32", use_last_error=True)
        self.psapi = ctypes.WinDLL("psapi", use_last_error=True)
        signatures = {
            "CreateJobObjectW": ([ctypes.c_void_p, w.LPCWSTR], w.HANDLE),
            "SetInformationJobObject": ([w.HANDLE, ctypes.c_int, ctypes.c_void_p, w.DWORD], w.BOOL),
            "QueryInformationJobObject": ([w.HANDLE, ctypes.c_int, ctypes.c_void_p, w.DWORD, ctypes.c_void_p], w.BOOL),
            "AssignProcessToJobObject": ([w.HANDLE, w.HANDLE], w.BOOL),
            "TerminateJobObject": ([w.HANDLE, w.UINT], w.BOOL),
            "CloseHandle": ([w.HANDLE], w.BOOL),
            "ResumeThread": ([w.HANDLE], w.DWORD),
            "OpenProcess": ([w.DWORD, w.BOOL, w.DWORD], w.HANDLE),
            "GlobalMemoryStatusEx": ([ctypes.POINTER(MemoryStatus)], w.BOOL),
            "AttachConsole": ([w.DWORD], w.BOOL),
            "FreeConsole": ([], w.BOOL),
            "GetConsoleProcessList": ([ctypes.POINTER(w.DWORD), w.DWORD], w.DWORD),
            "GenerateConsoleCtrlEvent": ([w.DWORD, w.DWORD], w.BOOL),
            "SetConsoleCtrlHandler": ([ctypes.c_void_p, w.BOOL], w.BOOL),
        }
        for name, (arguments, result) in signatures.items():
            function = getattr(self.k, name)
            function.argtypes = arguments
            function.restype = result
        self.psapi.GetProcessMemoryInfo.argtypes = [w.HANDLE, ctypes.POINTER(ProcessMemory), w.DWORD]
        self.psapi.GetProcessMemoryInfo.restype = w.BOOL

    @staticmethod
    def check(ok, operation: str):
        if not ok:
            raise ctypes.WinError(ctypes.get_last_error(), operation)
        return ok

    def memory(self) -> dict:
        value = self.MemoryStatus()
        value.dwLength = ctypes.sizeof(value)
        self.check(self.k.GlobalMemoryStatusEx(ctypes.byref(value)), "GlobalMemoryStatusEx")
        return {
            "total_bytes": value.ullTotalPhys, "available_bytes": value.ullAvailPhys,
            "commit_available_bytes": value.ullAvailPageFile,
        }

    def process_memory(self, handle) -> tuple[int, int]:
        value = self.ProcessMemory()
        value.cb = ctypes.sizeof(value)
        self.check(self.psapi.GetProcessMemoryInfo(handle, ctypes.byref(value), value.cb), "GetProcessMemoryInfo")
        return value.WorkingSetSize, value.PeakWorkingSetSize


class WindowsProcess:
    containment = {
        "kind": "windows_job_kill_on_close_suspended_assignment",
        "limitations": [
            "RAM trigger is sampled working set, not a hard allocation limit",
            "summed working sets can double-count shared pages",
            "Job membership can include the hidden console host",
            "CTRL_BREAK covers the dedicated console; detached descendants require Job termination",
            "job peak committed bytes are not resident working set bytes",
        ],
    }

    @staticmethod
    def preflight() -> dict:
        import _winapi  # CPython's native CreateProcess wrapper, not a shell.
        if not hasattr(_winapi, "CreateProcess"):
            raise RuntimeError("CPython native Windows process creation is unavailable")
        api = WindowsAPI()
        process = api.check(api.k.OpenProcess(0x0400 | 0x0010, False, os.getpid()), "OpenProcess self")
        try:
            api.process_memory(process)
        finally:
            api.k.CloseHandle(process)
        return api.memory()

    def __init__(self, argv: list[str], cwd: Path, stdout, stderr):
        import _winapi
        import msvcrt

        self.native = _winapi
        self.api = WindowsAPI()
        self.job = self.api.check(self.api.k.CreateJobObjectW(None, None), "CreateJobObjectW")
        self.handle = None
        self.thread = None
        self.returncode = None
        self.pid = None
        try:
            limits = self.api.ExtendedLimits()
            limits.BasicLimitInformation.LimitFlags = 0x2000  # KILL_ON_JOB_CLOSE; no breakaway.
            self.api.check(self.api.k.SetInformationJobObject(
                self.job, 9, ctypes.byref(limits), ctypes.sizeof(limits)), "SetInformationJobObject")
            with open(os.devnull, "rb") as stdin:
                handles = [msvcrt.get_osfhandle(s.fileno()) for s in (stdin, stdout, stderr)]
                previous = [os.get_handle_inheritable(handle) for handle in handles]
                startup = subprocess.STARTUPINFO()
                startup.dwFlags = subprocess.STARTF_USESTDHANDLES | subprocess.STARTF_USESHOWWINDOW
                startup.wShowWindow = subprocess.SW_HIDE
                startup.hStdInput, startup.hStdOutput, startup.hStdError = handles
                startup.lpAttributeList = {"handle_list": handles}
                try:
                    for handle in handles:
                        os.set_handle_inheritable(handle, True)
                    self.handle, self.thread, self.pid, _ = _winapi.CreateProcess(
                        argv[0], subprocess.list2cmdline(argv), None, None, True,
                        0x00000004 | subprocess.CREATE_NEW_CONSOLE,  # SUSPENDED, hidden console.
                        None, str(cwd), startup,
                    )
                finally:
                    for handle, flag in zip(handles, previous):
                        os.set_handle_inheritable(handle, flag)
            # No workload instruction executes before successful containment.
            self.api.check(self.api.k.AssignProcessToJobObject(self.job, self.handle), "AssignProcessToJobObject")
        except BaseException as error:
            # A constructor can fail after CreateProcess but before assigning the
            # caller's backend variable. Preserve the partial launch's cleanup
            # evidence instead of reporting that no process was ever created.
            error.workload_pid = self.pid
            error.cleanup_complete = self.handle is None
            try:
                if self.handle is not None:
                    _winapi.TerminateProcess(self.handle, 2)
                    error.cleanup_complete = _winapi.WaitForSingleObject(self.handle, 5000) == 0
            except BaseException as cleanup_error:
                error.add_note(f"partial launch cleanup: {cleanup_error}")
                error.cleanup_complete = False
            finally:
                self.close()
            raise

    def start(self) -> None:
        if self.api.k.ResumeThread(self.thread) != 1:
            raise RuntimeError("ResumeThread did not resume the singly suspended workload")
        self.native.CloseHandle(self.thread)
        self.thread = None

    def poll(self):
        if self.returncode is None and self.native.WaitForSingleObject(self.handle, 0) == 0:
            self.returncode = self.native.GetExitCodeProcess(self.handle)
        return self.returncode

    def pids(self) -> list[int]:
        capacity = 64
        while capacity <= 1048576:
            buffer = ctypes.create_string_buffer(8 + ctypes.sizeof(ctypes.c_size_t) * capacity)
            if self.api.k.QueryInformationJobObject(self.job, 3, buffer, len(buffer), None):
                count = ctypes.c_ulong.from_buffer(buffer, 4).value
                array = (ctypes.c_size_t * count).from_buffer(buffer, 8)
                return sorted(array)
            if ctypes.get_last_error() != 234:  # ERROR_MORE_DATA
                raise ctypes.WinError(ctypes.get_last_error(), "QueryInformationJobObject process list")
            capacity *= 2
        raise RuntimeError("Job process list exceeds supported size")

    def sample(self) -> dict:
        rss = 0
        members = self.pids()
        for pid in members:
            handle = self.api.k.OpenProcess(0x0400 | 0x0010, False, pid)
            if not handle:
                if pid not in self.pids():
                    continue  # Exited after the snapshot.
                raise ctypes.WinError(ctypes.get_last_error(), f"OpenProcess {pid}")
            try:
                current, _ = self.api.process_memory(handle)
                rss += current
            finally:
                self.api.k.CloseHandle(handle)
        _, peak = self.api.process_memory(self.handle)
        limits = self.api.ExtendedLimits()
        self.api.check(self.api.k.QueryInformationJobObject(
            self.job, 9, ctypes.byref(limits), ctypes.sizeof(limits), None), "QueryInformationJobObject peak memory")
        return {
            "pids": members, "tree_resident_bytes": rss,
            "root_os_peak_resident_bytes": peak,
            "root_os_peak_source": "GetProcessMemoryInfo.PeakWorkingSetSize",
            "job_os_peak_commit_bytes": limits.PeakJobMemoryUsed,
        }

    def graceful(self) -> dict:
        members = self.pids()
        if not members:
            return {"method": "CTRL_BREAK_dedicated_console", "delivered": False, "already_empty": True}
        # Attaching the supervisor itself would change its own console/signals.
        # A disposable, bounded helper checks membership before console-wide delivery.
        targets = sorted(members, key=lambda pid: pid != self.pid)
        deadline = time.monotonic() + 3
        failures = []
        for target in targets:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            command = [sys.executable, str(Path(__file__).resolve()), "--_windows-console-event",
                       str(target), ",".join(map(str, self.pids()))]
            result = subprocess.run(
                command, stdin=subprocess.DEVNULL, capture_output=True, text=True,
                shell=False, creationflags=subprocess.CREATE_NO_WINDOW, timeout=remaining,
            )
            if result.returncode == 0:
                return json.loads(result.stdout)
            failures.append(result.stderr.strip())
        raise RuntimeError("CTRL_BREAK helper failed: " + "; ".join(failures))

    def kill(self) -> None:
        self.api.check(self.api.k.TerminateJobObject(self.job, 137), "TerminateJobObject")

    def close(self) -> None:
        if self.job is not None:
            self.api.k.CloseHandle(self.job)  # Safety net even if another operation failed.
            self.job = None
        for name in ("thread", "handle"):
            handle = getattr(self, name, None)
            if handle is not None:
                self.native.CloseHandle(handle)
                setattr(self, name, None)


def windows_console_event(target: int, allowed: set[int]) -> int:
    """Internal helper: never signal a console containing an unrelated client."""
    api = WindowsAPI()
    api.k.FreeConsole()
    api.check(api.k.AttachConsole(target), "AttachConsole")
    handler_type = ctypes.WINFUNCTYPE(api.w.BOOL, api.w.DWORD)
    handler = handler_type(lambda event: True)
    api.check(api.k.SetConsoleCtrlHandler(handler, True), "SetConsoleCtrlHandler")
    try:
        capacity = max(64, len(allowed) + 2)
        while True:
            pids = (api.w.DWORD * capacity)()
            count = api.check(api.k.GetConsoleProcessList(pids, capacity), "GetConsoleProcessList")
            if count <= capacity:
                break
            capacity = count
        actual = set(pids[:count])
        unrelated = actual - allowed - {os.getpid()}
        if unrelated:
            raise RuntimeError(f"refusing CTRL_BREAK: unrelated console PIDs {sorted(unrelated)}")
        api.check(api.k.GenerateConsoleCtrlEvent(1, 0), "GenerateConsoleCtrlEvent CTRL_BREAK")
        time.sleep(0.05)  # Keep the handler alive until this helper receives the event.
        print(json.dumps({"method": "CTRL_BREAK_dedicated_console", "delivered": True,
                          "console_pids": sorted(actual - {os.getpid()})}))
        return 0
    finally:
        api.k.FreeConsole()


@contextlib.contextmanager
def signal_requests():
    requests = []
    old = {}

    def handler(number, frame):
        if not requests:
            requests.append(signal.Signals(number).name)

    for name in ("SIGINT", "SIGTERM", "SIGBREAK"):
        if hasattr(signal, name):
            number = getattr(signal, name)
            old[number] = signal.signal(number, handler)
    try:
        yield requests
    finally:
        for number, previous in old.items():
            signal.signal(number, previous)


def supervise(args) -> int:
    record_path = args.record.resolve()
    record_path.parent.mkdir(parents=True, exist_ok=True)
    outputs = {
        "stdout": (args.stdout or record_path.with_suffix(".stdout.log")).resolve(),
        "stderr": (args.stderr or record_path.with_suffix(".stderr.log")).resolve(),
        "samples": (args.samples or record_path.with_suffix(".samples.jsonl")).resolve(),
    }
    paths = [record_path, *outputs.values()]
    if len(set(map(os.path.normcase, map(str, paths)))) != len(paths):
        raise ValueError("record, stdout, stderr and samples paths must be distinct")
    record = {
        "schema": SCHEMA, "state": "preparing", "created_at": utc_now(),
        "argv": args.command, "cwd": str(args.cwd.resolve()), "shell": False,
        "limits": {
            "timeout_seconds": args.timeout_seconds, "grace_seconds": args.grace_seconds,
            "kill_wait_seconds": args.kill_wait_seconds, "poll_seconds": args.poll_seconds,
            "memory_limit_bytes": args.memory_limit_bytes,
            "min_free_memory_bytes": args.min_free_memory_bytes,
            "disk_reserve_bytes": args.disk_reserve_bytes,
        },
        "runtime": {"python": sys.version, "platform": platform.platform(),
                    "machine": platform.machine(), "logical_cpus": os.cpu_count()},
        "outputs": {key: {"path": str(path)} for key, path in outputs.items()},
        "identity_before": [], "identity_after": [], "identity_unchanged": None,
        "stop_reason": None, "stop_requested_at": None, "events": [], "errors": [],
        "child_exit_code": None, "cleanup_complete": None, "forced": False,
        "measurement": {
            "interval": "before_process_creation_to_root_reaped_and_containment_empty",
            "sample_count": 0, "max_sample_gap_seconds": 0.0,
            "sampled_peak_tree_resident_bytes": 0,
            "root_os_peak_resident_bytes": None, "root_os_peak_source": None,
            "job_os_peak_commit_bytes": None, "max_observed_processes": 0,
        },
    }
    atomic_json(record_path, record, initial=True)
    backend = None
    started = None
    finished = None
    first_stop = None
    code = 2
    previous_sample = None
    created_outputs = set()
    forced_deadline = None

    def stop(reason: str) -> None:
        nonlocal first_stop
        if record["stop_reason"] is None:
            first_stop = time.monotonic()
            record["stop_reason"] = reason
            record["stop_requested_at"] = utc_now()
            record["events"].append({"kind": "stop_requested", "reason": reason,
                                     "elapsed_seconds": None if started is None else first_stop - started})

    def note_error(error: BaseException, where: str) -> None:
        record["errors"].append({"where": where, "type": type(error).__name__, "message": str(error)})

    def sample(stream) -> dict:
        nonlocal previous_sample
        now = time.monotonic()
        item = backend.sample()
        host = backend.api.memory() if os.name == "nt" else linux_memory()
        item.update({"elapsed_seconds": now - started, "at": utc_now(),
                     "host_available_memory_bytes": host["available_bytes"],
                     "disk_free_bytes": shutil.disk_usage(disk_path).free})
        measure = record["measurement"]
        measure["sample_count"] += 1
        if previous_sample is not None:
            measure["max_sample_gap_seconds"] = max(measure["max_sample_gap_seconds"], now - previous_sample)
        previous_sample = now
        measure["sampled_peak_tree_resident_bytes"] = max(
            measure["sampled_peak_tree_resident_bytes"], item["tree_resident_bytes"])
        measure["max_observed_processes"] = max(measure["max_observed_processes"], len(item["pids"]))
        for key in ("root_os_peak_resident_bytes", "root_os_peak_source", "job_os_peak_commit_bytes"):
            if item[key] is not None:
                measure[key] = item[key]
        record["last_sample"] = item
        stream.write(json.dumps(item, allow_nan=False) + "\n")
        stream.flush()
        return item

    try:
        if platform.python_implementation() != "CPython":
            raise RuntimeError("only CPython is supported")
        if os.name == "nt":
            process_type = WindowsProcess
        elif sys.platform == "linux":
            process_type = LinuxProcess
        else:
            raise RuntimeError("containment/metrics supported only on Windows and Linux")
        record["containment"] = process_type.containment
        record["host_before"] = process_type.preflight()
        cwd = args.cwd.resolve(strict=True)
        if not cwd.is_dir():
            raise ValueError("cwd must be a directory")
        executable = resolve_executable(args.command[0], cwd)
        argv = [str(executable), *args.command[1:]]
        record["resolved_argv"] = argv
        identity_paths = list(dict.fromkeys([
            executable, Path(sys.executable).resolve(), Path(__file__).resolve(),
            *(path.resolve() for path in args.identity_file),
        ]))
        if any(path in paths for path in identity_paths):
            raise ValueError("an output path aliases an executable or identity file")
        record["identity_before"] = [identity(path) for path in identity_paths]
        # Hashing large inputs can take time; reserve checks use a fresh value.
        record["host_before"] = process_type.preflight()
        disk_path = (args.disk_path or outputs["stdout"].parent).resolve(strict=True)
        record["disk_path"] = str(disk_path)
        record["disk_free_before_bytes"] = shutil.disk_usage(disk_path).free
        if args.min_free_memory_bytes is not None and record["host_before"]["available_bytes"] < args.min_free_memory_bytes:
            stop("host_memory_reserve")
        if args.disk_reserve_bytes is not None and record["disk_free_before_bytes"] < args.disk_reserve_bytes:
            stop("disk_reserve")
        if record["stop_reason"]:
            record["state"] = "resource_exceeded"
            code = 125
        else:
            with contextlib.ExitStack() as stack:
                for path in outputs.values():
                    path.parent.mkdir(parents=True, exist_ok=True)
                for path in outputs.values():
                    if path.exists():
                        raise FileExistsError(f"output already exists: {path}")
                stdout = stack.enter_context(outputs["stdout"].open("xb", buffering=0))
                created_outputs.add("stdout")
                stderr = stack.enter_context(outputs["stderr"].open("xb", buffering=0))
                created_outputs.add("stderr")
                samples = stack.enter_context(outputs["samples"].open("x", encoding="utf-8", newline="\n"))
                created_outputs.add("samples")
                requests = stack.enter_context(signal_requests())
                started = time.monotonic()
                record["started_at"] = utc_now()
                backend = process_type(argv, cwd, stdout, stderr)
                record["pid"] = backend.pid
                record["state"] = "running"
                atomic_json(record_path, record)
                backend.start()
                graceful_attempted = False
                forced_at = None
                root_exit_at = None
                while True:
                    exit_code = backend.poll()
                    record["child_exit_code"] = exit_code
                    current = sample(samples)
                    now = time.monotonic()
                    if requests:
                        stop("signal:" + requests[0])
                    if exit_code is not None and not current["pids"]:
                        record["cleanup_complete"] = True
                        finished = now
                        break
                    if exit_code is not None and root_exit_at is None:
                        root_exit_at = now
                    if record["stop_reason"] is None:
                        if now - started >= args.timeout_seconds:
                            stop("timeout")
                        elif args.memory_limit_bytes is not None and current["tree_resident_bytes"] > args.memory_limit_bytes:
                            stop("tree_memory_limit")
                        elif args.min_free_memory_bytes is not None and current["host_available_memory_bytes"] < args.min_free_memory_bytes:
                            stop("host_memory_reserve")
                        elif args.disk_reserve_bytes is not None and current["disk_free_bytes"] < args.disk_reserve_bytes:
                            stop("disk_reserve")
                        elif root_exit_at is not None and now - root_exit_at >= args.grace_seconds:
                            # Windows may still have its console host in the Job
                            # for a moment after the workload exits. Allow a finite
                            # natural drain, then stop actual surviving children.
                            stop("descendants_after_root_exit")
                    if record["stop_reason"] is not None:
                        if not graceful_attempted:
                            graceful_attempted = True
                            try:
                                event = backend.graceful()
                                record["events"].append({"kind": "graceful", **event})
                            except Exception as error:
                                note_error(error, "graceful_signal")
                            atomic_json(record_path, record)
                        if forced_at is None and time.monotonic() - first_stop >= args.grace_seconds:
                            backend.kill()
                            forced_at = time.monotonic()
                            forced_deadline = forced_at + args.kill_wait_seconds
                            record["forced"] = True
                            record["events"].append({"kind": "forced_termination", "at": utc_now()})
                        if forced_at is not None and time.monotonic() - forced_at >= args.kill_wait_seconds:
                            raise RuntimeError("containment still nonempty after forced termination deadline")
                    time.sleep(args.poll_seconds)
                if record["stop_reason"] is None:
                    record["stop_reason"] = "completed" if exit_code == 0 else "child_failed"
                reason = record["stop_reason"]
                if reason == "completed":
                    record["state"], code = "completed", 0
                elif reason == "timeout":
                    record["state"], code = "timeout", 124
                elif reason in {"tree_memory_limit", "host_memory_reserve", "disk_reserve"}:
                    record["state"], code = "resource_exceeded", 125
                elif reason.startswith("signal:"):
                    record["state"], code = "interrupted", 130
                else:
                    record["state"], code = "failed", 1
    except BaseException as error:
        stop("supervisor_error")
        note_error(error, "supervision")
        record["state"] = "supervisor_error"
        code = 2
        if getattr(error, "workload_pid", None) is not None:
            record["pid"] = error.workload_pid
            record["cleanup_complete"] = error.cleanup_complete
    finally:
        if backend is not None:
            # Any exception (including record/log/metric failure) first stops the
            # whole containment, before hashing or writing another final record.
            try:
                if record["cleanup_complete"] is not True:
                    backend.kill()
                    record["forced"] = True
                    deadline = forced_deadline or time.monotonic() + args.kill_wait_seconds
                    while True:
                        record["child_exit_code"] = backend.poll()
                        last = backend.sample()
                        empty = record["child_exit_code"] is not None and not last["pids"]
                        if empty or time.monotonic() >= deadline:
                            record["cleanup_complete"] = empty
                            break
                        time.sleep(min(args.poll_seconds, 0.1))
                    if not empty:
                        raise RuntimeError("could not verify empty containment")
            except BaseException as error:
                note_error(error, "cleanup")
                record["cleanup_complete"] = False
                record["state"], code = "supervisor_error", 2
            finally:
                try:
                    backend.close()
                except BaseException as error:
                    note_error(error, "close_containment")
                    record["state"], code = "supervisor_error", 2
                if finished is None:
                    finished = time.monotonic()
        elif record["cleanup_complete"] is None:
            record["cleanup_complete"] = True  # No successfully created workload.
        if started is not None:
            record["elapsed_seconds"] = (finished or time.monotonic()) - started
        record["ended_at"] = utc_now()
        if record["identity_before"]:
            for before in record["identity_before"]:
                try:
                    record["identity_after"].append(identity(Path(before["path"])))
                except BaseException as error:
                    note_error(error, "identity_after")
            record["identity_unchanged"] = record["identity_before"] == record["identity_after"]
            if not record["identity_unchanged"]:
                record["state"], code = "supervisor_error", 2
                if record["stop_reason"] == "completed":
                    record["events"].append({"kind": "identity_changed_after_completion"})
        for key, path in outputs.items():
            if key in created_outputs:
                try:
                    record["outputs"][key] = identity(path)
                except BaseException as error:
                    note_error(error, "output_hash")
                    record["state"], code = "supervisor_error", 2
        record["supervisor_exit_code"] = code
        atomic_json(record_path, record)
    return code


def positive_float(value: str) -> float:
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("must be finite and positive")
    return number


def nonnegative_int(value: str) -> int:
    number = int(value)
    if number < 0:
        raise argparse.ArgumentTypeError("must be nonnegative")
    return number


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--record", type=Path, required=True)
    result.add_argument("--stdout", type=Path)
    result.add_argument("--stderr", type=Path)
    result.add_argument("--samples", type=Path, help="JSONL resource samples (default: RECORD.samples.jsonl)")
    result.add_argument("--cwd", type=Path, default=Path.cwd())
    result.add_argument("--timeout-seconds", type=positive_float, required=True)
    result.add_argument("--grace-seconds", type=positive_float, default=10.0)
    result.add_argument("--kill-wait-seconds", type=positive_float, default=5.0)
    result.add_argument("--poll-seconds", type=positive_float, default=0.25)
    result.add_argument("--memory-limit-bytes", type=nonnegative_int)
    result.add_argument("--min-free-memory-bytes", type=nonnegative_int)
    result.add_argument("--disk-reserve-bytes", type=nonnegative_int)
    result.add_argument("--disk-path", type=Path)
    result.add_argument("--identity-file", type=Path, action="append", default=[])
    result.add_argument("command", nargs=argparse.REMAINDER)
    return result


def main(argv=None) -> int:
    if argv is None:
        argv = sys.argv[1:]
    if len(argv) == 3 and argv[0] == "--_windows-console-event":
        try:
            if os.name != "nt":
                raise RuntimeError("Windows console helper invoked on another OS")
            return windows_console_event(int(argv[1]), {int(pid) for pid in argv[2].split(",")})
        except Exception as error:
            print(str(error), file=sys.stderr)
            return 2
    argument_parser = parser()
    args = argument_parser.parse_args(argv)
    if args.command and args.command[0] == "--":
        args.command.pop(0)
    if not args.command:
        argument_parser.error("an executable and its arguments are required after --")
    if args.poll_seconds > 1:
        argument_parser.error("--poll-seconds must be <= 1")
    try:
        return supervise(args)
    except Exception as error:
        print(f"supervisor: {type(error).__name__}: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
