"""Portable strict full-proof reader and predeclared candidate performance screen.

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
    need(manifest["schema"] == "r1.chance-grain-vm18-recovery/v1"
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
        need(manifest["schema"] == "r1.chance-grain-retained/v1", "retained schema differs")
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

    def state(self, canonical, case, iterations):
        key = canonical["state_gzip"]["path"]
        if key in self.states:
            return
        path = self.require(canonical["state_gzip"])
        header_values = (iterations, iterations, *run.common.HEADERS[case])
        expected_length = 72 + 2 * sum(header_values[4:6]) + 4 * sum(header_values[6:8])
        with gzip.open(path, "rb") as stream:
            value = digest(stream, expected_length, b"R1F32S01" + struct.pack("<8Q", *header_values))
        need(value == pair(canonical["state"]) and value["bytes"] == expected_length, "canonical full stream differs")
        self.states.add(key)


def verify_plan(e):
    p = e.plan
    need(p["schema"] == "r1.chance-grain/v1" and p["schedule"] == run.schedule()
         and p["phase"] == "build" and p["plan_file"] == "plan.json" and p["limits"] == run.phase_limits("build") and p["environment"] == run.environment(p["tools"]), "prepared protocol differs")
    need(0 < (utc(p["deadline_utc"]) - utc(p["created_at"])).total_seconds() <= run.WINDOW_SECONDS, "global time window differs")
    verify_host(p["host"], "build")
    required = run.controls()
    manifest_name, _, manifest_path = e.control("manifest.json")
    _, _, installation_path = e.control("installation.json")
    manifest, installation = read(manifest_path), read(installation_path)
    need(manifest["schema"] == "r1-chance-grain-package/v1" and pin(manifest_path) == p["package"]["manifest"]
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
    sources = p["sources"]
    need(set(sources) == set(run.ARMS), "source arm inventory differs")
    run.source_bindings(manifest["source_pins"], {a: s["files"] for a, s in sources.items()}, pin(run.CPU_ADAPTER))
    for source in sources.values():
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


def verify_host(h, phase):
    count = 2 if phase == "build" else 32
    need(h["logical_cpus"] == count and h["machine"] == "x86_64" and h["boot_id"] and h["instance_id"].isdecimal(), "host identity differs")
    need(len(h["affinity"]) == len(set(h["affinity"])) == count and {t["cpu"] for t in h["topology"]} == set(h["affinity"]), "phase affinity differs")
    need(h["physical_cores"] == len({(t["socket"], t["core"]) for t in h["topology"]}), "physical topology differs")
    need(h["one_per_core"] == (run.one_per_core(h["topology"], h["affinity"]) if phase == "measure" else []), "measurement topology differs")
    run.validate_features(h["v3"], h["affinity"])
    cg = h["cgroup"]
    need(cg["memory_max"] == str((6 if phase == "build" else 12) * 1024**3) and cg["swap_max"] == "0" and cg["cpu_weight"] == "100" and cg["cpu_limits"], "outer service limits differ")
    for value in cg["cpu_limits"].values():
        maximum, period = value.split()
        need(maximum == "max" or int(maximum) / int(period) >= count, "CPU quota below phase count")


def verify_measurement(e, build_execution):
    original = e.plan
    p = read(e.root / "measurement.json")
    changing = {"phase", "plan_file", "host", "limits", "created_at", "deadline_utc", "deadline_monotonic"}
    extra = {"build_plan", "build_receipt", "build_execution"}
    need(set(p) == set(original) | extra and {k:v for k,v in p.items() if k not in changing | extra}
         == {k:v for k,v in original.items() if k not in changing}, "measurement preparation changed source/protocol")
    need(p["phase"] == "measure" and p["plan_file"] == "measurement.json" and p["limits"] == run.phase_limits("measure"), "measurement phase differs")
    need(p["build_plan"] == e.files["plan.json"] and p["build_receipt"] == e.files["build.json"] and p["build_execution"] == e.files["build-execution.json"], "build-to-measurement pins differ")
    need(600 < (utc(p["deadline_utc"]) - utc(p["created_at"])).total_seconds() <= run.WINDOW_SECONDS, "measurement window differs")
    need(utc(build_execution["ended_at"]) <= utc(p["created_at"]), "measurement precedes build completion")
    verify_host(p["host"], "measure")
    need(p["host"]["boot_id"] != original["host"]["boot_id"] and p["host"]["instance_id"] == original["host"]["instance_id"], "build/measurement reboot boundary differs")
    e.plan = p


def verify_stage(e, row, binaries):
    need(row["status"] == "completed" and row["supervisor_exit"] == 0
         and row["host_before"] == row["host_after"] == e.plan["host"], "stage not completed on original host/boot")
    need(row["environment"] == e.plan["environment"] and row["forbidden_environment"] == [], "actual stage environment differs")
    need(utc(e.plan["created_at"]) <= utc(row["started_at"]) <= utc(row["ended_at"]) <= utc(row["verified_at"])
         <= utc(e.plan["deadline_utc"]), "stage time outside deadline")
    completion = read(e.require(row["completion"]))
    need(completion == {k: v for k, v in row.items() if k != "completion"}, "immutable stage receipt differs")
    record = read(e.require(row["record"]))
    run.terminal(record)
    need(e.name(row["record"]["path"]) == row["name"] + "/supervisor.json"
         and e.name(row["completion"]["path"]) == row["name"] + "/completed.json", "stage receipt path differs")
    need(record["argv"] == record["resolved_argv"] == row["command"] and record["shell"] is False, "workload command differs")
    need(record["cwd"] == (e.plan["sources"][row["arm"]]["path"] if row["kind"] in {"build", "tests"} else str(e.origin)), "stage source/cwd differs")
    seconds = run.BUILD_SECONDS if row["kind"] in {"build", "tests"} else 10 if row["kind"] == "toolchain" else 90
    need(record["limits"] == {**e.plan["limits"], "timeout_seconds": seconds}, "stage resource limits differ")
    need(record["runtime"]["logical_cpus"] == e.plan["host"]["logical_cpus"] and record["runtime"]["machine"] == "x86_64", "runtime CPU differs")
    identities = {x["path"]: pair(x) for x in record["identity_before"]}
    need(identities.get(str(e.origin / e.plan["plan_file"])) == e.files[e.plan["plan_file"]], "supervisor plan identity differs")
    for suffix in ("experiments/hu-postflop-r1/flop-scaling/chance-grain/run.py", "tools/run_supervised.py"):
        name, value, _ = e.control(suffix)
        need(identities.get(name) == pair(value), "supervisor control identity differs")
    tools = e.plan["tools"]
    if row["kind"] in {"build", "tests", "toolchain"}:
        tool = tools["cargo" if row["kind"] in {"build", "tests"} else "rustc"]
        need(identities.get(tool["path"]) == pair(tool), "build tool identity differs")
    else:
        binary = binaries[row["arm"]]
        need(identities.get(binary["path"]) == pair(binary), "solver binary identity differs")
    for value in run.raw_outputs(record, e.origin / row["name"]):
        e.require(value)
    need(row["process_seconds"] == record["elapsed_seconds"] and 0 < row["process_seconds"] <= seconds + 1, "whole-process timer differs")
    need(row["root_os_peak_resident_bytes"] == record["last_sample"]["root_os_peak_resident_bytes"] > 0
         and row["root_os_peak_source"] == record["last_sample"]["root_os_peak_source"] == "wait4.ru_maxrss_linux_kib", "root RSS counter differs")
    return record


def verify_build(e):
    built, execution = read(e.root / "build.json"), read(e.root / "build-execution.json")
    need(built["schema"] == "r1.chance-grain-build/v1" and built["status"] == execution["status"] == "completed"
         and built["plan"] == e.files["plan.json"] and built["execution"] == e.files["build-execution.json"]
         and built["stages"] == execution["stages"] and built["binaries"] == execution["binaries"], "build receipt differs")
    need(execution["schema"] == "r1.chance-grain-execution/v1" and execution["phase"] == "build" and execution["receipt_file"] == "build-execution.json"
         and execution["plan"] == built["plan"] and execution["counts"] == {"completed": 3, "failed": 0, "skipped": 0}, "build execution schema/counts differ")
    need(utc(e.plan["created_at"]) <= utc(execution["started_at"]) <= utc(execution["ended_at"]) <= utc(e.plan["deadline_utc"]), "build execution deadline differs")
    expected_files = {n: v for n, v in e.files.items() if n == "plan.json" or n == "build-execution.json" or n == "source-baseline.tar.gz" or n == "binary-baseline.gz"
                      or n.startswith(("inputs/", "toolchain/", "build-baseline/", "tests-baseline/"))}
    need(built["files"] == expected_files, "immutable complete build evidence inventory differs")
    need(len(built["stages"]) == 3 and all(all(row[k] == v for k,v in wanted.items()) for row,wanted in zip(built["stages"],run.build_rows())), "build schedule differs")
    first, build, tests = built["stages"]
    tools = e.plan["tools"]
    need(first["command"] == [tools["rustc"]["path"], "-Vv"], "toolchain command differs")
    record = verify_stage(e, first, {})
    version = e.require(record["outputs"]["stdout"]).read_text()
    need(version == execution["rustc_version"] and "release: 1.97.0" in version and "host: x86_64-unknown-linux-gnu" in version, "compiler version differs")
    need(set(built["binaries"]) == {"baseline"}, "single binary inventory differs")
    need(utc(first["verified_at"]) <= utc(build["started_at"]), "build ordering differs")
    target = str(PurePosixPath(e.plan["workspace"]) / "baseline-target")
    expected = [tools["cargo"]["path"], "build", "--locked", "--offline", "--release", "-j2", "--target-dir", target,
                "-p", "holdem", "--example", run.EXAMPLE, "--message-format=json"]
    need(build["command"] == expected, "portable build command differs")
    record = verify_stage(e, build, {})
    artifacts = []
    for line in e.require(record["outputs"]["stdout"]).read_text().splitlines():
        message = run.loads(line)
        if message.get("reason") == "compiler-artifact" and message["target"]["name"] == run.EXAMPLE:
            need(message["target"]["kind"] == ["example"] and message["profile"]["opt_level"] == "3"
                 and not message["profile"]["test"] and not message["fresh"], "optimized fresh example profile differs")
            artifacts.append(message["executable"])
    binary = built["binaries"]["baseline"]
    need(artifacts == [binary["path"]] and binary["path"] == target + "/release/examples/" + run.EXAMPLE, "binary path/artifact count differs")
    with gzip.open(e.require(build["retained_binary"]), "rb") as stream:
        need(digest(stream, 128 * 1024**2) == pair(binary), "retained binary bytes differ")
    need(utc(build["verified_at"]) <= utc(tests["started_at"]) and tests["command"] == run.test_command(e.plan), "core test command/order differs")
    record = verify_stage(e, tests, {})
    run.validate_tests(e.require(record["outputs"]["stdout"]).read_text())
    need(utc(tests["verified_at"]) <= utc(execution["ended_at"]), "build terminal ordering differs")
    return built["binaries"], execution

def verify_solve(e, row, binaries, expected_canonical):
    verify_stage(e, row, binaries)
    affinity = e.plan["host"]["affinity"]
    output = str(e.origin / row["name"] / "artifacts")
    command = [binaries[row["arm"]]["path"], row["case"], str(row["workers"]), str(row["iterations"]), str(row["depth"]), output]
    need(command == row["command"] and row["expected_child_affinity"] == affinity, "child command/affinity differs")
    outputs = row["outputs"]
    names = {"invocation.json", "result.json", "quality.json", "state.bin"} | {"cpu.json", "grain.json"}
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
    cpu = read(e.require(outputs["cpu.json"]))
    run.validate_cpu(row, cpu, result, affinity)
    need(cpu == row["cpu"], "CPU receipt differs")
    grain = read(e.require(outputs["grain.json"]))
    run.validate_grain(row, grain)
    need(grain == row["grain"], "grain receipt differs")
    retention = read(e.require(row["retention_receipt"]))
    need(retention == row["state_retention"] and retention["fullbyte_verified"] is True and retention["raw_removed"] is True
         and retention["original"] == outputs["state.bin"], "durable raw removal receipt differs")
    canonical = retention["canonical"]
    need(pair(canonical["state"]) == pair(outputs["state.bin"]) and result["state_bytes"] == outputs["state.bin"]["bytes"], "full state identity differs")
    need(canonical["state"]["path"] == str(PurePosixPath(e.plan["workspace"]) / "canonical" / (run.canonical_key(row) + ".bin"))
         and e.name(canonical["state_gzip"]["path"]) == "canonical/" + run.canonical_key(row) + ".bin.gz", "canonical path differs")
    if expected_canonical is None:
        need(retention["kind"] == "new_canonical" and canonical["quality"] == outputs["quality.json"], "canonical ownership differs")
    else:
        need(retention["kind"] == "alias" and canonical == expected_canonical, "canonical alias differs")
        need(e.require(outputs["quality.json"]).read_bytes() == e.require(canonical["quality"]).read_bytes(), "quality full bytes differ")
    e.state(canonical, row["case"], row["iterations"])
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
    values = {"cfr_plus_quality_wall_seconds": result["cfr_seconds"] + result["quality_seconds"],
              "construction_wall_seconds": result["build_seconds"], "state_write_wall_seconds": result["state_write_seconds"],
              "whole_process_seconds": row["process_seconds"], "root_os_peak_resident_bytes": row["root_os_peak_resident_bytes"]}
    for name, (processor, wall) in intervals.items():
        values.update({name + "_cpu_seconds": processor, name + "_wall_seconds": wall,
                       name + "_cpu_over_wall": processor / wall,
                       name + "_cpu_over_wall_per_allowed_logical": processor / wall / len(row["expected_child_affinity"])})
    return values


def summarize(rows):
    expected = run.schedule()
    need(len(rows) == len(expected) == 38 and all(all(row[k] == v for k, v in wanted.items()) for row, wanted in zip(rows, expected))
         and all(row["status"] == "completed" for row in rows), "incomplete or reordered fixed schedule")
    groups = []
    for case in run.CASES:
        for depth in run.DEPTHS:
            for workers in run.WORKERS:
                selected = [r for r in rows if r["kind"] == "matrix" and r["case"] == case and r["depth"] == depth and r["workers"] == workers and not r["warmup"]]
                need([r["round"] for r in selected] == [1, 2, 3], "measured rounds differ")
                samples = [observations(r) for r in selected]
                groups.append({"case": case, "depth": depth, "workers": workers, "iterations": 16,
                               "stages": [r["name"] for r in selected], "metrics": {k: stats([s[k] for s in samples]) for k in samples[0]}})
    return summarize_groups(groups)


def summarize_groups(groups):
    need(len(groups) == 8 and {(g["case"], g["depth"], g["workers"]) for g in groups}
         == {(c, d, w) for c in run.CASES for d in run.DEPTHS for w in run.WORKERS}, "group matrix differs")
    lookup = {(g["case"], g["depth"], g["workers"]): g for g in groups}
    ratios = []
    for case in run.CASES:
        for workers in run.WORKERS:
            baseline = lookup[(case, 2, workers)]["metrics"]
            candidate = lookup[(case, 1, workers)]["metrics"]
            ratios.append({"case": case, "workers": workers,
                           "depth1_over_depth2_cfr_median": candidate["cfr_wall_seconds"]["median"] / baseline["cfr_wall_seconds"]["median"],
                           "depth1_over_depth2_quality_median": candidate["quality_7_walks_wall_seconds"]["median"] / baseline["quality_7_walks_wall_seconds"]["median"],
                           "depth1_over_depth2_rss_maximum": candidate["root_os_peak_resident_bytes"]["maximum"] / baseline["root_os_peak_resident_bytes"]["maximum"]})
    for group in groups:
        base16 = lookup[(group["case"], group["depth"], 16)]["metrics"]
        group["same_depth_16worker_relative"] = {}
        for metric in ("cfr_wall_seconds", "quality_7_walks_wall_seconds", "cfr_plus_quality_wall_seconds"):
            speedup = base16[metric]["median"] / group["metrics"][metric]["median"]
            group["same_depth_16worker_relative"][metric] = {"speedup": speedup, "relative_efficiency": speedup / (group["workers"] / 16)}
    guards = {
        "both_inputs_32worker_cfr_ratio_at_most_0_95": all(r["depth1_over_depth2_cfr_median"] <= .95 for r in ratios if r["workers"] == 32),
        "both_inputs_16worker_cfr_ratio_at_most_1_03": all(r["depth1_over_depth2_cfr_median"] <= 1.03 for r in ratios if r["workers"] == 16),
        "all_quality_median_ratios_at_most_1_05": all(r["depth1_over_depth2_quality_median"] <= 1.05 for r in ratios),
        "all_root_rss_maximum_ratios_at_most_1_10": all(r["depth1_over_depth2_rss_maximum"] <= 1.10 for r in ratios),
        "all_cfr_quality_three_sample_max_over_min_at_most_1_15": all(g["metrics"][m]["max_over_min"] is not None and g["metrics"][m]["max_over_min"] <= 1.15
            for g in groups for m in ("cfr_wall_seconds", "quality_7_walks_wall_seconds"))}
    return {"groups": groups, "comparisons": ratios, "predeclared_guards": guards,
            "performance_screen": "passed" if all(guards.values()) else "rejected", "production_adoption": False,
            "adoption_pending": "Screen only; full workspace fmt/clippy/tests, broader applicability and code review are required before changing production defaults"}


def check(root):
    e = Evidence(root)
    verify_plan(e)
    build_host = e.plan["host"]
    binaries, build_execution = verify_build(e)
    verify_measurement(e, build_execution)
    execution = read(e.root / "execution.json")
    need(execution["schema"] == "r1.chance-grain-execution/v1" and execution["phase"] == "measure" and execution["receipt_file"] == "execution.json"
         and execution["plan"] == e.files["measurement.json"] and execution["binaries"] == binaries, "measurement execution binding differs")
    need(execution["status"] == "completed", "incomplete proof: performance screen is not evaluable")
    need(len(execution["stages"]) == 38 and execution["counts"] == {"completed": 38, "failed": 0, "skipped": 0}, "fixed stage counts differ")
    need(utc(e.plan["created_at"]) <= utc(execution["started_at"]) <= utc(execution["ended_at"]) <= utc(e.plan["deadline_utc"]), "measurement deadline differs")
    canonical, frontiers = {}, {}
    previous = utc(e.plan["created_at"])
    for row, expected in zip(execution["stages"], run.schedule()):
        need(all(row[k] == value for k, value in expected.items()), "fixed solve schedule differs")
        need(previous <= utc(row["started_at"]), "solve stage ordering overlaps")
        key = run.canonical_key(row)
        canonical[key] = verify_solve(e, row, binaries, canonical.get(key))
        shape = {k:v for k,v in row["grain"].items() if k != "cfr_depth"}
        need(row["case"] not in frontiers or frontiers[row["case"]] == shape, "frontier observations differ within one fixture")
        frontiers[row["case"]] = shape
        previous = utc(row["verified_at"])
    need(previous <= utc(execution["ended_at"]), "terminal measurement precedes last verification")
    need(canonical == execution["canonical"] and set(canonical) == {"smoke-narrow", *run.CASES} and len(e.states) == 3, "canonical final inventory differs")
    return {"schema": "r1.chance-grain-report/v1", "status": "completed", "payload_integrity": "verified",
            "build_plan": e.files["plan.json"], "measurement_plan": e.files["measurement.json"], "execution": e.files["execution.json"],
            "build_host": build_host, "measurement_host": e.plan["host"], "frontier_observations": frontiers,
            "counts": {"build_commands": 1, "core_test_commands": 1, "smoke": 4, "canonical": 2, "warmup": 8, "measured": 24},
            **summarize(execution["stages"]),
            "limits": ["Two fixed synthetic Flop inputs, F32/DCFR16 iterations and CFV capture disabled; not an external quality certification",
                       "Same portable x86-64-v3 binary in all measurement conditions; no absolute comparison to earlier native builds",
                       "CFR depth changes only; quality is reset to depth2 before all seven quality traversals",
                       "Structural frontier counts are not Rayon tasks, iterations, seat passes or measured scheduler overhead",
                       "Process CPU includes spinning, allocation and scheduling; CPU/wall does not identify a cause",
                       "Guest16core/32logical topology does not prove dedicated physical cores or 32physical-core scaling",
                       "Root wait4 ru_maxrss is whole-process Linux high-water RSS, not phase or aggregate child memory",
                       "Warmup/smoke/canonical excluded; no adaptive iterations, retries or mixed measurement boots",
                       "Build and measurement boots intentionally differ on one instance; all measured rows share one boot",
                       "Predeclared screen alone does not authorize a production/default change"]}

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
        report = {"schema": "r1.chance-grain-report/v1", "status": "not_evaluable", "error": str(error),
                  "payload_integrity": "not_verified", "groups": [], "performance_screen": "not_evaluable", "production_adoption": False, "performance_claims": False}
        code = 2
    with args.report.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(report, stream, indent=2, sort_keys=True, allow_nan=False)
        stream.write("\n")
    print(json.dumps({"status": report["status"], "payload_integrity": report["payload_integrity"]}))
    return code


if __name__ == "__main__":
    raise SystemExit(main())
