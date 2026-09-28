"""Summarize a completed VM20 trusted-reader report; never read raw proof data.

Usage: python -B summarize-profile.py --report ANALYSIS.json
       --receipt ANALYSIS.receipt.json --json NEW.json --markdown NEW.md
Use --self-test for tiny synthetic validation tests, without creating reports.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import re
import sys

READER = {"bytes": 24584, "sha256": "5f7a03b4fc5bff751aa9843456a531d1c46ac0303e15ae4c55820562770d2c61"}
REMOTE_READER = "/opt/r1/flop-cpu-profile-package/experiments/hu-postflop-r1/flop-scaling/cpu-profile/analyze.py"
REMOTE_PREFIX = "/opt/r1/flop-cpu-profile-analysis01"
COUNTS = {"native_build": 1, "core_test_commands": 1, "perf_preflights": 2, "canonical": 2, "profiles": 8}
SCHEDULE = [(c, w, r) for c in ("narrow", "expanded") for r in (0, 1)
            for w in ((16, 32) if r == 0 else (32, 16))]
NOTES = [
    "全10 solve（canonical 2 + profile 8）の完全検証済みreaderレポートだけを要約する。生のproofは再検証しない。",
    "全行は同じportable frame-pointer binaryの診断値。他buildとの速度比較・性能合格・本体採用は判定しない。",
    "CPU秒はprocess内の全thread合計。CPU/壁時計は平均CPU使用数の目安で、spinや有用な仕事を区別しない。",
    "leafのperiod比はCFR内exclusive leaf period合計を分母とする。経過時間比・off-CPU時間・帯域量ではない。",
    "unknownを除外しない。上限未到達のcallchainも完全とは限らない。unknownと上限到達は重複しうる。",
    "trusted reportにはTIDの集合やTID別件数がないためtid_countだけを表示する。カテゴリ推測は行わない。",
    "サンプル不足・SMT・移動・spin・allocator・cache・帯域の単独原因は、この集計だけから確定しない。",
]


def need(value, message):
    if not value:
        raise ValueError(message)


def pin(raw):
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def pairs(items):
    result = {}
    for key, value in items:
        need(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def finite_tree(value):
    if isinstance(value, float):
        need(math.isfinite(value), "non-finite value")
    elif isinstance(value, dict):
        for child in value.values():
            finite_tree(child)
    elif isinstance(value, list):
        for child in value:
            finite_tree(child)


def loads(raw):
    result = json.loads(raw, object_pairs_hook=pairs,
                        parse_constant=lambda _: need(False, "non-finite JSON token"))
    finite_tree(result)
    return result


def small_file(path, limit=16 * 1024**2):
    path = Path(path)
    need(not path.is_symlink() and path.is_file() and path.stat().st_size <= limit,
         "bounded regular input required: " + str(path))
    with path.open("rb") as stream:
        raw = stream.read(limit + 1)
    need(len(raw) <= limit, "input exceeded cap")
    return raw


def natural(value, positive=False):
    need(type(value) is int and int(positive) <= value < 2**63, "invalid count/period")
    return value


def seconds(value, positive=False):
    need(type(value) in (int, float) and math.isfinite(value)
         and (value > 0 if positive else value >= 0), "invalid seconds")
    return value


def identity(value):
    need(isinstance(value, dict) and set(value) >= {"bytes", "sha256"}, "identity missing")
    natural(value["bytes"], True)
    need(isinstance(value["sha256"], str) and re.fullmatch(r"[a-f0-9]{64}", value["sha256"]), "invalid SHA")


def utc(value):
    parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    need(parsed.utcoffset() == dt.timedelta(0), "UTC timestamp required")
    return parsed


def validate_receipt(receipt, payloads):
    need(receipt["exit_code"] == 0 and type(receipt["exit_code"]) is int
         and "error" not in receipt and receipt["timeout_seconds"] == 120, "reader did not pass")
    need(receipt["reader"] == READER, "receipt does not bind frozen reader")
    argv = receipt["argv"]
    need(len(argv) == 7 and PurePosixPath(argv[0]).is_absolute()
         and PurePosixPath(argv[0]).name in {"python3", "python3.11", "python3.12", "python3.13"}
         and argv[1:] == ["-B", REMOTE_READER, "--out", "/opt/r1/flop-cpu-profile-proof01",
                         "--report", REMOTE_PREFIX + ".json"], "reader command differs")
    need(utc(receipt["started_at"]) <= utc(receipt["ended_at"]), "receipt time order differs")
    need(receipt["unit"]["ActiveState"] in {"inactive", "failed"}
         and receipt["unit"]["MainPID"] == "0", "reader unit was not quiescent")
    need(set(receipt["files"]) == set(payloads) == {"report", "stdout", "stderr"}, "reader output set differs")
    for kind, raw in payloads.items():
        suffix = ".json" if kind == "report" else "." + kind + ".log"
        need(receipt["files"][kind] == {"path": REMOTE_PREFIX + suffix, **pin(raw)}, "captured output pin differs")
    need(payloads["stderr"] == b"", "reader stderr not empty")
    report = loads(payloads["report"])
    need(loads(payloads["stdout"]) == report, "reader stdout/report differ")
    return report


def summarize(report, top):
    finite_tree(report)
    need(report["schema"] == "r1.cpu-profile-report/v1" and report["status"] == "completed"
         and report["payload_integrity"] == "verified" and report["diagnostic"] == "samples_validated"
         and report["performance_screen"] == "not_applicable" and report["production_adoption"] is False,
         "only a completed validated diagnostic report is accepted")
    need(report["counts"] == COUNTS and len(report["observations"]) == 8, "complete 10-solve/8-profile result required")
    for key in ("binary", "build_plan", "measurement_plan", "execution"):
        identity(report[key])
    rows = []
    for value, (case, workers, round_) in zip(report["observations"], SCHEDULE):
        need((value["case"], value["workers"], value["round"], value["stage"])
             == (case, workers, round_, f"{case}-r{round_}-w{workers}"), "profile order/identity differs")
        cpu, result, sample, census = (value[k] for k in ("process_cpu", "result", "sampling", "census"))
        need(cpu["schema"] == "r1.flop-cpu-occupancy/v1" and cpu["case"] == case
             and cpu["threads"] == workers and cpu["iterations"] == 64 and cpu["performance_claim"] is False
             and cpu["clock"] == "CLOCK_PROCESS_CPUTIME_ID" and cpu["clock_id"] == 2
             and cpu["scope"] == "all threads in this process", "CPU identity differs")
        wall, cpu_seconds = seconds(cpu["cfr_wall_seconds"], True), seconds(cpu["cfr_cpu_seconds"])
        need(wall == result["cfr_seconds"], "CPU/result wall differs")
        total, count, period = (natural(sample[k], True) for k in ("all_samples", "cfr_samples", "cfr_period_sum"))
        need(count <= total and census["record_counts"]["PERF_RECORD_SAMPLE"] == total
             and census["loss_or_throttle_records_present"] is False
             and census["counts_are_record_occurrences_not_lost_sample_cardinality"] is True,
             "sample census differs")
        for key in ("PERF_RECORD_LOST", "PERF_RECORD_LOST_SAMPLES", "PERF_RECORD_THROTTLE", "PERF_RECORD_UNTHROTTLE"):
            need(census["record_counts"].get(key, 0) == 0, "loss/throttle record present")
        need(sample["lost_samples"] is None and sample["throttle_events"] is None, "missing cardinality was replaced")
        tids = natural(sample["tid_count"], True)
        need(tids <= count, "TID count exceeds samples")
        unknown, any_unknown, at_limit = (natural(sample[k]) for k in
                                        ("unknown_leaf_samples", "unknown_any_frame_samples", "at_callchain_limit_samples"))
        need(unknown <= any_unknown <= count and at_limit <= count, "unknown/limit count exceeds samples")
        counts, periods = sample["leaf_sample_counts"], sample["leaf_period_sums"]
        need(isinstance(counts, dict) and set(counts) == set(periods) and counts, "leaf maps differ")
        leaves = []
        for leaf in counts:
            n, p = natural(counts[leaf], True), natural(periods[leaf], True)
            need(p >= n, "leaf period smaller than positive-period sample count")
            leaves.append({"leaf": leaf, "samples": n, "period_sum": p,
                           "sample_share": n / count, "period_share": p / period})
        need(sum(x["samples"] for x in leaves) == count and sum(x["period_sum"] for x in leaves) == period,
             "exclusive leaf totals differ")
        leaves.sort(key=lambda x: (-x["period_sum"], -x["samples"], x["leaf"]))
        tail = leaves[top:]
        rows.append({"stage": value["stage"], "case": case, "workers": workers, "round": round_,
                     "cfr_wall_seconds": wall, "cfr_cpu_seconds": cpu_seconds, "cfr_cpu_over_wall": cpu_seconds / wall,
                     "tid_count": tids, "all_samples": total, "cfr_samples": count, "cfr_period_sum": period,
                     "unknown_leaf_samples": unknown, "unknown_any_frame_samples": any_unknown,
                     "at_callchain_limit_samples": at_limit, "unknown_leaf_sample_share": unknown / count,
                     "unknown_any_frame_sample_share": any_unknown / count, "at_callchain_limit_sample_share": at_limit / count,
                     "lost_sample_cardinality": None, "loss_or_throttle_records_present": False,
                     "exclusive_leaves": leaves, "markdown_top_count": min(top, len(leaves)),
                     "markdown_remainder": {"leaf_count": len(tail), "samples": sum(x["samples"] for x in tail),
                                            "period_sum": sum(x["period_sum"] for x in tail),
                                            "period_share": sum(x["period_sum"] for x in tail) / period}})
    output = {"schema": "r1.vm20-profile-summary/v1", "status": "completed_report_summarized",
              "counts": report["counts"], "solves_verified_by_trusted_reader": 10,
              "performance_screen": "not_applicable", "production_adoption": False, "grouping": "none",
              "source_identities": {k: report[k] for k in ("binary", "build_plan", "measurement_plan", "execution")},
              "build_host": report["build_host"], "measurement_host": report["measurement_host"],
              "observations": rows, "limitations": NOTES, "trusted_reader_limits": report["limits"]}
    finite_tree(output)
    return output


def escape(text):
    return str(text).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("|", "&#124;").replace("`", "&#96;").replace("\n", " ").replace("\r", " ")


def markdown(summary):
    lines = ["# VM20 CPU profile 診断要約", "", "検証済み全10 solve・8 profileの集計。性能合格・本体採用は判定しない。", "",
             "| case | workers | round | CFR wall秒 | CFR CPU秒 | CPU/wall | TID件数 | CFR/all samples | CFR period合計 | unknown leaf/any | 上限到達 |",
             "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"]
    for row in summary["observations"]:
        lines.append(f'| {row["case"]} | {row["workers"]} | {row["round"]} | {row["cfr_wall_seconds"]:.6f} | {row["cfr_cpu_seconds"]:.6f} | {row["cfr_cpu_over_wall"]:.3f} | {row["tid_count"]} | {row["cfr_samples"]}/{row["all_samples"]} | {row["cfr_period_sum"]} | {row["unknown_leaf_samples"]}/{row["unknown_any_frame_samples"]} | {row["at_callchain_limit_samples"]} |')
    for row in summary["observations"]:
        lines += ["", "## " + row["stage"], "", "exclusive leafをperiod降順で表示。全leafはJSONに保持し、カテゴリ化しない。", "",
                  "| leaf (DSO) | samples | period合計 | CFR period比 |", "| --- | ---: | ---: | ---: |"]
        for leaf in row["exclusive_leaves"][:row["markdown_top_count"]]:
            lines.append(f'| {escape(leaf["leaf"])} | {leaf["samples"]} | {leaf["period_sum"]} | {leaf["period_share"]:.6%} |')
        tail = row["markdown_remainder"]
        lines.append(f'| 残り {tail["leaf_count"]} leaf | {tail["samples"]} | {tail["period_sum"]} | {tail["period_share"]:.6%} |')
    lines += ["", "## 入力pin", "", "| 入力 | bytes | SHA-256 |", "| --- | ---: | --- |"]
    for key, value in summary["inputs"].items():
        lines.append(f'| {escape(key)}: {escape(value["path"])} | {value["bytes"]} | {value["sha256"]} |')
    lines += ["", "## 解釈の限界", "", *["- " + text for text in NOTES], ""]
    return "\n".join(lines)


def self_test():
    import copy
    import unittest

    def fixture():
        report = {"schema": "r1.cpu-profile-report/v1", "status": "completed", "payload_integrity": "verified",
                  "diagnostic": "samples_validated", "performance_screen": "not_applicable", "production_adoption": False,
                  "counts": dict(COUNTS), "build_host": {}, "measurement_host": {}, "limits": ["synthetic test only"], "observations": []}
        for key in ("binary", "build_plan", "measurement_plan", "execution"):
            report[key] = {"bytes": 1, "sha256": "0" * 64}
        for case, workers, round_ in SCHEDULE:
            report["observations"].append({"stage": f"{case}-r{round_}-w{workers}", "case": case, "workers": workers, "round": round_,
                "process_cpu": {"schema": "r1.flop-cpu-occupancy/v1", "case": case, "threads": workers, "iterations": 64,
                    "performance_claim": False, "clock": "CLOCK_PROCESS_CPUTIME_ID", "clock_id": 2,
                    "scope": "all threads in this process", "cfr_wall_seconds": 2.0, "cfr_cpu_seconds": 0.0},
                "result": {"cfr_seconds": 2.0}, "sampling": {"all_samples": 4, "cfr_samples": 3, "cfr_period_sum": 9,
                    "unknown_leaf_samples": 1, "unknown_any_frame_samples": 2, "at_callchain_limit_samples": 1, "tid_count": 2,
                    "lost_samples": None, "throttle_events": None,
                    "leaf_sample_counts": {"known (/test)": 2, "[unknown] ([unknown])": 1},
                    "leaf_period_sums": {"known (/test)": 3, "[unknown] ([unknown])": 6}},
                "census": {"record_counts": {"PERF_RECORD_SAMPLE": 4}, "loss_or_throttle_records_present": False,
                    "counts_are_record_occurrences_not_lost_sample_cardinality": True}})
        return report

    class Tests(unittest.TestCase):
        def test_period_ranking_and_unknown_preserved(self):
            row = summarize(fixture(), 1)["observations"][0]
            self.assertEqual(row["exclusive_leaves"][0]["leaf"], "[unknown] ([unknown])")
            self.assertEqual(row["exclusive_leaves"][0]["period_share"], 2 / 3)
            self.assertEqual(row["markdown_remainder"]["period_share"], 1 / 3)
            self.assertEqual(row["cfr_cpu_over_wall"], 0)

        def test_partial_duplicate_order_and_bad_totals_rejected(self):
            base = fixture()
            mutations = [lambda x: x.update(status="failed"), lambda x: x["counts"].update(profiles=7),
                         lambda x: x["observations"].pop(), lambda x: x["observations"].__setitem__(1, x["observations"][0]),
                         lambda x: x["observations"][0]["sampling"].update(cfr_period_sum=10),
                         lambda x: x["observations"][0]["sampling"].update(unknown_leaf_samples=4),
                         lambda x: x["observations"][0]["census"]["record_counts"].update(PERF_RECORD_LOST=1),
                         lambda x: x["observations"][0]["process_cpu"].update(cfr_cpu_seconds=float("nan"))]
            for mutate in mutations:
                value = copy.deepcopy(base)
                mutate(value)
                with self.assertRaises(ValueError):
                    summarize(value, 1)

        def test_json_duplicate_and_nonfinite_rejected(self):
            for raw in ('{"x":1,"x":2}', '{"x":NaN}', '{"x":1e999}'):
                with self.assertRaises(ValueError):
                    loads(raw)

        def test_receipt_binding(self):
            raw = json.dumps(fixture()).encode()
            payloads = {"report": raw, "stdout": raw + b"\n", "stderr": b""}
            receipt = {"exit_code": 0, "timeout_seconds": 120, "reader": READER,
                "argv": ["/usr/bin/python3", "-B", REMOTE_READER, "--out", "/opt/r1/flop-cpu-profile-proof01", "--report", REMOTE_PREFIX + ".json"],
                "started_at": "2026-09-27T00:00:00Z", "ended_at": "2026-09-27T00:00:01Z",
                "unit": {"ActiveState": "inactive", "MainPID": "0"},
                "files": {k: {"path": REMOTE_PREFIX + (".json" if k == "report" else "." + k + ".log"), **pin(v)} for k, v in payloads.items()}}
            self.assertEqual(validate_receipt(receipt, payloads), fixture())
            payloads["report"] += b" "
            with self.assertRaises(ValueError):
                validate_receipt(receipt, payloads)

    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests))
    raise SystemExit(0 if result.wasSuccessful() else 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--json", type=Path)
    parser.add_argument("--markdown", type=Path)
    parser.add_argument("--top", type=int, default=10)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        need(not any((args.report, args.receipt, args.json, args.markdown)), "self-test cannot emit measurement reports")
        self_test()
    need(all((args.report, args.receipt, args.json, args.markdown)) and 1 <= args.top <= 100, "four paths and top 1..100 required")
    need(args.json.resolve() != args.markdown.resolve() and not args.json.exists() and not args.markdown.exists(), "new distinct output paths required")
    receipt_raw = small_file(args.receipt, 1024**2)
    receipt = loads(receipt_raw)
    payloads, inputs = {}, {"receipt": {"path": str(args.receipt.resolve()), **pin(receipt_raw)}}
    for kind in ("report", "stdout", "stderr"):
        local = args.report if kind == "report" else args.report.with_suffix("." + kind + ".log")
        payloads[kind] = small_file(local, 1024**2 if kind == "stderr" else 16 * 1024**2)
        inputs[kind] = {"path": str(local.resolve()), **pin(payloads[kind])}
    report = validate_receipt(receipt, payloads)
    result = summarize(report, args.top)
    inputs["summarizer"] = {"path": str(Path(__file__).resolve()), **pin(small_file(__file__, 1024**2))}
    result.update(inputs=inputs, trusted_reader=READER)
    encoded = (json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode("utf-8")
    md = markdown(result).encode("utf-8")
    for path, raw in ((args.json, encoded), (args.markdown, md)):
        with path.open("xb") as stream:
            stream.write(raw)
    print(json.dumps({"status": result["status"], "json": pin(encoded), "markdown": pin(md)}))


if __name__ == "__main__":
    main()
