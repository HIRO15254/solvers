"""Portable strict reader; completed diagnostics only, never an adoption guard.

Stream verification is for the authorized cloud/recovery host. Imports are
from this trusted checkout, never from retained proof or recovered source.
"""
from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import importlib.util
import json
import math
from pathlib import Path, PurePosixPath
import statistics
import struct
import sys
import tarfile

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("occupancy_trusted_runner", HERE / "run.py")
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)
need, read, pin, pair, relative = run.need, run.read, run.pin, run.pair, run.relative


def utc(text):
    value = dt.datetime.fromisoformat(text.replace("Z", "+00:00"))
    need(value.utcoffset() == dt.timedelta(0), "non-UTC timestamp")
    return value


def digest(stream, limit, header=None):
    sha, size = hashlib.sha256(), 0
    if header is not None:
        actual = stream.read(len(header))
        need(actual == header, "state header differs")
        sha.update(actual)
        size += len(actual)
    while block := stream.read(1024**2):
        size += len(block)
        need(size <= limit, "decompressed size bound exceeded")
        sha.update(block)
    return {"bytes": size, "sha256": sha.hexdigest()}


def recovery_members(manifest):
    need(manifest["schema"] == "r1.cpu-occupancy-vm16-recovery/v1"
         and manifest["unit_quiescence_checked"] is True and manifest["proof_present"] is True, "recovery provenance incomplete")
    expected = {}
    for row in manifest["files"]:
        name = relative(row["member"]).as_posix()
        need(name not in expected and name != "recovery-manifest.json", "duplicate/reserved recovered member")
        expected[name] = pair(row)
    return expected


class Evidence:
    def __init__(self, root):
        self.root = Path(root).resolve(strict=True)
        manifest = read(self.root / "retained.json")
        need(manifest["schema"] == "r1.cpu-occupancy-retained/v1", "retained schema differs")
        self.files = manifest["files"]
        recovered = self.root / "recovery-manifest.json"
        recovery = recovery_members(read(recovered)) if recovered.exists() else None
        actual, all_files = {}, {}
        total = 0
        for p in sorted(self.root.rglob("*")):
            need(not p.is_symlink(), "evidence symlink")
            if p.is_file():
                name = p.relative_to(self.root).as_posix()
                if name == "recovery-manifest.json":
                    continue
                relative(name)
                total += p.stat().st_size
                need(total <= run.MAX_RETAINED + (32 * 1024**2 if recovery is not None else 0), "retained evidence size exceeded")
                all_files[name] = pin(p)
                if name == "retained.json" or (recovery is not None and name.startswith("recovery/")):
                    continue
                actual[name] = all_files[name]
            else:
                need(p.is_dir(), "nonregular evidence member")
        if recovery is not None:
            need(all_files == recovery, "recovery exact membership or bytes differ")
        need(actual == self.files, "retained membership or exact bytes differ")
        self.plan = read(self.root / "plan.json")
        self.origin = PurePosixPath(self.plan["output"])
        need(self.origin.is_absolute(), "original Linux output root required")
        self.states = set()

    def name(self, original):
        p = PurePosixPath(original)
        need(p.is_relative_to(self.origin), "reference outside proof")
        return relative(p.relative_to(self.origin).as_posix()).as_posix()

    def require(self, reference):
        name = self.name(reference["path"])
        need(self.files.get(name) == pair(reference), "reference pin/membership differs")
        return self.root / name

    def control(self, suffix):
        matches = [(name, value) for name, value in self.plan["controls"].items() if name.endswith("/" + suffix)]
        need(len(matches) == 1, "control not uniquely identified")
        name, value = matches[0]
        retained = self.root / relative(value["retained"])
        need(self.files.get(value["retained"]) == pair(value), "retained control differs")
        return name, value, retained

    def state(self, canonical, case):
        key = canonical["state_gzip"]["path"]
        if key in self.states:
            return
        path = self.require(canonical["state_gzip"])
        header_values = (16, 16, *run.common.HEADERS[case])
        expected_length = 72 + 2 * sum(header_values[4:6]) + 4 * sum(header_values[6:8])
        with gzip.open(path, "rb") as stream:
            value = digest(stream, expected_length, b"R1F32S01" + struct.pack("<8Q", *header_values))
        need(value == pair(canonical["state"]) and value["bytes"] == expected_length, "canonical full stream differs")
        self.states.add(key)


def verify_plan(e):
    p = e.plan
    need(p["schema"] == "r1.cpu-occupancy/v1" and p["schedule"] == run.schedule()
         and p["limits"] == run.LIMITS and p["environment"] == run.environment(p["tools"]), "prepared protocol differs")
    need(0 < (utc(p["deadline_utc"]) - utc(p["created_at"])).total_seconds() <= 960, "global time window differs")
    h = p["host"]
    need(h["logical_cpus"] == 32 and h["physical_cores"] == 16 and h["machine"] == "x86_64"
         and h["one_per_core"] == run.one_per_core(h["topology"], h["affinity"]) and h["boot_id"], "host topology differs")
    cg = h["cgroup"]
    need(cg["memory_max"] == str(12 * 1024**3) and cg["swap_max"] == "0" and cg["cpu_weight"] == "100" and cg["cpu_limits"], "outer service limits differ")
    for value in cg["cpu_limits"].values():
        maximum, period = value.split()
        need(maximum == "max" or int(maximum) / int(period) >= 32, "CPU quota below 32")
    required = [HERE / "run.py", Path(__file__), HERE / "protocol.jp.md", run.COMMON, run.common.SHARED,
                run.DURABLE, run.SUPERVISOR, run.ORIGINAL]
    required += [path for path in sorted((HERE / "adapter").rglob("*")) if path.is_file()]
    manifest_name, _, manifest_path = e.control("manifest.json")
    _, _, installation_path = e.control("installation.json")
    manifest, installation = read(manifest_path), read(installation_path)
    need(manifest["schema"] == "r1-cpu-occupancy-package/v1" and pin(manifest_path) == p["package"]["manifest"]
         and installation["manifest"] == p["package"]["manifest"] and installation["archive_sha256"] == p["package"]["archive_sha256"], "package binding differs")
    package_root = str(PurePosixPath(manifest_name).parent)
    need(installation["destination"] == package_root and installation["source_revision"] == manifest["source_revision"]
         and installation["source_files"] == len(manifest["source_pins"]) and installation["builds_or_solves_started"] == 0, "installation receipt differs")
    for path in required:
        suffix = path.relative_to(run.ROOT).as_posix()
        original, value, retained = e.control(suffix)
        need(original == package_root + "/" + suffix and pair(value) == pin(path) == pin(retained)
             and manifest["files"].get(suffix) == pair(value), "trusted control pin differs")
    need({n.removeprefix("source/"): v for n, v in manifest["files"].items() if n.startswith("source/")}
         == manifest["source_pins"], "original source package pins differ")
    source = p["source"]
    run.source_bindings(manifest["source_pins"], source["files"], pin(run.ORIGINAL), pin(run.CPU_ADAPTER))
    observed, total = {}, 0
    with tarfile.open(e.require(source["archive"]), "r|gz") as archive:
        for member in archive:
            name = relative(member.name).as_posix()
            need(member.isfile() and name not in observed and member.size <= 2 * 1024**2, "source archive member differs")
            total += member.size
            need(total <= 16 * 1024**2 and len(observed) < 1024, "source archive bound exceeded")
            with archive.extractfile(member) as stream:
                observed[name] = digest(stream, member.size)
    need(observed == source["files"], "retained source archive differs")


def verify_stage(e, row, binaries):
    need(row["status"] == "completed" and row["supervisor_exit"] == 0
         and row["host_before"] == row["host_after"] == e.plan["host"], "stage not completed on original host/boot")
    need(utc(e.plan["created_at"]) <= utc(row["started_at"]) <= utc(row["ended_at"]) <= utc(row["verified_at"])
         <= utc(e.plan["deadline_utc"]), "stage time outside deadline")
    completion = read(e.require(row["completion"]))
    need(completion == {k: v for k, v in row.items() if k != "completion"}, "immutable stage receipt differs")
    record = read(e.require(row["record"]))
    run.terminal(record)
    need(e.name(row["record"]["path"]) == row["name"] + "/supervisor.json"
         and e.name(row["completion"]["path"]) == row["name"] + "/completed.json", "stage receipt path differs")
    need(record["argv"] == record["resolved_argv"] == row["command"] and record["shell"] is False, "workload command differs")
    need(record["cwd"] == (e.plan["source"]["path"] if row["kind"] == "build" else str(e.origin)), "stage source/cwd differs")
    seconds = 240 if row["kind"] == "build" else 10 if row["kind"] == "toolchain" else 90
    need(record["limits"] == {**run.LIMITS, "timeout_seconds": seconds}, "stage resource limits differ")
    need(record["runtime"]["logical_cpus"] == 32 and record["runtime"]["machine"] == "x86_64", "runtime CPU differs")
    identities = {x["path"]: pair(x) for x in record["identity_before"]}
    need(identities.get(str(e.origin / "plan.json")) == e.files["plan.json"], "supervisor plan identity differs")
    for suffix in ("experiments/hu-postflop-r1/flop-scaling/cpu-occupancy/run.py", "tools/run_supervised.py"):
        name, value, _ = e.control(suffix)
        need(identities.get(name) == pair(value), "supervisor control identity differs")
    tools = e.plan["tools"]
    if row["kind"] in {"build", "toolchain"}:
        tool = tools["cargo" if row["kind"] == "build" else "rustc"]
        need(identities.get(tool["path"]) == pair(tool), "build tool identity differs")
    else:
        binary = binaries[row["adapter"]]
        need(identities.get(binary["path"]) == pair(binary), "solver binary identity differs")
        if row["configuration"] == "16-onecore":
            need(identities.get(tools["taskset"]["path"]) == pair(tools["taskset"]), "taskset identity differs")
    for value in run.raw_outputs(record, e.origin / row["name"]):
        e.require(value)
    need(row["process_seconds"] == record["elapsed_seconds"] and 0 < row["process_seconds"] <= seconds + 1, "whole-process timer differs")
    need(row["root_os_peak_resident_bytes"] == record["last_sample"]["root_os_peak_resident_bytes"] > 0
         and row["root_os_peak_source"] == record["last_sample"]["root_os_peak_source"] == "wait4.ru_maxrss_linux_kib", "root RSS counter differs")
    return record


def verify_build(e, execution):
    built = read(e.root / "build.json")
    need(built["status"] == "completed" and built["plan"] == e.files["plan.json"]
         and built["stages"] == execution["stages"][:2] and built["binaries"] == execution["binaries"], "build receipt differs")
    first, build = built["stages"]
    need(first["name"] == first["kind"] == "toolchain" and build["name"] == build["kind"] == "build", "build schedule differs")
    tools = e.plan["tools"]
    need(first["command"] == [tools["rustc"]["path"], "-Vv"], "toolchain command differs")
    record = verify_stage(e, first, {})
    version = e.require(record["outputs"]["stdout"]).read_text()
    need(version == execution["rustc_version"] and "release: 1.97.0" in version and "host: x86_64-unknown-linux-gnu" in version, "compiler version differs")
    target = str(PurePosixPath(e.plan["workspace"]) / "target")
    expected = [tools["cargo"]["path"], "build", "--locked", "--offline", "--release", "-j2", "--target-dir", target,
                "-p", "holdem", "--example", run.EXAMPLES["original"], "--example", run.EXAMPLES["cpu"], "--message-format=json"]
    need(build["command"] == expected, "native build command differs")
    record = verify_stage(e, build, {})
    artifacts = {}
    for line in e.require(record["outputs"]["stdout"]).read_text().splitlines():
        message = run.loads(line)
        if message.get("reason") == "compiler-artifact" and message["target"]["name"] in run.EXAMPLES.values():
            need(message["target"]["name"] not in artifacts, "duplicate example build artifact")
            need(message["target"]["kind"] == ["example"] and message["profile"]["opt_level"] == "3"
                 and not message["profile"]["test"] and not message["fresh"], "example optimized fresh profile differs")
            artifacts[message["target"]["name"]] = message["executable"]
    need(set(artifacts) == set(run.EXAMPLES.values()), "both example build artifacts missing")
    need(set(built["binaries"]) == set(build["retained_binaries"]) == set(run.EXAMPLES), "binary inventory differs")
    for kind, name in run.EXAMPLES.items():
        binary = built["binaries"][kind]
        need(binary["path"] == artifacts[name] == target + "/release/examples/" + name, "native binary path differs")
        with gzip.open(e.require(build["retained_binaries"][kind]), "rb") as stream:
            need(digest(stream, 128 * 1024**2) == pair(binary), "retained binary bytes differ")
    return built["binaries"]


def verify_solve(e, row, binaries, expected_canonical):
    verify_stage(e, row, binaries)
    affinity = e.plan["host"]["one_per_core"] if row["configuration"] == "16-onecore" else e.plan["host"]["affinity"]
    output = str(e.origin / row["name"] / "artifacts")
    command = [binaries[row["adapter"]]["path"], row["case"], str(row["workers"]), "16", output]
    if row["configuration"] == "16-onecore":
        command = [e.plan["tools"]["taskset"]["path"], "--cpu-list", ",".join(map(str, affinity)), *command]
    need(command == row["command"] and row["expected_child_affinity"] == affinity, "child command/affinity differs")
    outputs = row["outputs"]
    names = {"invocation.json", "result.json", "quality.json", "state.bin"} | ({"cpu.json"} if row["adapter"] == "cpu" else set())
    need(set(outputs) == names, "solver artifact set differs")
    prefix = row["name"] + "/artifacts/"
    need({n.removeprefix(prefix) for n in e.files if n.startswith(prefix)} == names - {"state.bin"}, "retained artifact membership differs")
    for name, value in outputs.items():
        need(value["path"] == output + "/" + name, "artifact path differs")
        if name != "state.bin":
            e.require(value)
    result, invocation, quality = (read(e.require(outputs[n])) for n in ("result.json", "invocation.json", "quality.json"))
    run.validate_values(row, result, invocation, quality)
    need(result == row["result"], "result receipt differs")
    if row["kind"] == "canonical":
        need(result["cfr_seconds"] >= 4 and expected_canonical is None, "baseline timing threshold differs")
    if row["adapter"] == "cpu":
        cpu = read(e.require(outputs["cpu.json"]))
        run.validate_cpu(row, cpu, result, affinity)
        need(cpu == row["cpu"], "CPU receipt differs")
    retention = read(e.require(row["retention_receipt"]))
    need(retention == row["state_retention"] and retention["fullbyte_verified"] is True and retention["raw_removed"] is True
         and retention["original"] == outputs["state.bin"], "durable raw removal receipt differs")
    canonical = retention["canonical"]
    need(pair(canonical["state"]) == pair(outputs["state.bin"]) and result["state_bytes"] == outputs["state.bin"]["bytes"], "full state identity differs")
    need(canonical["state"]["path"] == str(PurePosixPath(e.plan["workspace"]) / "canonical" / (row["case"] + ".bin"))
         and e.name(canonical["state_gzip"]["path"]) == "canonical/" + row["case"] + ".bin.gz", "canonical path differs")
    if expected_canonical is None:
        need(retention["kind"] == "new_canonical" and canonical["quality"] == outputs["quality.json"], "canonical ownership differs")
    else:
        need(retention["kind"] == "alias" and canonical == expected_canonical, "canonical alias differs")
        need(e.require(outputs["quality.json"]).read_bytes() == e.require(canonical["quality"]).read_bytes(), "quality full bytes differ")
    e.state(canonical, row["case"])
    return canonical


def stats(values):
    need(len(values) == 3 and all(math.isfinite(v) and v >= 0 for v in values), "three finite nonnegative samples required")
    low, high = min(values), max(values)
    return {"samples": values, "median": statistics.median(values), "minimum": low, "maximum": high,
            "max_over_min": high / low if low else None}


def observations(row):
    cpu, result = row["cpu"], row["result"]
    intervals = {"cfr": (cpu["cfr_cpu_seconds"], cpu["cfr_wall_seconds"]),
                 "quality_7_walks": (cpu["quality_cpu_seconds"], cpu["quality_wall_seconds"]),
                 "exploitability_3_walks": (cpu["exploitability_cpu_seconds"], cpu["exploitability_wall_seconds"])}
    for kind in ("ev", "br"):
        for seat in range(2):
            intervals[f"{kind}_p{seat}"] = (cpu[kind + "_cpu_seconds"][seat], cpu[kind + "_wall_seconds"][seat])
    values = {"construction_wall_seconds": result["build_seconds"], "state_write_wall_seconds": result["state_write_seconds"],
              "whole_process_seconds": row["process_seconds"], "root_os_peak_resident_bytes": row["root_os_peak_resident_bytes"]}
    for name, (processor, wall) in intervals.items():
        values.update({name + "_cpu_seconds": processor, name + "_wall_seconds": wall,
                       name + "_cpu_over_wall": processor / wall,
                       name + "_cpu_over_wall_per_allowed_logical": processor / wall / len(row["expected_child_affinity"])})
    return values


def summarize(rows):
    expected = run.schedule()
    need(len(rows) == 28 and all(all(row[k] == v for k, v in wanted.items()) for row, wanted in zip(rows, expected))
         and all(row["status"] == "completed" for row in rows), "incomplete or reordered fixed schedule")
    groups = []
    for case in run.CASES:
        for configuration in run.CONFIGS:
            selected = [r for r in rows if r["case"] == case and r["configuration"] == configuration and not r["warmup"]]
            need([r["round"] for r in selected] == [1, 2, 3], "measured rounds differ")
            samples = [observations(r) for r in selected]
            groups.append({"case": case, "configuration": configuration, "iterations": 16,
                           "stages": [r["name"] for r in selected], "metrics": {k: stats([s[k] for s in samples]) for k in samples[0]}})
    calibration = []
    for case in run.CASES:
        original = next(r for r in rows if r["case"] == case and r["kind"] == "canonical")
        cpu = next(r for r in rows if r["case"] == case and r["kind"] == "calibration")
        calibration.append({"case": case, "original_stage": original["name"], "cpu_stage": cpu["name"],
                            "original_cfr_wall_seconds": original["result"]["cfr_seconds"],
                            "cpu_cfr_wall_seconds": cpu["result"]["cfr_seconds"],
                            "cpu_over_original_cfr_wall": cpu["result"]["cfr_seconds"] / original["result"]["cfr_seconds"],
                            "cpu_observations": observations(cpu), "statistical_perturbation_calibration": False})
    return {"groups": groups, "calibration": calibration}


def check(root):
    e = Evidence(root)
    verify_plan(e)
    execution = read(e.root / "execution.json")
    need(execution["schema"] == "r1.cpu-occupancy-execution/v1" and execution["plan"] == e.files["plan.json"], "execution binding differs")
    need(execution["status"] == "completed", "incomplete proof: diagnostics are not evaluable")
    need(len(execution["stages"]) == 30 and execution["counts"] == {"completed": 30, "failed": 0, "skipped": 0}, "fixed stage counts differ")
    need(utc(e.plan["created_at"]) <= utc(execution["started_at"]) <= utc(execution["ended_at"]) <= utc(e.plan["deadline_utc"]), "execution deadline differs")
    binaries = verify_build(e, execution)
    canonical = {}
    previous = utc(execution["stages"][1]["verified_at"])
    for row, expected in zip(execution["stages"][2:], run.schedule()):
        need(all(row[k] == value for k, value in expected.items()), "fixed solve schedule differs")
        need(previous <= utc(row["started_at"]), "solve stage ordering overlaps")
        canonical[row["case"]] = verify_solve(e, row, binaries, canonical.get(row["case"]))
        previous = utc(row["verified_at"])
    need(canonical == execution["canonical"] and len(e.states) == 2, "canonical final inventory differs")
    return {"schema": "r1.cpu-occupancy-report/v1", "status": "completed_descriptive_diagnostic", "payload_integrity": "verified",
            "plan": e.files["plan.json"], "execution": e.files["execution.json"], "host": e.plan["host"],
            "counts": {"build_commands": 1, "canonical": 2, "calibration": 2, "warmup": 6, "measured": 18},
            **summarize(execution["stages"][2:]),
            "limits": ["Baseline-only process CPU/wall observations; no optimization adoption or quality certification",
                       "One calibration pair per input checks state/quality and clocks, not statistical timing perturbation",
                       "Process CPU includes spinning, allocator and scheduler work; CPU/wall is average logical CPU consumption",
                       "Guest 16 core/32 logical topology does not prove dedicated host physical cores",
                       "Affinity differences do not isolate SMT causally from placement, scheduling or bandwidth",
                       "Root wait4 ru_maxrss is a whole-process Linux high-water counter, not phase memory or aggregate child memory"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    need(not args.report.exists(), "new report path required")
    try:
        report = check(args.out)
        code = 0
    except Exception as error:
        report = {"schema": "r1.cpu-occupancy-report/v1", "status": "not_evaluable", "error": str(error),
                  "payload_integrity": "not_verified", "groups": [], "performance_claims": False}
        code = 2
    with args.report.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(report, stream, indent=2, sort_keys=True, allow_nan=False)
        stream.write("\n")
    print(json.dumps({"status": report["status"], "payload_integrity": report["payload_integrity"]}))
    return code


if __name__ == "__main__":
    raise SystemExit(main())
