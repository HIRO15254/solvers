"""VM17 completed-prefix integrity only; no timing summary or adoption decision.

Execute on the authorized Linux recovery host. Measurement identity is read
from immutable evidence; the recovery host may be a different, smaller boot.
"""
import argparse
import datetime as dt
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys

PACKAGE = Path("/opt/r1/flop-fused-update-package")
TIMING = PACKAGE / "experiments/hu-postflop-r1/flop-scaling/fused-update/timing"
PROOF = Path("/opt/r1/flop-fused-update-proof01")
REPORT = Path("/tmp/flop-fused-update-partial01.json")
UNIT = "solvers-r1-vm17-fused32.service"
PINS = {"run.py": "4d7fbb89dfe90a319dd70640ae1f30f038f8ac257ae04d6f5917db028a373fdd",
        "analyze.py": "6ce2406ef39432a1ccf1a0394790d0d02db3d48115afe2f19b477db51c186eaf"}


def need(value, message):
    if not value:
        raise ValueError(message)


def pin(path):
    data = path.read_bytes()
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def prefix_length(rows, expected, status):
    need(status in {"failed", "running"}, "use the complete reader for a completed run")
    need(len(rows) == len(expected), "fixed stage inventory missing")
    length = 0
    for row, wanted in zip(rows, expected):
        need(all(row.get(k) == v for k, v in wanted.items()), "fixed stage condition/order differs")
        if row.get("status") == "completed":
            need(length == rows.index(row), "completed stage after incomplete stage")
            length += 1
        else:
            need(row.get("status") in {"failed", "skipped", "running", "pending"}, "unknown incomplete state")
    suffix = rows[length:]
    if status == "failed" and suffix:
        need(suffix[0]["status"] == "failed" and all(r["status"] == "skipped" for r in suffix[1:]), "failed terminal suffix differs")
    elif status == "running" and suffix:
        need(suffix[0]["status"] in {"running", "pending"} and all(r["status"] == "pending" for r in suffix[1:]), "interrupted suffix differs")
    need(length >= 4, "toolchain, both builds and candidate tests must be complete")
    return length


def quiescence():
    command = ["/usr/bin/systemctl", "show", UNIT, "--property=LoadState,ActiveState,SubState,MainPID,ExecMainStatus,Result,ControlGroup"]
    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
    fields = {}
    for line in result.stdout.splitlines():
        key, value = line.split("=", 1)
        need(key not in fields, "duplicate unit property")
        fields[key] = value
    need(fields.get("LoadState") in {"loaded", "not-found"}
         and fields.get("ActiveState") in {"inactive", "failed"} and fields.get("MainPID") == "0", "measurement unit not quiescent")
    need(result.returncode == 0 or fields["LoadState"] == "not-found", "unit query failed")
    checked_cgroups = []
    if fields.get("ControlGroup"):
        cgroup_root = Path("/sys/fs/cgroup").resolve(strict=True)
        group = (cgroup_root / fields["ControlGroup"].lstrip("/")).resolve()
        need(group != cgroup_root and group.is_relative_to(cgroup_root), "unsafe unit cgroup path")
        for path in sorted(group.rglob("cgroup.procs")):
            need(not path.read_text().strip(), "measurement cgroup still contains descendant processes")
            checked_cgroups.append(str(path))
    return {"argv": command, "exit_code": result.returncode, "stdout": result.stdout,
            "stderr": result.stderr, "properties": fields, "empty_cgroup_process_files": checked_cgroups}


def audit():
    need(sys.platform == "linux", "authorized Linux recovery host required")
    sys.dont_write_bytecode = True
    before = quiescence()
    for name, sha in PINS.items():
        need(pin(TIMING / name)["sha256"] == sha, "frozen trusted helper differs: " + name)
    spec = importlib.util.spec_from_file_location("vm17_partial_trusted_analysis", TIMING / "analyze.py")
    analysis = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analysis)
    run = analysis.run
    # Evidence checks exact whole-proof membership/bytes, even for failed rows.
    # No salvage, recovery-code import, or replacement of missing original data.
    e = analysis.Evidence(PROOF)
    analysis.verify_plan(e)
    execution = analysis.read(PROOF / "execution.json")
    need(execution["schema"] == "r1.fused-update-timing-execution/v1"
         and execution["plan"] == e.files["plan.json"], "execution/plan binding differs")
    expected = [{"name": "toolchain", "kind": "toolchain"}]
    expected += [{"name": "build-" + arm, "kind": "build", "arm": arm} for arm in run.ARMS]
    expected += [{"name": "tests-candidate", "kind": "tests", "arm": "candidate"}]
    expected += run.schedule()
    rows = execution["stages"]
    prefix = prefix_length(rows, expected, execution["status"])
    observed = {s: sum(r["status"] == s for r in rows) for s in ("completed", "failed", "skipped", "running", "pending")}
    if "counts" in execution:
        need(execution["counts"] == {s: observed[s] for s in ("completed", "failed", "skipped")}, "recorded terminal counts differ")
    need(analysis.utc(e.plan["created_at"]) <= analysis.utc(execution["started_at"]), "execution start precedes plan")
    if "ended_at" in execution:
        need(analysis.utc(execution["started_at"]) <= analysis.utc(execution["ended_at"]), "execution end precedes start")
    binaries = analysis.verify_build(e, execution)
    canonical = {}
    previous = analysis.utc(rows[3]["verified_at"])
    for row in rows[4:prefix]:
        need(previous <= analysis.utc(row["started_at"]), "completed solve ordering overlaps")
        key = run.canonical_key(row)
        canonical[key] = analysis.verify_solve(e, row, binaries, canonical.get(key))
        previous = analysis.utc(row["verified_at"])
    need(canonical == execution["canonical"], "canonical inventory exceeds or differs from completed prefix")
    if "ended_at" in execution:
        need(previous <= analysis.utc(execution["ended_at"]), "terminal timestamp precedes prefix completion")
    # Recheck immutable evidence membership after streaming all canonical bytes.
    again = analysis.Evidence(PROOF)
    need(again.files == e.files and again.plan == e.plan, "evidence changed during audit")
    after = quiescence()
    need(after["properties"] == before["properties"], "unit state changed during audit")
    return {"schema": "r1.fused-update-partial-integrity/v1", "status": "partial_verified",
            "payload_integrity": "verified", "performance_claims": False,
            "performance_screen": "not_evaluable", "production_adoption": False,
            "quality_certification": False, "groups": [], "source": pin(Path(__file__)),
            "trusted_helpers": {n: pin(TIMING / n) for n in PINS},
            "plan": e.files["plan.json"], "execution": e.files["execution.json"],
            "retained_manifest": pin(PROOF / "retained.json"), "proof_files": len(e.files),
            "measurement_boot_id": e.plan["host"]["boot_id"],
            "recovery_boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip(),
            "original_execution_status": execution["status"], "counts": observed,
            "completed_prefix_stages": prefix,
            "verified_completed": [{"name": r["name"], "kind": r["kind"], "completion": r["completion"], "record": r["record"]} for r in rows[:prefix]],
            "incomplete_stage_metadata": rows[prefix:],
            "original_failure_metadata": {k: execution[k] for k in ("error", "traceback", "retention_manifest_error", "failed_state_capture", "failed_state_capture_error", "ended_at") if k in execution},
            "canonical_streams_verified": len(e.states), "quiescence_before": before, "quiescence_after": after,
            "scope": "Exact completed-prefix source/build/test/host/state/quality integrity only. Incomplete rows have retained bytes but no successful-solve claim. No timing aggregation, certification, sample replacement, or adoption."}


def self_test():
    expected = [{"name": str(i), "kind": "test"} for i in range(7)]
    rows = [{**r, "status": "completed" if i < 5 else "failed" if i == 5 else "skipped"} for i, r in enumerate(expected)]
    assert prefix_length(rows, expected, "failed") == 5
    interrupted = [{**r, "status": "completed" if i < 4 else "running" if i == 4 else "pending"} for i, r in enumerate(expected)]
    assert prefix_length(interrupted, expected, "running") == 4
    for bad in (rows[:-1], [{**r, "status": "completed"} if i == 6 else r for i, r in enumerate(rows)], [rows[1], rows[0], *rows[2:]]):
        try:
            prefix_length(bad, expected, "failed")
        except ValueError:
            pass
        else:
            raise AssertionError("malformed prefix was accepted")
    print("5 tiny prefix checks passed; no native/cloud/proof access")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    need(not REPORT.exists(), "new fixed report path required")
    started = dt.datetime.now(dt.timezone.utc).isoformat()
    try:
        report = audit()
        code = 0
    except Exception as error:
        report = {"schema": "r1.fused-update-partial-integrity/v1", "status": "not_verified", "error": str(error),
                  "payload_integrity": "not_verified", "performance_claims": False, "performance_screen": "not_evaluable",
                  "production_adoption": False, "quality_certification": False, "groups": []}
        code = 2
    report.update(started_at=started, ended_at=dt.datetime.now(dt.timezone.utc).isoformat())
    with REPORT.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2, sort_keys=True, allow_nan=False)
        stream.write("\n")
    print(json.dumps({"status": report["status"], "performance_screen": "not_evaluable", "report": str(REPORT)}))
    return code


if __name__ == "__main__":
    raise SystemExit(main())
