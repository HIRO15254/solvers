"""Trusted portable exact-proof reader; CPU samples are diagnostic, never a speed screen."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import sys
import tarfile

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("cpu_profile_trusted_run", HERE / "run.py")
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)
HELPER = HERE.parent / "chance-grain/analyze.py"
need, read, pin, pair, relative, utc = run.need, run.read, run.pin, run.pair, run.relative, run.utc
need(pin(HELPER)["sha256"] == "866dfdc35ff9e2e5f6f0ed2b681e657dd7aa80b29e12da43731801c8bc0b1fcf", "trusted reader changed")
old = run.base.load("cpu_profile_common_reader", HELPER)
old.run = run
digest, verify_host = old.digest, old.verify_host


class Evidence(old.Evidence):
    def __init__(self, root):
        self.root = Path(root).resolve(strict=True)
        manifest = read(self.root / "retained.json")
        need(manifest["schema"] == "r1.cpu-profile-retained/v1", "retained schema differs")
        self.files = manifest["files"]
        recovered = self.root / "recovery-manifest.json"
        recovery = None
        if recovered.exists():
            m = read(recovered)
            need(m["schema"] == "r1.cpu-profile-vm19-recovery/v1" and m["unit_quiescence_checked"] is True
                 and m["proof_present"] is True, "recovery provenance incomplete")
            recovery = {}
            for r in m["files"]:
                name = relative(r["member"]).as_posix()
                need(name not in recovery and name != "recovery-manifest.json", "duplicate recovery member")
                recovery[name] = pair(r)
        actual, all_files, size = {}, {}, 0
        for p in sorted(self.root.rglob("*"), key=lambda p: p.relative_to(self.root).parts):
            need(not p.is_symlink(), "evidence symlink")
            if not p.is_file():
                need(p.is_dir(), "nonregular evidence member")
                continue
            name = p.relative_to(self.root).as_posix()
            if name == "recovery-manifest.json":
                continue
            relative(name)
            size += p.stat().st_size
            need(size <= run.MAX_RETAINED + (32 * 1024**2 if recovery else 0), "retention size exceeded")
            all_files[name] = pin(p)
            if name != "retained.json" and not (recovery and name.startswith("recovery/")):
                actual[name] = all_files[name]
        need(actual == self.files, "retained exact membership/bytes differ")
        if recovery is not None:
            need(all_files == recovery, "recovery exact membership/bytes differ")
        self.plan = read(self.root / "plan.json")
        self.origin = PurePosixPath(self.plan["output"])
        need(self.origin.is_absolute(), "original Linux output path required")
        self.states = set()


def verify_plan(e):
    p = e.plan
    need(p["schema"] == "r1.cpu-profile/v1" and p["schedule"] == run.schedule()
         and p["phase"] == "build" and p["plan_file"] == "plan.json" and p["limits"] == run.phase_limits("build")
         and p["environment"] == run.environment(p["tools"]), "prepared protocol differs")
    run.bounds(p["launch_attempted_at"], p["stop_deadline_utc"], p["deadline_utc"], "build", p["created_at"])
    verify_host(p["host"], "build")
    need(set(p["tools"]) == {"cargo", "rustc", "perf", "python"}, "tool set differs")
    manifest_name, _, manifest_path = e.control("manifest.json")
    _, _, installation_path = e.control("installation.json")
    manifest, installation = read(manifest_path), read(installation_path)
    need(manifest["schema"] == "r1-cpu-profile-package/v1" and pin(manifest_path) == p["package"]["manifest"]
         and installation["manifest"] == p["package"]["manifest"] and installation["archive_sha256"] == p["package"]["archive_sha256"], "package binding differs")
    package_root = str(PurePosixPath(manifest_name).parent)
    need(installation["destination"] == package_root and installation["source_revision"] == manifest["source_revision"]
         and installation["source_files"] == len(manifest["source_pins"]) and installation["builds_or_solves_started"] == 0, "installation differs")
    for path in run.controls():
        suffix = path.relative_to(run.ROOT).as_posix()
        original, value, retained = e.control(suffix)
        need(original == package_root + "/" + suffix and pair(value) == pin(path) == pin(retained)
             and manifest["files"].get(suffix) == pair(value), "trusted control pin differs")
    need({n.removeprefix("source/"): v for n, v in manifest["files"].items() if n.startswith("source/")}
         == manifest["source_pins"], "original source package differs")
    sources = p["sources"]
    need(set(sources) == {"baseline"}, "source arm differs")
    run.source_bindings(manifest["source_pins"], {a:s["files"] for a,s in sources.items()}, pin(run.CPU_ADAPTER))
    source = sources["baseline"]
    observed, total = {}, 0
    with tarfile.open(e.require(source["archive"]), "r|gz") as archive:
        for member in archive:
            name = relative(member.name).as_posix()
            need(member.isfile() and name not in observed and member.size <= 2 * 1024**2, "source archive member differs")
            total += member.size
            need(total <= 16 * 1024**2 and len(observed) < 1024, "source archive limit exceeded")
            with archive.extractfile(member) as stream:
                observed[name] = digest(stream, member.size)
    need(observed == source["files"] and list(observed) == sorted(source["files"], key=lambda n: PurePosixPath(n).parts), "source archive bytes/order differ")


def verify_measurement(e, execution):
    original, p = e.plan, read(e.root / "measurement.json")
    changing = {"phase", "plan_file", "host", "limits", "created_at", "deadline_utc", "deadline_monotonic"}
    extra = {"build_plan", "build_receipt", "build_execution"}
    need(set(p) == set(original) | extra and {k:v for k,v in p.items() if k not in changing | extra}
         == {k:v for k,v in original.items() if k not in changing}, "measurement changed fixed inputs")
    need(p["phase"] == "measure" and p["plan_file"] == "measurement.json" and p["limits"] == run.phase_limits("measure")
         and p["build_plan"] == e.files["plan.json"] and p["build_receipt"] == e.files["build.json"]
         and p["build_execution"] == e.files["build-execution.json"], "measurement binding differs")
    run.bounds(p["launch_attempted_at"], p["stop_deadline_utc"], p["deadline_utc"], "measure", p["created_at"])
    need(utc(execution["ended_at"]) <= utc(p["created_at"]), "measurement precedes build")
    verify_host(p["host"], "measure")
    need(p["host"]["boot_id"] != original["host"]["boot_id"] and p["host"]["instance_id"] == original["host"]["instance_id"], "resize identity differs")
    e.plan = p


def verify_stage(e, row, binaries):
    need(row["status"] == "completed" and row["supervisor_exit"] == 0
         and row["host_before"] == row["host_after"] == e.plan["host"], "stage host/terminal differs")
    need(row["environment"] == e.plan["environment"] and row["forbidden_environment"] == [], "stage environment differs")
    need(utc(e.plan["created_at"]) <= utc(row["started_at"]) <= utc(row["ended_at"]) <= utc(row["verified_at"])
         <= utc(e.plan["deadline_utc"]), "stage outside deadline")
    need(read(e.require(row["completion"])) == {k:v for k,v in row.items() if k != "completion"}, "completion receipt differs")
    record = read(e.require(row["record"]))
    run.terminal(record)
    need(e.name(row["record"]["path"]) == row["name"] + "/supervisor.json"
         and e.name(row["completion"]["path"]) == row["name"] + "/completed.json", "stage path differs")
    need(record["argv"] == record["resolved_argv"] == row["command"] and record["shell"] is False, "stage argv differs")
    need(record["cwd"] == (e.plan["sources"]["baseline"]["path"] if row["kind"] in {"build", "tests"} else str(e.origin)), "stage cwd differs")
    seconds = {"build": 480, "tests": 480, "toolchain": 10, "perf-preflight": 60, "canonical": 120, "profile": 90}[row["kind"]]
    need(record["limits"] == {**e.plan["limits"], "timeout_seconds": seconds}, "stage limits differ")
    need(record["runtime"]["logical_cpus"] == e.plan["host"]["logical_cpus"] and record["runtime"]["machine"] == "x86_64", "runtime CPU differs")
    identities = {x["path"]: pair(x) for x in record["identity_before"]}
    need(identities.get(str(e.origin / e.plan["plan_file"])) == e.files[e.plan["plan_file"]], "plan identity differs")
    for suffix in ("experiments/hu-postflop-r1/flop-scaling/cpu-profile/run.py", "tools/run_supervised.py"):
        name, value, _ = e.control(suffix)
        need(identities.get(name) == pair(value), "control identity differs")
    tools = e.plan["tools"]
    keys = {"build": ("cargo",), "tests": ("cargo",), "toolchain": ("rustc",), "perf-preflight": ("python", "perf"),
            "canonical": ("perf",), "profile": ("perf",)}[row["kind"]]
    for key in keys:
        need(identities.get(tools[key]["path"]) == pair(tools[key]), "tool identity differs")
    if row["kind"] in {"canonical", "profile"}:
        need(identities.get(binaries["baseline"]["path"]) == pair(binaries["baseline"]), "binary identity differs")
    for value in run.raw_outputs(record, e.origin / row["name"]):
        e.require(value)
    need(row["process_seconds"] == record["elapsed_seconds"] and 0 < row["process_seconds"] <= seconds + 1, "process timer differs")
    need(row["root_os_peak_resident_bytes"] == record["last_sample"]["root_os_peak_resident_bytes"] > 0
         and row["root_os_peak_source"] == record["last_sample"]["root_os_peak_source"] == "wait4.ru_maxrss_linux_kib", "RSS counter differs")
    return record


def capture_result(e, value, command, *, allowed=(0,), gz_stdout=None):
    need(value["argv"] == command and value["returncode"] in allowed, "perf command/result differs")
    stdout_name = e.name(value["stdout"]["path"])
    need(stdout_name.endswith(".stdout.log"), "capture stdout suffix differs")
    receipt_name = stdout_name.removesuffix(".stdout.log") + ".json"
    need(receipt_name in e.files and read(e.root / receipt_name) == value, "capture command receipt differs")
    if gz_stdout is None:
        stdout = e.require(value["stdout"]).read_bytes()
    else:
        with gzip.open(e.require(gz_stdout), "rb") as stream:
            stdout = stream.read(64 * 1024**2 + 1)
        need(len(stdout) <= 64 * 1024**2 and {"bytes": len(stdout), "sha256": hashlib.sha256(stdout).hexdigest()} == pair(value["stdout"]), "perf text gzip bytes differ")
    stderr = e.require(value["stderr"]).read_bytes()
    return stdout.decode("utf-8"), stderr.decode("utf-8")


def verify_perf(e, value, phases=None):
    data = e.require(value["data"])
    need(value["schema"] == "r1.cpu-profile-perf/v1" and 0 < data.stat().st_size <= run.PERF_MAX, "perf size/schema differs")
    perf, original = e.plan["tools"]["perf"]["path"], value["data"]["path"]
    text, stderr = capture_result(e, value["script"], run.script_command(perf, original), gz_stdout=value["script_stdout_gzip"])
    need(not stderr.strip(), "perf script diagnostic present")
    summary = run.parse_samples(text, phases)
    dump, _ = capture_result(e, value["dump"], [perf, "script", "-D", "-i", original], gz_stdout=value["dump_stdout_gzip"])
    census = run.record_census(dump)
    attrs, attrerr = capture_result(e, value["attributes"], [perf, "evlist", "-v", "-i", original])
    run.validate_attributes(attrs)
    need(not attrerr.strip() and census["record_counts"]["PERF_RECORD_SAMPLE"] == summary["all_samples"], "perf attribute/sample census differs")
    need(not census["loss_or_throttle_records_present"] and value["summary"] == summary and value["census"] == census, "sampling summary/census differs")
    return summary


def verify_preflight(e, row, binaries):
    verify_stage(e, row, binaries)
    python, perf = (e.plan["tools"][n]["path"] for n in ("python", "perf"))
    original_runner, _, _ = e.control("experiments/hu-postflop-r1/flop-scaling/cpu-profile/run.py")
    directory = e.origin / row["name"]
    need(row["command"] == [python, original_runner, "perf-check", "--perf", perf, "--out", str(directory)], "preflight command differs")
    p = read(e.require(row["preflight"]))
    need(p["schema"] == "r1.cpu-profile-preflight/v1" and p["status"] == "passed"
         and p["perf_identity"] == e.plan["tools"]["perf"], "preflight identity differs")
    capture_result(e, p["version"], [perf, "--version"])
    text = ""
    for key, verb in (("record_help", "record"), ("script_help", "script")):
        so, se = capture_result(e, p[key], [perf, verb, "-h"], allowed=(0, 129))
        text += so + se
    for name in ("--strict-freq", "--clockid", "--call-graph", "--no-buildid-cache", "--max-size", "--ns", "--show-lost-events"):
        need(name in text, "required actual help option missing")
    capture_result(e, p["record"], run.perf_command(perf, directory / "preflight.data", [python, original_runner, "perf-child", "--out", str(directory)]))
    need(verify_perf(e, p["perf"])["all_samples"] >= 10, "insufficient preflight samples")
    clock = read(e.require(p["clock"]))
    need(clock["clock"] == "CLOCK_MONOTONIC" and type(clock["pid"]) is int and clock["pid"] > 0
         and 0 <= clock["start_ns"] < clock["end_ns"], "preflight clock invalid")
    count = 0
    with gzip.open(e.require(p["perf"]["script_stdout_gzip"]), "rt") as stream:
        for line in stream:
            m = run.HEADER.fullmatch(line.rstrip("\n"))
            if m and int(m[1]) == clock["pid"] and clock["start_ns"] <= int(m[3]) * 10**9 + int(m[4]) < clock["end_ns"]:
                count += 1
    need(count == p["clock_samples"] and count >= 10, "preflight monotonic clock/sample mismatch")


def verify_build(e):
    built, execution = read(e.root / "build.json"), read(e.root / "build-execution.json")
    need(built["schema"] == "r1.cpu-profile-build/v1" and built["status"] == execution["status"] == "completed"
         and built["plan"] == e.files["plan.json"] and built["execution"] == e.files["build-execution.json"]
         and built["stages"] == execution["stages"] and built["binaries"] == execution["binaries"], "build receipt differs")
    need(execution["schema"] == "r1.cpu-profile-execution/v1" and execution["phase"] == "build"
         and execution["receipt_file"] == "build-execution.json" and execution["plan"] == built["plan"]
         and execution["counts"] == {"completed": 4, "failed": 0, "skipped": 0}, "build execution differs")
    need(utc(e.plan["created_at"]) <= utc(execution["started_at"]) <= utc(execution["ended_at"]) <= utc(e.plan["deadline_utc"]), "build deadline differs")
    expected_files = {n:v for n,v in e.files.items() if n in {"plan.json", "build-execution.json", "source-baseline.tar.gz", "binary-baseline.gz"}
                      or n.startswith(("inputs/", "perf-preflight-build/", "toolchain/", "build-baseline/", "tests-baseline/"))}
    need(built["files"] == expected_files, "immutable build membership differs")
    rows = built["stages"]
    need(len(rows) == 4 and all(all(r[k] == v for k,v in w.items()) for r,w in zip(rows, run.build_rows())), "build order differs")
    preflight, first, build, tests = rows
    verify_preflight(e, preflight, {})
    tools = e.plan["tools"]
    need(first["command"] == [tools["rustc"]["path"], "-Vv"], "toolchain command differs")
    record = verify_stage(e, first, {})
    version = e.require(record["outputs"]["stdout"]).read_text()
    need(version == execution["rustc_version"] and "release: 1.97.0" in version and "host: x86_64-unknown-linux-gnu" in version, "compiler differs")
    target = str(PurePosixPath(e.plan["workspace"]) / "baseline-target")
    need(build["command"] == [tools["cargo"]["path"], "build", "--locked", "--offline", "--release", "-j2", "--target-dir", target,
                            "-p", "holdem", "--example", run.EXAMPLE, "--message-format=json"], "build command differs")
    record = verify_stage(e, build, {})
    binaries = built["binaries"]
    need(set(binaries) == {"baseline"}, "binary inventory differs")
    binary = binaries["baseline"]
    artifacts = []
    for line in e.require(record["outputs"]["stdout"]).read_text().splitlines():
        m = run.base.loads(line)
        if m.get("reason") == "compiler-artifact" and m["target"]["name"] == run.EXAMPLE:
            need(m["target"]["kind"] == ["example"] and m["profile"]["opt_level"] == "3"
                 and not m["profile"]["test"] and not m["fresh"], "fresh optimized artifact differs")
            artifacts.append(m["executable"])
    need(artifacts == [binary["path"]] and binary["path"] == target + "/release/examples/" + run.EXAMPLE, "binary artifact differs")
    with gzip.open(e.require(build["retained_binary"]), "rb") as stream:
        need(digest(stream, 128 * 1024**2) == pair(binary), "retained binary differs")
    need(tests["command"] == run.test_command(e.plan), "core test command differs")
    record = verify_stage(e, tests, {})
    run.validate_tests(e.require(record["outputs"]["stdout"]).read_text())
    previous = utc(execution["started_at"])
    for row in rows:
        need(previous <= utc(row["started_at"]), "build stages overlap")
        previous = utc(row["verified_at"])
    need(previous <= utc(execution["ended_at"]), "build terminal precedes last stage")
    return binaries, execution


def verify_solve(e, row, binaries, canonical):
    verify_stage(e, row, binaries)
    output = str(e.origin / row["name"] / "artifacts")
    child = [binaries["baseline"]["path"], row["case"], str(row["workers"]), "64", output]
    command = run.perf_command(e.plan["tools"]["perf"]["path"], e.origin / row["name"] / "perf.data", child) if row["kind"] == "profile" else child
    need(row["command"] == command and row["child_command"] == child and row["expected_child_affinity"] == e.plan["host"]["affinity"], "solve command differs")
    names = {"invocation.json", "result.json", "quality.json", "state.bin", "cpu.json", "phases.json"}
    outputs = row["outputs"]
    need(set(outputs) == names, "artifact set differs")
    prefix = row["name"] + "/artifacts/"
    need({n.removeprefix(prefix) for n in e.files if n.startswith(prefix)} == names - {"state.bin"}, "retained artifacts differ")
    for name, value in outputs.items():
        need(value["path"] == output + "/" + name, "artifact path differs")
        if name != "state.bin":
            e.require(value)
    result, invocation, quality = (read(e.require(outputs[n])) for n in ("result.json", "invocation.json", "quality.json"))
    run.base.validate_values(row, result, {**invocation, "quality_chance_depth": 2}, quality)
    cpu, phases = (read(e.require(outputs[n])) for n in ("cpu.json", "phases.json"))
    run.validate_cpu(row, cpu, result, row["expected_child_affinity"])
    run.validate_phases(row, phases, result)
    need(result == row["result"] and cpu == row["cpu"] and phases == row["phases"], "artifact/receipt differs")
    if row["kind"] == "profile":
        verify_perf(e, row["perf"], phases)
    retention = read(e.require(row["retention_receipt"]))
    need(retention == row["state_retention"] and retention["fullbyte_verified"] is True and retention["raw_removed"] is True
         and retention["original"] == outputs["state.bin"], "state retention differs")
    observed = retention["canonical"]
    need(pair(observed["state"]) == pair(outputs["state.bin"]) and result["state_bytes"] == outputs["state.bin"]["bytes"], "full state identity differs")
    need(observed["state"]["path"] == str(PurePosixPath(e.plan["workspace"]) / "canonical" / (row["case"] + ".bin"))
         and e.name(observed["state_gzip"]["path"]) == "canonical/" + row["case"] + ".bin.gz", "canonical paths differ")
    if canonical is None:
        need(row["kind"] == "canonical" and retention["kind"] == "new_canonical" and observed["quality"] == outputs["quality.json"], "canonical ownership differs")
    else:
        need(row["kind"] == "profile" and retention["kind"] == "alias" and observed == canonical, "canonical alias differs")
        need(e.require(outputs["quality.json"]).read_bytes() == e.require(canonical["quality"]).read_bytes(), "quality full bytes differ")
    e.state(observed, row["case"], 64)
    return observed


def check(root):
    e = Evidence(root)
    verify_plan(e)
    build_host = e.plan["host"]
    binaries, built = verify_build(e)
    verify_measurement(e, built)
    execution = read(e.root / "execution.json")
    need(execution["schema"] == "r1.cpu-profile-execution/v1" and execution["phase"] == "measure"
         and execution["receipt_file"] == "execution.json" and execution["plan"] == e.files["measurement.json"]
         and execution["binaries"] == binaries, "measurement binding differs")
    need(execution["status"] == "completed" and execution["counts"] == {"completed": 11, "failed": 0, "skipped": 0}, "incomplete diagnostic: not_evaluated")
    need(utc(e.plan["created_at"]) <= utc(execution["started_at"]) <= utc(execution["ended_at"]) <= utc(e.plan["deadline_utc"]), "measurement deadline differs")
    rows = execution["stages"]
    expected = [{"name": "perf-preflight-measure", "kind": "perf-preflight"}, *run.schedule()]
    need(len(rows) == 11 and all(all(r[k] == v for k,v in wanted.items()) for r,wanted in zip(rows,expected)), "measurement schedule differs")
    verify_preflight(e, rows[0], binaries)
    canonical, observations = {}, []
    previous = utc(rows[0]["verified_at"])
    for row in rows[1:]:
        need(previous <= utc(row["started_at"]), "solve stages overlap")
        canonical[row["case"]] = verify_solve(e, row, binaries, canonical.get(row["case"]))
        previous = utc(row["verified_at"])
        if row["kind"] == "profile":
            observations.append({"stage": row["name"], "case": row["case"], "workers": row["workers"], "round": row["round"],
                                 "sampling": row["perf"]["summary"], "census": row["perf"]["census"],
                                 "process_cpu": row["cpu"], "phases": row["phases"], "result": row["result"]})
    need(previous <= utc(execution["ended_at"]) and canonical == execution["canonical"] and set(canonical) == set(run.CASES)
         and len(e.states) == 2, "terminal/canonical membership differs")
    need(sum(v["bytes"] for n,v in e.files.items() if n.endswith("/perf.data")) <= 128 * 1024**2, "total perf cap exceeded")
    return {"schema": "r1.cpu-profile-report/v1", "status": "completed", "payload_integrity": "verified",
            "diagnostic": "samples_validated", "performance_screen": "not_applicable", "production_adoption": False,
            "build_host": build_host, "measurement_host": e.plan["host"], "binary": binaries["baseline"],
            "build_plan": e.files["plan.json"], "measurement_plan": e.files["measurement.json"], "execution": e.files["execution.json"],
            "counts": {"native_build": 1, "core_test_commands": 1, "perf_preflights": 2, "canonical": 2, "profiles": 8},
            "observations": observations,
            "limits": ["F32/DCFR64 fixed iterations; exact same-input state/quality, not convergence or external quality",
                       "All measurements use one portable frame-pointer binary and one 32-logical/16-core guest boot",
                       "Leaf sample counts include unknown symbols; not elapsed-time shares, off-CPU time or memory bandwidth",
                       "perf root wait4 RSS may include child high-water accounting; not solver-phase memory",
                       "At callchain limit suggests truncation; shorter fp stacks can also be incomplete",
                       "No PMU, cache/bandwidth attribution, SMT causal conclusion, speed guard or production change"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    need(not args.report.exists(), "fresh report required")
    result = check(args.out)
    args.report.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
