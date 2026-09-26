"""Render verified evidence: --out PROOF [--json-target FILE] [--markdown-target FILE]."""
from collections import Counter
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("trusted_final_report_runner", HERE / "run.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)
ARMS = ("old", "new")
CODECS = ("decode-all", "read-root", "stream-write")


def counts(document):
    return dict(sorted(Counter(row["status"] for row in document["stages"]).items()))


def build_report(out):
    """Always check raw proof with trusted code; never accept a saved verdict."""
    out = Path(out)
    prepare_failed = (out / "prepare-failure.json").exists()
    names = ("prepare-failure.json",) if prepare_failed else ("plan.json", "build.json", "result.json")
    before = {name: (out / name).read_bytes() for name in names}
    verified = runner.check(out)
    runner.require(all((out / name).read_bytes() == data for name, data in before.items()), "report input changed during verification")
    runner.require(verified["payload_integrity"] == "verified", "unverified report input")
    report = {
        "schema": "r1.final-pipeline-readable-report/v1", "status": verified["status"],
        "verification": {key: verified[key] for key in ("schema", "status", "build_status", "processes_passed", "payload_integrity", "limitations") if key in verified},
        "input_sha256": {name: hashlib.sha256(data).hexdigest() for name, data in before.items()},
        "stage_counts": None, "failures": [], "performance": None,
        "scope": "Three synthetic no-rake HU games, one worker, F32 Full artifacts; no overall R1 certification.",
    }
    if prepare_failed:
        report["failures"] = [{"phase": "prepare", "error": verified["error"]}]
        report["scope"] = verified["scope"]
        return report
    plan, builds, result = (json.loads(before[name]) for name in names)
    report["stage_counts"] = {"build": counts(builds), "measurement": counts(result)}
    for phase, document in (("build", builds), ("measurement", result)):
        if document.get("error"):
            report["failures"].append({"phase": phase, "error": document["error"]})
        report["failures"].extend({"phase": phase, "stage": row["stage"]["label"], "error": row.get("error")}
                                  for row in document["stages"] if row["status"] == "failed")
    if verified["status"] != "completed":
        runner.require(verified.get("summary") is None, "partial evidence must not claim performance")
        return report
    protocol, summary = plan["protocol"], verified["summary"]
    cases = {}
    for case, value in summary["cases"].items():
        solve = value["solve"]
        live_nc = {arm: [row["sample"]["live"]["nashConv"] for row in result["stages"]
                        if row["status"] == "passed" and row["stage"]["case"] == case
                        and row["stage"]["arm"] == arm and row["stage"]["kind"] == "solve"
                        and not row["stage"]["warmup"]] for arm in ARMS}
        saved_nc = {arm: [q["nash_conv"] for q in value["audit"]["saved_quality"][arm]] for arm in ARMS}
        runner.require(all(len(live_nc[arm]) == len(saved_nc[arm]) == protocol["measured_blocks"] for arm in ARMS), "missing report quality repetition")
        memory = solve["memory"]
        cases[case] = {
            "solve_seconds": solve["seconds"], "solve_medians_seconds": solve["medians"],
            "solve_new_over_old": solve["new_over_old"], "paired_new_faster": solve["paired_new_faster"],
            "single_case_speed_claim_eligible": solve["medians"]["old"] >= protocol["guard"]["single_case_speed_claim_old_seconds"],
            "iterations": solve["iterations"], "artifact_bytes": solve["artifact_bytes"],
            "quality": {"unit": "chips", "target_strictly_below": protocol["cases"][case]["target_nash_conv"],
                        "negative_roundoff_allowance": protocol["quality_negative_tolerance_chips"],
                        "live_nash_conv": live_nc, "saved_nash_conv": saved_nc,
                        "saved_minus_live": {arm: [saved - live for saved, live in zip(saved_nc[arm], live_nc[arm])] for arm in ARMS}},
            "memory": {"os_rss_bound_new_over_old": solve["os_rss_bound_ratio"],
                       "new_native_os_peak_bytes": [m["root_os_peak_resident_bytes"] for m in memory["new"]],
                       "old_sampled_os_peak_bytes": [m["sampled_peak_tree_resident_bytes"] for m in memory["old"]],
                       "meaning": "max(new native OS peaks) / min(old sampled OS peaks), conditional on verified root-only observations; not a physical, median, population or phase memory ratio. An unmet bound is inconclusive."},
            "io": {kind: {"medians_seconds": value[kind]["medians"], "new_over_old": value[kind]["new_over_old"],
                          "regression_screen": value[kind]["regression_screen"],
                          "interpretation": "descriptive_only_below_10ms" if value[kind]["regression_screen"] is None else "eligible_regression_screen"}
                   for kind in CODECS},
        }
    report["performance"] = {
        "cases": cases, "revisions": protocol["revisions"], "versions": protocol["versions"],
        "measured_repetitions_per_arm": protocol["measured_blocks"], "warmups_excluded": protocol["warmup_blocks"],
        "solve_geomean_new_over_old": summary["solve_geomean_new_over_old"],
        "screens": {"time": summary["time_screen"], "flop_memory": summary["memory_screen"], "io": summary["io_screen"]},
        "guard": protocol["guard"], "phase_timings": None,
        "timing_scope": "Solve is the entire CLI process. Codec decode/write uses operation time; read-root includes open time. Sub-10ms old codec medians are descriptive only.",
    }
    return report


def numbers(values):
    return ", ".join(format(value, ".9g") for value in values)


def verdict(value):
    return "pass" if value is True else "not met" if value is False else "unavailable"


def markdown(report):
    lines = [f"Final pipeline: **{report['status']}**", "", report["scope"], ""]
    if report["stage_counts"] is not None:
        lines += ["| Phase | Passed | Failed | Skipped | Pending |", "|---|---:|---:|---:|---:|"]
        for phase, values in report["stage_counts"].items():
            lines.append(f"| {phase} | " + " | ".join(str(values.get(status, 0)) for status in ("passed", "failed", "skipped", "pending")) + " |")
    performance = report["performance"]
    if performance is None:
        lines += ["", "The proof is not completed. No timing, quality, memory or I/O performance claim is reported."]
        if report["stage_counts"] is None:
            lines += ["Stage counts are unavailable for a prepare failure."]
        if report["failures"]:
            lines += ["Failure reasons are retained in the JSON report."]
        return "\n".join(lines) + "\n"
    lines += ["", f"Measured repetitions: {performance['measured_repetitions_per_arm']} per arm; one warmup excluded. Ratios are new / old.",
              "", "| Case | Old CLI median (s) | New CLI median (s) | Ratio | Old iterations | New iterations |", "|---|---:|---:|---:|---|---|"]
    for case, data in performance["cases"].items():
        med = data["solve_medians_seconds"]
        lines.append(f"| {case} | {med['old']:.9g} | {med['new']:.9g} | {data['solve_new_over_old']:.6f} | {numbers(data['iterations']['old'])} | {numbers(data['iterations']['new'])} |")
    screens = performance["screens"]
    lines += ["", f"CLI ratio geometric mean: {performance['solve_geomean_new_over_old']:.6f}; time screen: {verdict(screens['time'])}.",
              "", "| Case / arm | Live NashConv (chips, repeats) | Saved NashConv (chips, repeats) | SOL bytes | Checkpoint bytes |",
              "|---|---|---|---|---|"]
    for case, data in performance["cases"].items():
        for arm in ARMS:
            q, artifacts = data["quality"], data["artifact_bytes"][arm]
            lines.append(f"| {case} / {arm} | {numbers(q['live_nash_conv'][arm])} | {numbers(q['saved_nash_conv'][arm])} | {numbers(artifacts['solution.sol'])} | {numbers(artifacts['checkpoint.ckpt'])} |")
    lines += ["", "Both quality checks require NashConv strictly below 0.04 chips. The fixed negative roundoff allowance is 1e-6 chips; raw values are not clamped.",
              "", "| Case | Conservative OS RSS bound (new / old) |", "|---|---:|"]
    for case, data in performance["cases"].items():
        ratio = data["memory"]["os_rss_bound_new_over_old"]
        lines.append(f"| {case} | {'unavailable' if ratio is None else format(ratio, '.6f')} |")
    lines += ["", "Memory uses max(new native OS peaks) / min(old sampled OS peaks). It is not a physical-memory median, population estimate or phase peak. An unmet bound is inconclusive about improvement.",
              f"Flop memory screen: {verdict(screens['flop_memory'])}.", "", "| Case / I/O operation | Old median (ms) | New median (ms) | Ratio | Interpretation |", "|---|---:|---:|---:|---|"]
    for case, data in performance["cases"].items():
        for kind, io in data["io"].items():
            description = "descriptive only (old < 10 ms)" if io["regression_screen"] is None else verdict(io["regression_screen"])
            lines.append(f"| {case} / {kind} | {io['medians_seconds']['old'] * 1000:.6f} | {io['medians_seconds']['new'] * 1000:.6f} | {io['new_over_old']:.6f} | {description} |")
    lines += ["", f"Eligible I/O screen: {verdict(screens['io'])}. Its maximum ratio is 1.10; sub-10ms rows do not pass or fail it.",
              "Codec decode/write times cover the operation; read-root also includes open time. Whole CLI solves include initialization, CFR, quality checks, checkpoint/SOL writes and shutdown. Internal phase timings are unavailable.",
              "Single-case speed claims require an old CLI median of at least 1 second. These are scoped descriptive screens, not overall R1 certification."]
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--json-target", type=Path)
    parser.add_argument("--markdown-target", type=Path)
    args = parser.parse_args()
    targets = [path for path in (args.json_target, args.markdown_target) if path is not None]
    if len({path.resolve() for path in targets}) != len(targets) or any(path.exists() for path in targets):
        parser.error("report targets must be distinct new files; existing proof must not be overwritten")
    report = build_report(args.out)
    encoded = json.dumps(report, indent=2, allow_nan=False) + "\n"
    if args.json_target:
        args.json_target.write_text(encoded, encoding="utf-8", newline="\n")
    if args.markdown_target:
        args.markdown_target.write_text(markdown(report), encoding="utf-8", newline="\n")
    if not args.json_target:
        print(encoded, end="")


if __name__ == "__main__":
    main()
