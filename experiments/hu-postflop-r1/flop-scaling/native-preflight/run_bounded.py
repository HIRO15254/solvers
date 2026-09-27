"""Research-only Windows preflight; no production supervisor modifications.

Delegates the normal run_supervised.py CLI after an exact pinned, literal
in-memory transformation. The Job has a fixed 512 MiB aggregate committed-memory
limit, including descendants; allocation beyond that limit fails. This is not
an RSS limit or an automatic memory-limit termination reason. Ordinary sampled
working-set logs and optional --memory-limit-bytes retain their original meaning.
The root is created BELOW_NORMAL, suspended, assigned, checked, then resumed.
Default child priority inherits BELOW_NORMAL; this is not a child priority ceiling.
Fresh available commit must be >= 1 GiB reserve + 512 MiB before launch. No live
host-commit polling is added. The supervisor/console-signal helper are outside
the workload Job, as in the original supervisor. No CPU cap is imposed.

Use --calibration-plan to print proposed commands, without launching them.
Native calibration results are recorded separately; this file alone is not
evidence of execution.

Primary Windows specifications:
https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information
https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information
https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw
https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getpriorityclass
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import sys

BASE_SHA256 = "5bd46e106bbc48e971c43c1ea080ed16e22356e83747ec6fb57157013fd029b8"
JOB_COMMIT_BYTES = 512 * 1024 * 1024
MIN_AVAILABLE_COMMIT_BYTES = 1024 * 1024 * 1024 + JOB_COMMIT_BYTES
BELOW_NORMAL = 0x00004000
JOB_FLAGS = 0x00002000 | 0x00000200  # KILL_ON_JOB_CLOSE | JOB_MEMORY; no breakaway.
HERE = Path(__file__).resolve()
ROOT = HERE.parents[4]
BASE = ROOT / "tools/run_supervised.py"


def transformed_source(raw: bytes) -> str:
    if hashlib.sha256(raw).hexdigest() != BASE_SHA256:
        raise ValueError("Pinned tools/run_supervised.py source differs")
    text = raw.decode("utf-8").replace("\r\n", "\n")

    def replace(old: str, new: str) -> None:
        nonlocal text
        if text.count(old) != 1:
            raise ValueError("Supervisor transformation anchor differs: " + old[:80])
        text = text.replace(old, new, 1)

    replace(
        '            "ResumeThread": ([w.HANDLE], w.DWORD),',
        '            "ResumeThread": ([w.HANDLE], w.DWORD),\n'
        '            "GetPriorityClass": ([w.HANDLE], w.DWORD),',
    )
    replace(
        '            limits.BasicLimitInformation.LimitFlags = 0x2000  # KILL_ON_JOB_CLOSE; no breakaway.',
        '            limits.BasicLimitInformation.LimitFlags = BOUND_JOB_FLAGS\n'
        '            limits.JobMemoryLimit = BOUND_JOB_COMMIT_BYTES',
    )
    replace(
        '                self.job, 9, ctypes.byref(limits), ctypes.sizeof(limits)), "SetInformationJobObject")',
        '''                self.job, 9, ctypes.byref(limits), ctypes.sizeof(limits)), "SetInformationJobObject")
            confirmed = self.api.ExtendedLimits()
            self.api.check(self.api.k.QueryInformationJobObject(
                self.job, 9, ctypes.byref(confirmed), ctypes.sizeof(confirmed), None),
                "QueryInformationJobObject hard limits")
            if (confirmed.BasicLimitInformation.LimitFlags != BOUND_JOB_FLAGS
                    or confirmed.JobMemoryLimit != BOUND_JOB_COMMIT_BYTES):
                raise RuntimeError("Job hard limit query differs before workload creation")
            self.bounded_job_settings = {
                "limit_flags": confirmed.BasicLimitInformation.LimitFlags,
                "job_memory_limit_bytes": confirmed.JobMemoryLimit,
                "root_priority_class": None,
                "verified_before_resume": False,
            }''',
    )
    replace(
        '                        0x00000004 | subprocess.CREATE_NEW_CONSOLE,  # SUSPENDED, hidden console.',
        '                        0x00000004 | subprocess.CREATE_NEW_CONSOLE | BOUND_BELOW_NORMAL,',
    )
    replace(
        '            self.api.check(self.api.k.AssignProcessToJobObject(self.job, self.handle), "AssignProcessToJobObject")',
        '''            self.api.check(self.api.k.AssignProcessToJobObject(self.job, self.handle), "AssignProcessToJobObject")
            priority = self.api.check(self.api.k.GetPriorityClass(self.handle), "GetPriorityClass")
            if priority != BOUND_BELOW_NORMAL:
                raise RuntimeError("Root process priority differs before resume")
            self.bounded_job_settings["root_priority_class"] = priority
            self.bounded_job_settings["verified_before_resume"] = True''',
    )
    replace(
        '            "RAM trigger is sampled working set, not a hard allocation limit",',
        '            "Optional RAM trigger is sampled working set; separate fixed Job limit caps committed memory",',
    )
    replace(
        '            "memory_limit_bytes": args.memory_limit_bytes,',
        '''            "memory_limit_bytes": args.memory_limit_bytes,
            "hard_job_commit_limit_bytes": BOUND_JOB_COMMIT_BYTES,
            "minimum_available_commit_before_launch_bytes": BOUND_MIN_COMMIT_BYTES,
            "root_priority_class": BOUND_BELOW_NORMAL,''',
    )
    replace(
        '        "identity_before": [], "identity_after": [], "identity_unchanged": None,',
        '''        "identity_before": [], "identity_after": [], "identity_unchanged": None,
        "research_wrapper": BOUND_PROVENANCE,
        "bounded_job_settings": None,''',
    )
    replace(
        '        # Hashing large inputs can take time; reserve checks use a fresh value.\n'
        '        record["host_before"] = process_type.preflight()',
        '''        # Hashing large inputs can take time; reserve checks use a fresh value.
        record["host_before"] = process_type.preflight()
        if record["host_before"]["commit_available_bytes"] < BOUND_MIN_COMMIT_BYTES:
            stop("host_commit_reserve")''',
    )
    replace(
        '                record["pid"] = backend.pid',
        '                record["pid"] = backend.pid\n'
        '                record["bounded_job_settings"] = backend.bounded_job_settings',
    )
    replace(
        'elif reason in {"tree_memory_limit", "host_memory_reserve", "disk_reserve"}:',
        'elif reason in {"tree_memory_limit", "host_memory_reserve", "host_commit_reserve", "disk_reserve"}:',
    )
    return text


def calibration_plan() -> dict:
    """Pure description. VirtualAlloc requests commit but never touches pages."""
    setup = '''import ctypes, json
k = ctypes.WinDLL("kernel32", use_last_error=True)
k.VirtualAlloc.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_ulong, ctypes.c_ulong]
k.VirtualAlloc.restype = ctypes.c_void_p
k.VirtualFree.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_ulong]
k.VirtualFree.restype = ctypes.c_int
'''
    def allocation_code(amount: int, expected_success: bool) -> str:
        return setup + f'''amount = {amount}
p = k.VirtualAlloc(None, amount, 0x3000, 0x04)
error = ctypes.get_last_error() if not p else 0
print(json.dumps({{"requested_commit_bytes": amount, "allocation_succeeded": bool(p), "winerror": error}}), flush=True)
released = not p or bool(k.VirtualFree(p, 0, 0x8000))
expected = bool(p) if {expected_success!r} else not p and error != 0
raise SystemExit(0 if expected and released else 3)
'''

    def command(name: str, code: str, expected: object) -> dict:
        return {
            "name": name,
            "argv": [sys.executable, "-B", str(HERE), "--record",
                     str(ROOT / "runs/flop-native-calibration" / name / "record.json"),
                     "--timeout-seconds", "10", "--grace-seconds", "0.2",
                     "--kill-wait-seconds", "2", "--poll-seconds", "0.05",
                     "--", sys.executable, "-B", "-c", code],
            "expected_supervisor_exit": 0,
            "expected_allocation_success": expected,
        }

    commands = [command(name, allocation_code(amount, expected), expected)
                for name, amount, expected in [
                    ("below-cap", 1024 * 1024, True),
                    ("above-cap", JOB_COMMIT_BYTES + 65536, False),
                ]]
    child_code = allocation_code(256 * 1024 * 1024, False)
    aggregate_code = setup + f'''import subprocess, sys
amount = {300 * 1024 * 1024}
p = k.VirtualAlloc(None, amount, 0x3000, 0x04)
print(json.dumps({{"parent_requested_commit_bytes": amount, "parent_allocation_succeeded": bool(p)}}), flush=True)
if not p:
    raise SystemExit(4)
try:
    result = subprocess.run([sys.executable, "-B", "-c", {child_code!r}],
        capture_output=True, text=True, timeout=5, creationflags=0x08000000)
    print(json.dumps({{"child_exit": result.returncode, "child_stdout": result.stdout, "child_stderr": result.stderr}}), flush=True)
finally:
    released = bool(k.VirtualFree(p, 0, 0x8000))
raise SystemExit(0 if result.returncode == 0 and released else 5)
'''
    commands.append(command("aggregate-child-cap", aggregate_code,
                            {"parent_300_mib": True, "child_256_mib": False}))
    return {
        "schema": "r1.native-preflight-calibration-plan/v1",
        "execution": "not_run",
        "job_commit_bytes": JOB_COMMIT_BYTES,
        "minimum_available_commit_bytes": MIN_AVAILABLE_COMMIT_BYTES,
        "checks": ["Fresh resource probe; calibration commands run serially, without this task's compiler/solver runs",
                   "Each record: completed, cleanup_complete=true, unchanged identities",
                   "Queried job limit 536870912, flags 8704, root priority 16384, verified_before_resume=true",
                   "Below-cap allocation succeeds; above-cap returns NULL and nonzero Windows error",
                   "While parent holds 300MiB commit, child 256MiB request returns NULL/nonzero error; both requests individually fit the Job cap",
                   "Job commit counter and sampled working-set counter remain distinct",
                   "Aggregate test supports child containment; allocation error alone does not prove its unique cause",
                   "Not a timeout, CPU isolation, or performance calibration"],
        "commands": commands,
    }


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv == ["--calibration-plan"]:
        print(json.dumps(calibration_plan(), indent=2))
        return 0
    if os.name != "nt":
        raise RuntimeError("This bounded research wrapper requires Windows")
    raw = BASE.read_bytes()
    source = transformed_source(raw)
    namespace = {
        "__name__": "_r1_bounded_supervisor", "__file__": str(BASE),
        "BOUND_JOB_FLAGS": JOB_FLAGS, "BOUND_JOB_COMMIT_BYTES": JOB_COMMIT_BYTES,
        "BOUND_MIN_COMMIT_BYTES": MIN_AVAILABLE_COMMIT_BYTES,
        "BOUND_BELOW_NORMAL": BELOW_NORMAL,
        "BOUND_PROVENANCE": {
            "wrapper_path": str(HERE), "wrapper_sha256": hashlib.sha256(HERE.read_bytes()).hexdigest(),
            "base_path": str(BASE), "base_sha256": BASE_SHA256,
            "transformed_source_sha256": hashlib.sha256(source.encode()).hexdigest(),
        },
    }
    exec(compile(source, str(BASE) + " [bounded research transform]", "exec"), namespace)
    return namespace["main"](["--identity-file", str(HERE), *argv])


if __name__ == "__main__":
    raise SystemExit(main())
