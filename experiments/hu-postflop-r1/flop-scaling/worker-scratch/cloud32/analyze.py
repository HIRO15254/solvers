"""Read-only portable evidence verification and fixed Cloud32 acceptance arithmetic.

Never imports retained code. This command streams retained gzip states and is
intended for the authorized cloud/recovery check, not an unbounded local probe.
"""
from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import statistics
import struct
import tarfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
WORKERS = (1, 4, 16, 32)
CASES = ("narrow", "expanded")
ARMS = ("baseline", "worker")
PILOTS = (16, 32, 64, 128)
SOLVER = "crates/engine/src/solver.rs"
EXAMPLE = "crates/holdem/examples/flop_cloud32_probe.rs"
SOLVERS = {"baseline": "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a",
           "worker": "c9549cc7cf433ccc38ee2da43b11a6c9a4970b7e0093f7304ff1053cb6143667"}
ADAPTER = "63cba37a5b0f79dfed2ca0ddf879daada3f0fee094ee5f287f86bc1f37414d46"
HEADERS = {"narrow": (367662, 147104, 34, 30, 10176768, 10176768),
           "expanded": (367662, 147104, 63, 160, 35459676, 35459676)}
LIMITS = {"grace_seconds": 0.2, "kill_wait_seconds": 5, "poll_seconds": 0.1,
          "memory_limit_bytes": 8 * 1024**3, "min_free_memory_bytes": 2 * 1024**3,
          "disk_reserve_bytes": 2 * 1024**3}
METRICS = ("cfr_seconds", "quality_7_walk_seconds", "cfr_plus_quality_seconds",
           "construction_seconds", "state_write_seconds", "whole_process_seconds",
           "root_os_peak_resident_bytes")


def need(condition, message):
    if not condition:
        raise ValueError(message)


def read(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def pair(value):
    return {"bytes": value["bytes"], "sha256": value["sha256"]}


def digest(stream, limit=1024**3):
    sha, length = hashlib.sha256(), 0
    while block := stream.read(1024**2):
        length += len(block)
        need(length <= limit, "stream exceeds fixed verification bound")
        sha.update(block)
    return {"bytes": length, "sha256": sha.hexdigest()}


def pin(path):
    with Path(path).open("rb") as stream:
        return digest(stream)


def relative(name):
    value = PurePosixPath(name)
    need(not value.is_absolute() and value.parts and ".." not in value.parts
         and "\\" not in name and ":" not in name and str(value) == name, "unsafe evidence path")
    return value


def utc(value):
    parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    need(parsed.utcoffset() == dt.timedelta(0), "timestamp is not UTC")
    return parsed


def schedule():
    rows = []
    for case in CASES:
        for r in range(4):
            order = WORKERS if r % 2 == 0 else WORKERS[::-1]
            for index, worker in enumerate(order):
                arms = ARMS if (r + index) % 2 == 0 else ARMS[::-1]
                for arm in arms:
                    rows.append({"name": f"{case}-r{r}-w{worker}-{arm}", "case": case,
                                 "round": r, "warmup": r == 0, "workers": worker, "arm": arm})
    return rows


def fixed_rows(rows):
    expected = schedule()
    need(len(rows) == 64, "expected 64 matrix rows")
    for actual, wanted in zip(rows, expected):
        need(all(actual.get(k) == v for k, v in wanted.items()), "matrix condition/order differs")
    need(sum(x["warmup"] for x in rows) == 16, "expected 16 warmup and 48 measured rows")


class Evidence:
    def __init__(self, root):
        self.root = Path(root).resolve(strict=True)
        self.manifest = read(self.root / "retained.json")
        need(self.manifest["schema"] == "r1.worker-scratch-cloud32-retained/v1", "retained schema differs")
        self.files = self.manifest["files"]
        verified = {}
        actual = set()
        for path in self.root.rglob("*"):
            need(not path.is_symlink(), "evidence symlink is not accepted")
            if path.is_file():
                actual.add(path.relative_to(self.root).as_posix())
            else:
                need(path.is_dir(), "nonregular evidence entry")
        extras = actual - set(self.files) - {"retained.json"}
        if extras:
            recovery = read(self.root / "recovery-manifest.json")
            need(recovery["schema"] == "r1.worker-scratch-vm15-recovery/v1" and recovery["unit_quiescence_checked"],
                 "recovery envelope differs")
            recovered = {v["member"]: v for v in recovery["files"]}
            need(len(recovered) == len(recovery["files"])
                 and set(recovered) == actual - {"recovery-manifest.json"}, "recovery membership differs")
            need(all(n == "recovery-manifest.json" or n.startswith("recovery/") for n in extras),
                 "unrecognized extra proof file")
            for name, value in recovered.items():
                verified[name] = pin(self.path(name))
                need(verified[name] == pair(value), "recovery payload pin differs: " + name)
        need(set(self.files).issubset(actual), "retained payload missing")
        for name, value in self.files.items():
            actual_pin = verified[name] if name in verified else pin(self.path(name))
            need(actual_pin == pair(value), "retained payload pin differs: " + name)
        need(self.manifest["stored_bytes"] == sum(v["bytes"] for v in self.files.values()), "retained byte total differs")
        self.plan = read(self.path("plan.json"))
        self.origin = PurePosixPath(self.plan["output"])
        self.state_cache = {}

    def path(self, name):
        path = self.root.joinpath(*relative(name).parts)
        need(path.resolve().is_relative_to(self.root), "evidence path leaves proof")
        return path

    def name(self, original):
        return PurePosixPath(original).relative_to(self.origin).as_posix()

    def require(self, value):
        name = self.name(value["path"])
        need(name in self.files and self.files[name] == pair(value), "artifact not bound to retained payload: " + name)
        return self.path(name)

    def control(self, suffix):
        matches = [(name, value) for name, value in self.plan["controls"].items() if name.endswith("/" + suffix)]
        need(len(matches) == 1, "control missing or ambiguous: " + suffix)
        original, value = matches[0]
        need(self.files.get(value["retained"]) == pair(value), "retained control pin differs")
        return original, value, self.path(value["retained"])

    def state(self, canonical, case, iterations):
        compressed = self.require(canonical["state_gzip"])
        key = str(compressed)
        expected = pair(canonical["state"])
        header = (iterations, iterations, *HEADERS[case])
        expected_size = 72 + 2 * sum(header[4:6]) + 4 * sum(header[6:8])
        need(expected["bytes"] == expected_size, "canonical F32 byte count differs")
        wanted = b"R1F32S01" + struct.pack("<8Q", *header)
        if key not in self.state_cache:
            with gzip.open(compressed, "rb") as stream:
                first = stream.read(72)
                need(first == wanted, "canonical F32 header differs")
                sha, length = hashlib.sha256(first), len(first)
                while chunk := stream.read(1024**2):
                    length += len(chunk)
                    need(length <= expected_size, "canonical gzip exceeds F32 size")
                    sha.update(chunk)
            self.state_cache[key] = ({"bytes": length, "sha256": sha.hexdigest()}, first)
        actual, first = self.state_cache[key]
        need(actual == expected and first == wanted, "canonical full stream pin/header differs")


def verify_environment(plan):
    need(plan["environment"] == {"RUSTC": plan["tools"]["rustc"]["path"], "RUSTFLAGS": "-C target-cpu=native",
                                 "CARGO_BUILD_JOBS": "2", "CARGO_INCREMENTAL": "0", "RAYON_NUM_THREADS": "1"},
         "native build environment differs")


def verify_sources(evidence):
    plan = evidence.plan
    need(plan["schema"] == "r1.worker-scratch-cloud32/v1" and plan["limits"] == LIMITS, "plan/schema limits differ")
    fixed_rows(plan["matrix"])
    need(all(x["status"] == "pending" for x in plan["matrix"]) and plan["pilot_iterations"] == list(PILOTS), "prepared schedule differs")
    need(plan["same_boot_build_required"] is True, "same-boot build guard missing")
    verify_environment(plan)
    manifest_original, manifest_value, manifest_path = evidence.control("manifest.json")
    _, _, installation_path = evidence.control("installation.json")
    manifest, installation = read(manifest_path), read(installation_path)
    need(manifest["schema"] == "r1-worker-scratch-cloud32-package/v1", "package schema differs")
    package_root = str(PurePosixPath(manifest_original).parent)
    need(installation["manifest"] == pair(manifest_value) == plan["package"]["manifest"]
         and installation["destination"] == package_root and installation["builds_or_solves_started"] == 0
         and installation["archive_sha256"] == plan["package"]["archive_sha256"]
         and installation["source_revision"] == manifest["source_revision"] == plan["package"]["source_revision"]
         and installation["source_files"] == len(manifest["source_pins"]), "installation package binding differs")
    for original, value in plan["controls"].items():
        need(evidence.files.get(value["retained"]) == pair(value), "captured control differs")
        name = PurePosixPath(original).relative_to(package_root).as_posix()
        if name not in {"manifest.json", "installation.json"}:
            need(manifest["files"].get(name) == pair(value), "control not package-pinned")
    # Compare to trusted checkout bytes, never execute a captured checker/runner.
    for path in (HERE / "runner.py", HERE / "solve.rs", HERE / "protocol.md", HERE / "install.py",
                 HERE / "prepare.py", HERE / "provenance.json", HERE / "adapter.patch",
                 ROOT / "experiments/hu-postflop-r1/flop-scaling/flat-ev/timing/run.py", HERE.parent.parent / "ev-scratch/run.py", ROOT / "tools/run_supervised.py", HERE.parent / "durable.py"):
        _, value, _ = evidence.control(path.relative_to(ROOT).as_posix())
        need(pair(value) == pin(path), "trusted checkout control differs: " + path.name)
    need(pin(HERE / "solve.rs")["sha256"] == ADAPTER, "trusted adapter pin differs")
    for arm in ARMS:
        source = plan["sources"][arm]
        expected = {**manifest["source_pins"], EXAMPLE: pin(HERE / "solve.rs")}
        if arm == "worker":
            expected[SOLVER] = manifest["candidate"]
        need(source["files"] == expected and expected[SOLVER]["sha256"] == SOLVERS[arm], "arm source binding differs")
        archive = evidence.require(source["archive"])
        observed, total = {}, 0
        with tarfile.open(archive, "r|gz") as tar:
            for item in tar:
                name = relative(item.name).as_posix()
                need(item.isfile() and name not in observed and item.size <= 2 * 1024**2, "source archive member differs")
                total += item.size
                need(total <= 16 * 1024**2 and len(observed) < 1024, "source archive bound exceeded")
                with tar.extractfile(item) as stream:
                    observed[name] = digest(stream, 2 * 1024**2)
        need(observed == expected, "source archive contents differ")
    host = plan["host"]
    need(host["machine"] == "x86_64" and host["logical_cpus"] == 32 and len(set(host["affinity"])) == 32
         and host["boot_id"] and host["physical_cores"] == len({(x["core"], x["socket"]) for x in host["topology"]}), "host topology differs")
    cg = host["cgroup"]
    need(cg["memory_max"] == str(12 * 1024**3) and cg["swap_max"] == "0" and cg["cpu_weight"] == "100", "outer service limits differ")
    need(cg["cpu_limits"], "CPU quota record missing")
    for value in cg["cpu_limits"].values():
        quota, period = value.split()
        need(quota == "max" or int(quota) / int(period) >= 32, "CPU quota below 32")


def verify_terminal(record):
    need(record["schema"] == "solvers.supervised-run/v1" and record["state"] == "completed"
         and record["child_exit_code"] == record["supervisor_exit_code"] == 0
         and record["cleanup_complete"] is True and not record["forced"] and record["last_sample"]["pids"] == []
         and record["stop_reason"] == "completed" and not record["errors"], "supervisor terminal/cleanup differs")


def verify_stage(evidence, item, binary=None):
    need(item["status"] == "completed" and item["supervisor_exit"] == 0, "stage not successfully terminal")
    need(item["host_before"] == item["host_after"] == evidence.plan["host"], "stage host/boot changed")
    need(utc(item["started_at"]) <= utc(item["ended_at"]) <= utc(item["verified_at"])
         <= utc(evidence.plan["deadline_utc"]), "stage time/deadline differs")
    record_path = evidence.require(item["record"])
    need(evidence.name(item["record"]["path"]) == item["name"] + "/supervisor.json", "stage record path differs")
    record = read(record_path)
    verify_terminal(record)
    need(record["argv"] == record["resolved_argv"] == item["command"] and record["shell"] is False, "workload argv differs")
    seconds = 300 if item.get("kind") in {"build", "tests"} else 10 if item.get("kind") == "toolchain" else 120
    need(record["limits"] == {**LIMITS, "timeout_seconds": seconds}, "supervisor resource bounds differ")
    need(record["runtime"]["logical_cpus"] == 32 and record["runtime"]["machine"] == "x86_64", "supervisor CPU differs")
    need(record["identity_unchanged"] is True and record["identity_before"] == record["identity_after"], "stage identity changed")
    identities = {v["path"]: pair(v) for v in record["identity_before"]}
    need(identities.get(str(evidence.origin / "plan.json")) == evidence.files["plan.json"], "supervisor plan binding differs")
    for suffix in ("experiments/hu-postflop-r1/flop-scaling/worker-scratch/cloud32/runner.py", "tools/run_supervised.py"):
        original, value, _ = evidence.control(suffix)
        need(identities.get(original) == pair(value), "supervisor control identity differs")
    expected_exe = binary or evidence.plan["tools"]["cargo" if item.get("kind") in {"build", "tests"} else "rustc"]
    need(identities.get(item["command"][0]) == pair(expected_exe), "supervisor executable differs")
    for value in record["outputs"].values():
        evidence.require(value)
    peak = record["last_sample"]["root_os_peak_resident_bytes"]
    need(isinstance(peak, int) and peak > 0 and peak == item["root_os_peak_resident_bytes"]
         and item["root_os_peak_source"] == record["last_sample"]["root_os_peak_source"] == "wait4.ru_maxrss_linux_kib", "root RSS counter differs")
    need(item["process_seconds"] == record["elapsed_seconds"] and math.isfinite(item["process_seconds"])
         and 0 < item["process_seconds"] <= seconds + 1, "whole process timer differs")


def verify_solve(evidence, item, binaries, expected_canonical=None):
    verify_stage(evidence, item, binaries[item["arm"]])
    expected_dir = str(evidence.origin / item["name"] / "artifacts")
    need(item["command"] == [binaries[item["arm"]]["path"], item["case"], str(item["workers"]),
                             str(item["iterations"]), expected_dir], "solve command condition differs")
    outputs = item["outputs"]
    need(set(outputs) == {"invocation.json", "result.json", "quality.json", "state.bin"}, "solver artifact set differs")
    artifact_prefix = item["name"] + "/artifacts/"
    need({n.removeprefix(artifact_prefix) for n in evidence.files if n.startswith(artifact_prefix)}
         == {"invocation.json", "result.json", "quality.json"}, "retained solver artifact membership differs")
    for name, value in outputs.items():
        need(value["path"] == expected_dir + "/" + name, "solver artifact path differs")
        if name != "state.bin":
            evidence.require(value)
    result, invocation, quality = [read(evidence.require(outputs[n])) for n in ("result.json", "invocation.json", "quality.json")]
    need(result == item["result"] and result["status"] == "completed", "result receipt differs")
    for value in (invocation, result):
        need(value["case"] == item["case"] and value["threads"] == item["workers"]
             and value["iterations"] == item["iterations"], "solve condition differs")
    need(invocation["planned_iterations"] == item["iterations"] and invocation["chance_depth"] == 2
         and invocation["min_children"] == 12 and invocation["storage"] == "f32" and invocation["schedule"] == "dcfr"
         and invocation["alpha"] == 1.5 and invocation["beta"] == 0 and invocation["gamma"] == 3
         and invocation["pow4_reset"] is True and invocation["quality_target"] is None and invocation["cfv_capture"] is False,
         "solver semantics differ")
    need(quality["case"] == item["case"] and quality["iterations"] == item["iterations"]
         and quality["root_support"] == list(HEADERS[item["case"]][2:4]) and quality["quality_target"] is None,
         "quality input differs")
    normalizer = 870.0 if item["case"] == "narrow" else 8700.0
    need(quality["normalizer_bits"] == struct.pack(">d", normalizer).hex(), "quality normalizer differs")
    for metric in ("ev", "br", "exploitability"):
        need(len(quality[metric]) == len(quality[metric + "_bits"]) == 2, "quality vector size differs")
        for value, bits in zip(quality[metric], quality[metric + "_bits"]):
            need(math.isfinite(value) and struct.pack(">d", value).hex() == bits, "quality number/bits differ")
    for metric in ("build_seconds", "cfr_seconds", "state_write_seconds", "quality_seconds"):
        need(math.isfinite(result[metric]) and result[metric] > 0, "invalid phase timer")
    retention = item["state_retention"]
    canonical = retention["canonical"]
    need(retention["raw_removed"] is True and retention["original"] == outputs["state.bin"]
         and pair(canonical["state"]) == pair(outputs["state.bin"])
         and result["state_bytes"] == outputs["state.bin"]["bytes"], "state retention binding differs")
    need(canonical["state"]["path"] == str(PurePosixPath(evidence.plan["workspace"]) / "canonical" / (outputs["state.bin"]["sha256"] + ".bin")), "canonical raw alias path differs")
    need(evidence.name(canonical["state_gzip"]["path"]) == "canonical/" + outputs["state.bin"]["sha256"] + ".bin.gz", "canonical gzip path differs")
    if expected_canonical is None:
        need(retention["kind"] == "new_canonical" and retention["gzip_fullbyte_verified"] is True
             and item["canonical"] is None and item["fullstate_and_quality_bytes_equal_canonical"] is False,
             "new canonical proof differs")
        need(canonical["quality"] == outputs["quality.json"], "new canonical quality differs")
    else:
        need(retention["kind"] == "alias" and retention["fullbyte_equal"] is True
             and item["fullstate_and_quality_bytes_equal_canonical"] is True
             and item["canonical"] == canonical == expected_canonical, "full-byte comparison receipt differs")
        need(evidence.require(outputs["quality.json"]).read_bytes() == evidence.require(canonical["quality"]).read_bytes(), "quality bytes differ from canonical")
    need(not evidence.path(evidence.name(outputs["state.bin"]["path"])).exists(), "removed raw state unexpectedly present")
    evidence.state(canonical, item["case"], item["iterations"])
    return canonical


def stats(values):
    need(len(values) == 3 and all(math.isfinite(x) and x > 0 for x in values), "three positive finite measurements required")
    return {"samples": values, "median": statistics.median(values), "minimum": min(values),
            "maximum": max(values), "max_over_min": max(values) / min(values)}


def summarize(rows):
    fixed_rows(rows)
    need(all(r["status"] == "completed" for r in rows), "incomplete matrix has no adoption statistics")
    groups = []
    for case in CASES:
        for arm in ARMS:
            for worker in WORKERS:
                selected = [r for r in rows if r["case"] == case and r["arm"] == arm and r["workers"] == worker and not r["warmup"]]
                need([r["round"] for r in selected] == [1, 2, 3], "measured rounds differ")
                values = [{"cfr_seconds": r["result"]["cfr_seconds"], "quality_7_walk_seconds": r["result"]["quality_seconds"],
                           "cfr_plus_quality_seconds": r["result"]["cfr_seconds"] + r["result"]["quality_seconds"],
                           "construction_seconds": r["result"]["build_seconds"], "state_write_seconds": r["result"]["state_write_seconds"],
                           "whole_process_seconds": r["process_seconds"], "root_os_peak_resident_bytes": r["root_os_peak_resident_bytes"]} for r in selected]
                groups.append({"case": case, "arm": arm, "workers": worker, "iterations": selected[0]["iterations"],
                               "stages": [r["name"] for r in selected], "metrics": {m: stats([v[m] for v in values]) for m in METRICS}})
    lookup = {(g["case"], g["arm"], g["workers"]): g for g in groups}
    for g in groups:
        for metric, value in g["metrics"].items():
            if metric != "root_os_peak_resident_bytes":
                value["speedup_vs_1_worker"] = lookup[g["case"], g["arm"], 1]["metrics"][metric]["median"] / value["median"]
                value["efficiency"] = value["speedup_vs_1_worker"] / g["workers"]
    ratios = [{"case": c, "workers": w,
               "time_worker_over_baseline": lookup[c, "worker", w]["metrics"]["cfr_plus_quality_seconds"]["median"] / lookup[c, "baseline", w]["metrics"]["cfr_plus_quality_seconds"]["median"],
               "rss_max_worker_over_baseline": lookup[c, "worker", w]["metrics"]["root_os_peak_resident_bytes"]["maximum"] / lookup[c, "baseline", w]["metrics"]["root_os_peak_resident_bytes"]["maximum"]} for c in CASES for w in WORKERS]
    noise = [{"case": g["case"], "arm": g["arm"], "workers": g["workers"], "max_over_min": g["metrics"]["cfr_plus_quality_seconds"]["max_over_min"]}
             for g in groups if g["metrics"]["cfr_plus_quality_seconds"]["max_over_min"] > 1.15]
    checks = {"one_worker_time_at_most_1_05": all(v["time_worker_over_baseline"] <= 1.05 for v in ratios if v["workers"] == 1),
              "all_condition_peak_at_most_1_10": all(v["rss_max_worker_over_baseline"] <= 1.10 for v in ratios),
              "both_inputs_sixteen_workers_time_at_most_0_90": all(v["time_worker_over_baseline"] <= 0.90 for v in ratios if v["workers"] == 16),
              "all_time_spreads_at_most_1_15": not noise}
    decision = "deferred_noise" if noise else "passes_local_guard" if all(checks.values()) else "does_not_pass_local_guard"
    return {"groups": groups, "arm_ratios": ratios, "guard": {"decision": decision, "checks": checks, "noisy_conditions": noise}}


def verify_complete(evidence):
    verify_sources(evidence)
    plan = evidence.plan
    build, execution = read(evidence.path("build.json")), read(evidence.path("execution.json"))
    for receipt, phase in ((build, "build"), (execution, "matrix")):
        need(receipt["schema"] == "r1.worker-scratch-cloud32-phase/v1" and receipt["phase"] == phase
             and receipt["status"] == "completed" and receipt["plan"] == evidence.files["plan.json"], "phase incomplete or unbound")
        need(receipt["counts"] == {"completed": len(receipt["stages"]), "failed": 0, "skipped": 0}, "phase counts differ")
        completed = [s for s in receipt["pilot_stages"] + receipt["stages"] if s["status"] == "completed"]
        need(completed and utc(receipt["started_at"]) <= min(utc(s["started_at"]) for s in completed)
             and max(utc(s["verified_at"]) for s in completed) <= utc(receipt["ended_at"])
             <= utc(plan["deadline_utc"]), "phase timestamps differ")
    build_names = ["toolchain", "build-baseline", "build-worker", "tests-worker", "smoke-narrow-baseline-1", "smoke-narrow-worker-1", "smoke-narrow-baseline-32", "smoke-narrow-worker-32"]
    need([v["name"] for v in build["stages"]] == build_names and not build["pilot_stages"], "build/smoke schedule differs")
    binaries = build["binaries"]
    need(set(binaries) == set(ARMS) and execution["binaries"] == binaries
         and execution["build"] == evidence.files["build.json"], "binary/build phase binding differs")
    for item in build["stages"][:4]:
        verify_stage(evidence, item)
        if item["kind"] == "build":
            arm = item["arm"]
            target = str(PurePosixPath(plan["workspace"]) / f"{arm}-target")
            need(item["command"] == [plan["tools"]["cargo"]["path"], "build", "--locked", "--offline", "--release", "-j2", "--target-dir", target,
                                     "-p", "holdem", "--example", "flop_cloud32_probe", "--message-format=json"], "release build command differs")
            need(item["artifact"] == binaries[arm] and binaries[arm]["path"] == target + "/release/examples/flop_cloud32_probe", "native binary path differs")
            with gzip.open(evidence.require(item["retained_binary"]), "rb") as stream:
                need(digest(stream, 64 * 1024**2) == pair(binaries[arm]), "retained native binary differs")
    test_item = build["stages"][3]
    need(test_item["kind"] == "tests" and test_item["arm"] == "worker", "candidate tests missing")
    need(test_item["command"] == [plan["tools"]["cargo"]["path"], "test", "--locked", "--offline", "--release", "-j2",
                                  "--target-dir", str(PurePosixPath(plan["workspace"]) / "worker-target"),
                                  "-p", "engine", "-p", "holdem", "-p", "cfr-ref", "--tests"], "candidate test command differs")
    need("release: 1.97.0" in build["rustc_version"] and "host: x86_64-unknown-linux-gnu" in build["rustc_version"], "native compiler version differs")
    smoke = None
    for item, (arm, workers) in zip(build["stages"][4:], (("baseline", 1), ("worker", 1), ("baseline", 32), ("worker", 32))):
        need((item["case"], item["iterations"], item["arm"], item["workers"]) == ("narrow", 2, arm, workers), "smoke condition differs")
        smoke = verify_solve(evidence, item, binaries, smoke)
    need(build["smoke_canonical"] == smoke, "smoke canonical differs")
    pilots, selected = execution["pilot_stages"], execution["selected"]
    need(len(pilots) == 8 and set(selected) == set(CASES), "pilot schedule incomplete")
    expected_pilots = [(c, n) for c in CASES for n in PILOTS]
    found = {}
    for item, (case, n) in zip(pilots, expected_pilots):
        need(item["name"] == f"pilot-{case}-{n}" and (item["case"], item["iterations"], item["arm"], item["workers"]) == (case, n, "baseline", 1), "pilot condition differs")
        if case in found:
            need(item["status"] == "skipped" and item["reason"] == "baseline already selected smaller N", "unselected pilot was run")
            continue
        canonical = verify_solve(evidence, item, binaries)
        seconds = item["result"]["cfr_seconds"]
        if seconds >= 4 or n == 128:
            found[case] = {"iterations": n, "cfr_seconds": seconds, "short_timing": seconds < 4, "canonical": canonical, "stage": item["name"]}
    need(found == selected, "pilot selection is not baseline-only first threshold/cap")
    need(execution["pilot_counts"] == {s: sum(p["status"] == s for p in pilots) for s in ("completed", "failed", "skipped")}, "pilot counts differ")
    rows = execution["stages"]
    fixed_rows(rows)
    for item in rows:
        need(item["iterations"] == selected[item["case"]]["iterations"], "case iteration count changed")
        verify_solve(evidence, item, binaries, selected[item["case"]]["canonical"])
    ordered = build["stages"] + [v for v in pilots if v["status"] == "completed"] + rows
    need(all(utc(a["verified_at"]) <= utc(b["started_at"]) for a, b in zip(ordered, ordered[1:])), "workloads overlapped or order differs")
    report = summarize(rows)
    report.update(status="completed", payload_integrity="verified", same_boot=plan["host"],
                  counts={"native_build_processes": 2, "toolchain_processes": 1, "smoke_processes": 4, "pilot_processes": sum(p["status"] == "completed" for p in pilots),
                          "matrix_processes": 64, "warmup": 16, "measured": 48},
                  selected={c: {k: v for k, v in selected[c].items() if k != "canonical"} for c in CASES},
                  compile_stages=[{"arm": s["arm"], "seconds": s["process_seconds"], "root_os_peak_resident_bytes": s["root_os_peak_resident_bytes"]} for s in build["stages"][1:3]],
                  canonical_quality={c: read(evidence.require(selected[c]["canonical"]["quality"])) for c in CASES},
                  unique_state_streams_verified=len(evidence.state_cache), fullstate_and_quality_guard=True,
                  evidence={name: evidence.files[name] for name in ("plan.json", "build.json", "execution.json")})
    return report


def analyze(root):
    result = {"schema": "r1.worker-scratch-cloud32-analysis/v1", "status": "not_evaluable", "payload_integrity": "not_verified",
              "guard": {"decision": "not_evaluable"}, "counts": {},
              "limitations": ["Fixed iterations on two typed Flop fixtures; no convergence target, external certification, or whole-R1 acceptance.",
                              "Quality timer covers seven public value traversals as one interval; individual walk times are unavailable.",
                              "RSS is Linux wait4 ru_maxrss for the root process, not a simultaneous physical-memory or process-tree peak.",
                              "Each retained canonical gzip is fully rehashed. Removed duplicate bytes cannot be replayed; their equality is the pinned runner's pre-removal byte-comparison receipt.",
                              "Guard passage permits considering adoption; production checks remain required. Background workload is not isolated."]}
    try:
        evidence = Evidence(root)
        result["payload_integrity"] = "verified"
        for name in ("build.json", "execution.json"):
            if name in evidence.files:
                receipt = read(evidence.path(name))
                result["counts"][name] = {"status": receipt.get("status"), "counts": receipt.get("counts"), "pilot_counts": receipt.get("pilot_counts")}
        result.update(verify_complete(evidence))
    except (ValueError, KeyError, TypeError, OSError, EOFError, tarfile.TarError) as error:
        result["error"] = str(error)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proof", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    need(not args.out.exists() and not args.out.resolve().is_relative_to(args.proof.resolve()), "new report outside proof required")
    result = analyze(args.proof)
    result["analyzer"] = pin(Path(__file__))
    with args.out.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(result, stream, indent=2, ensure_ascii=False, allow_nan=False)
        stream.write("\n")
    print(json.dumps({"status": result["status"], "guard": result["guard"]["decision"], "out": str(args.out)}))
    return 0 if result["status"] == "completed" else 2


if __name__ == "__main__":
    raise SystemExit(main())
