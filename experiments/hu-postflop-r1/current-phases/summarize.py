"""Japanese JSON/Markdown readout of a trusted, terminal current-phases proof."""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import statistics
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
CASES = ("river", "turn", "flop")
OPERATIONS = ("solve", "decode-all", "read-root")
SOLVE_LEAVES = (
    ("input_preparation", "入力準備"), ("initialization", "初期化"),
    ("cfr_updates", "CFR更新"), ("periodic_ev_br", "定期EV/BR"),
    ("final_ev_br", "最終EV/BR"), ("checkpoint", "checkpoint保存"),
    ("sol_preparation", "SOL生成準備"), ("sol_serialization_and_write", "SOL直列化・書込み"),
    ("summary_publish", "summary出力"), ("overhead", "内部のその他区間"),
)
CODEC_LEAVES = {"decode-all": (("codec_decode_all", "全decode区間"), ("overhead", "内部のその他区間")),
                "read-root": (("codec_read_root", "root読込み区間"), ("overhead", "内部のその他区間"))}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def pin(path):
    path = Path(path).resolve(strict=True)
    raw = path.read_bytes()
    return {"path": str(path), "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def three(values):
    require(len(values) == 3 and all(type(x) in (int, float) and math.isfinite(x) for x in values),
            "three finite observations required")
    return {"samples": values, "median": statistics.median(values), "max": max(values)}


def selected(state, case, operation, arm):
    rows = sorted((row for row in state["stages"] if row["stage"].get("case") == case
                   and row["stage"]["kind"] == operation and row["stage"].get("arm") == arm
                   and row["stage"].get("warmup") is False), key=lambda row: row["stage"]["block"])
    require([row["stage"]["block"] for row in rows] == [1, 2, 3]
            and all(row["status"] == "passed" for row in rows), "non-warmup sample set differs")
    return rows


def retained_reference(index, proof, original):
    stored = index["files"][original]
    require(stored["path"] == original, "retention identity differs")
    return {**stored, "retained_path": str(proof / "payload" / stored["sha256"])}


def aggregate(state, verification, index, proof, original_output):
    """Called only after trusted check; no performance readout for failed evidence."""
    require(verification["payload_integrity"] == "verified", "unverified evidence")
    if verification["status"] != "completed":
        return None
    require(state["status"] == "completed" and state["summary"] == verification["summary"],
            "verified result/summary differs")
    answer = {}
    for case in CASES:
        answer[case] = {}
        for operation in OPERATIONS:
            rows = {arm: selected(state, case, operation, arm) for arm in ("plain", "off", "time", "memory")}
            calibrated = verification["summary"][case][operation]
            time_rows, memory_rows = rows["time"], rows["memory"]
            phases = SOLVE_LEAVES if operation == "solve" else CODEC_LEAVES[operation]
            leaves = []
            for name, label in phases:
                timed = [row["sample"]["phase"]["leaves"][name] for row in time_rows]
                memory = [row["sample"]["phase"]["leaves"][name] for row in memory_rows]
                leaves.append({"phase": name, "label_jp": label,
                    "time_arm_seconds": three([row["ns"] / 1e9 for row in timed]),
                    "time_arm_calls": [row["calls"] for row in timed],
                    "memory_arm_peak_kib": three([row["memory_peak_kib"] for row in memory]),
                    "memory_arm_calls": [row["calls"] for row in memory]})
            refs = {}
            for arm, arm_rows in rows.items():
                refs[arm] = []
                for row in arm_rows:
                    directory = original_output + "/stages/" + row["stage"]["label"]
                    refs[arm].append({"block": row["stage"]["block"], "stage": row["stage"]["label"],
                        "supervisor": retained_reference(index, proof, row["record"]["path"]),
                        "native": retained_reference(index, proof, directory + "/native.json"),
                        "phase": retained_reference(index, proof, directory + "/phase.json") if arm in ("time", "memory") else None})
            native = {arm: {"external_seconds": three([row["sample"]["native"]["elapsed_seconds"] for row in arm_rows]),
                            "unreset_whole_process_peak_kib": three([row["sample"]["native"]["ru_maxrss_kib"] for row in arm_rows])}
                      for arm, arm_rows in rows.items() if arm != "memory"}
            value = {"timing_calibration": calibrated["timing"],
                "timing_attribution_status": "eligible" if calibrated["timing"]["gate_passed"] else "not_evaluated",
                "memory_observer_screen": calibrated["phase_memory_counter_screen"],
                "memory_attribution_status": "descriptive_counter_only" if calibrated["phase_memory_counter_screen"]["eligible_descriptive_only"] else "not_evaluated",
                "native_unreset_arms": native, "leaves": leaves,
                "instrumented_time_scope_seconds": three([row["sample"]["phase"]["total_ns"] / 1e9 for row in time_rows]),
                "unassigned_envelope_difference_seconds": three([row["sample"]["native"]["elapsed_seconds"] - row["sample"]["phase"]["total_ns"] / 1e9 for row in time_rows]),
                "envelope_difference_scope": "Signed external-minus-internal envelope arithmetic only; not an inferred startup, exit, observer or solver phase; no correction/subtraction from measured leaves.",
                "raw_references": refs}
            if operation == "solve":
                generation = [row["sample"]["phase"]["output_generation_peak_kib"] for row in memory_rows]
                require(generation == [max(row["sample"]["phase"]["leaves"][name]["memory_peak_kib"]
                                          for name in ("sol_preparation", "sol_serialization_and_write")) for row in memory_rows],
                        "generation counter differs")
                value["output_generation_peak_kib"] = three(generation)
                value["output_generation_definition"] = "Per run max(SOL preparation peak, SOL serialization/write peak); resident solver/payload included; neither sum nor entry-RSS subtraction. Checkpoint is separate."
            else:
                value["codec_native_clock_seconds"] = {arm: {name: three([row["sample"]["timing"][name] for row in arm_rows])
                    for name in ("open_seconds", "operation_seconds", "validation_output_seconds")}
                    for arm, arm_rows in rows.items() if arm != "memory"}
            answer[case][operation] = value
    return answer


def number(value):
    return f"{value:.3f}"


def markdown(report):
    status = report["campaign_status"]
    lines = ["# 現行sourceの工程計測", ""]
    if status != "completed":
        lines += ["原証拠の保持検証は成功しましたが、campaignは失敗しています。性能・工程比率・メモリの受入値は出しません。", "",
                  "段階件数: " + json.dumps(report["verification"]["counts"], ensure_ascii=False) + "。", "",
                  "失敗記録: " + str(report["verification"].get("error")), "",
                  "失敗段階: " + "、".join(row["label"] for row in report["failure"]["stages"]) + "。", ""]
    else:
        lines += ["固定した3つのsynthetic HU case・F32・1workerの計測と全品質検査が完了しました。"
                  "各欄はwarmupを除く3回です。現行source内の工程診断であり、旧版との改善率や外部参照・R1全体の認定ではありません。", "",
                  "| case / operation | plain外部中央値 ms | off/plain | time/off | 時間帰属 | メモリobserver | plain全体peak最大 MiB |",
                  "|---|---:|---:|---:|---|---|---:|"]
        for case, operations in report["cases"].items():
            for operation, value in operations.items():
                cal = value["timing_calibration"]
                lines.append(f"| {case} / {operation} | {number(cal['medians']['plain']*1000)} | {number(cal['off_over_plain'])} | {number(cal['time_over_off'])} | {value['timing_attribution_status']} | {'記述可' if value['memory_observer_screen']['eligible_descriptive_only'] else 'not_evaluated'} | {number(value['native_unreset_arms']['plain']['unreset_whole_process_peak_kib']['max']/1024)} |")
        lines += ["", "時間は両比率が[0.95,1.05]かつplain/off中央値が10ms以上のときだけ帰属可能です。"
                  "`not_evaluated`の工程時間も原観測として残しますが、性能帰属には使いません。", "",
                  "## solveの固定leaf", "",
                  "各セルは **time armの中央値ms / memory armの最大counter MiB**。両者は別実行です。"
                  "校正不適合のcaseは上表の判定に従い、数値を合格扱いしません。", "",
                  "| 工程 | River | Turn | Flop |", "|---|---:|---:|---:|"]
        for i, (_, label) in enumerate(SOLVE_LEAVES):
            values = [report["cases"][case]["solve"]["leaves"][i] for case in CASES]
            cells = [number(v["time_arm_seconds"]["median"]*1000) + " / " + number(v["memory_arm_peak_kib"]["max"]/1024) for v in values]
            lines.append("| " + label + " | " + " | ".join(cells) + " |")
        lines += ["", "SOL生成時counter最大（準備と書込みのmax、加算しない）: " + "、".join(
            case + " " + number(report["cases"][case]["solve"]["output_generation_peak_kib"]["max"]/1024) + " MiB" for case in CASES) + "。checkpointは別工程です。", "",
            "## 読込みの固定leaf", "", "| case / operation | time中央値 ms | memory最大counter MiB |", "|---|---:|---:|"]
        for case in CASES:
            for operation in ("decode-all", "read-root"):
                value = report["cases"][case][operation]["leaves"][0]
                lines.append(f"| {case} / {operation} | {number(value['time_arm_seconds']['median']*1000)} | {number(value['memory_arm_peak_kib']['max']/1024)} |")
        lines += ["", "メモリはclear_refs=5で境界ごとにresetしたLinux VmHWMの絶対counterです。"
                  "常駐入力・solver・observerを含み、物理RSSの厳密peakや追加allocation量ではありません。"
                  "observer screen不適合時は工程メモリ帰属もnot_evaluatedです。plain全体peakはresetしない別processのwait4値です。", "",
                  "内部leafのその他区間、native codec時計、外部と内部の未分離差はJSONで別々に保持します。"
                  "外部−内部差はfork/exec・起動・終了・最終JSON保存・wait等が混在し、純粋なstartup/observer時間に読み替えず、工程値の補正・差引きに使いません。"
                  "read-root leafはopen/初期化も含み、native codecのoperation_seconds単独とは範囲が異なります。"
                  "memoryの時刻境界とHWM reset窓も同一ではありません。", ""]
    evidence = ("全raw値・3本の中央値/最大値・original pathとCAS pathは[report.json](report.json)に保持しています。"
                if status == "completed" else
                "段階状態・失敗情報と原証拠への参照は[report.json](report.json)に保持しています。未完の性能集計は行っていません。")
    lines += ["## 根拠", "", evidence, ""]
    for name in ("result.json", "plan.json", "retention.json"):
        value = report["input_pins"].get(name)
        if value:
            lines.append(f"- [{name}](<{value['path']}>) — {value['bytes']} bytes、SHA-256 `{value['sha256']}`")
    lines += ["", "検証結果: [verification.json](verification.json)。集計器/信頼するcheckerのhashもJSONに記録しています。", ""]
    return "\n".join(lines)


def run(proof, out):
    proof = Path(proof).resolve(strict=True)
    out = Path(out).resolve()
    require(not out.exists() and not out.is_relative_to(proof) and not proof.is_relative_to(out),
            "new report directory outside proof required")
    input_paths = {name: proof/name for name in ("result.json", "retention.json")}
    if (proof/"plan.json").exists():
        input_paths["plan.json"] = proof/"plan.json"
    inputs = {name: pin(path) for name, path in input_paths.items()}
    code = {name: pin(HERE/name) for name in ("summarize.py", "check_run.py", "runner.py", "validate.py")}
    spec = importlib.util.spec_from_file_location("trusted_current_phases_summary_check", HERE/"check_run.py")
    checker = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(checker)
    verification = checker.check(proof)
    require(verification["payload_integrity"] == "verified" and verification["status"] in ("completed", "failed"),
            "terminal trusted verification required")
    state = checker.core.read(proof/"result.json")
    index = checker.core.read(proof/"retention.json")
    original = checker.core.read(proof/"plan.json")["output"] if "plan.json" in inputs else None
    cases = aggregate(state, verification, index, proof, original)
    require(inputs == {name: pin(path) for name, path in input_paths.items()}, "proof metadata changed during verification")
    require(code == {name: pin(HERE/name) for name in code}, "trusted summary code changed during verification")
    report = {"schema": "r1.current-phases-report/v1", "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "campaign_status": verification["status"], "source_revision": "11e4062ba1735e58b60d12999cb23ed10fd1a163",
        "scope": "Three fixed synthetic cases, one worker, F32; Flop narrow three-combo fixture; current-source phase attribution only.",
        "sample_policy": "Non-warmup blocks1,2,3 only, no weighting or replacement; quality verification still includes all warmups.",
        "units": {"time": "seconds", "memory": "Linux KiB counters; Markdown MiB=KiB/1024"},
        "input_pins": inputs, "trusted_code_pins": code, "verification": verification, "cases": cases,
        "failure": {"stages": [{"label": row["stage"]["label"], "error": row.get("error"), "record": row.get("record")}
                               for row in state["stages"] if row["status"] == "failed"]} if verification["status"] == "failed" else None,
        "non_claims": ["old/new gain", "physical phase memory or allocation increment", "32-worker scaling", "external library or all-R1 certification"]}
    rendered = markdown(report)
    out.mkdir(parents=True, exist_ok=False)
    for name, value in (("verification.json", verification), ("report.json", report)):
        with (out/name).open("x", encoding="utf-8", newline="\n") as stream:
            json.dump(value, stream, ensure_ascii=False, indent=2, allow_nan=False)
            stream.write("\n")
    with (out/"report.jp.md").open("x", encoding="utf-8", newline="\n") as stream:
        stream.write(rendered)
    return {"status": verification["status"], "output": str(out), "files": [pin(out/name) for name in ("verification.json", "report.json", "report.jp.md")]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proof", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True, help="new report directory outside extracted proof")
    args = parser.parse_args()
    print(json.dumps(run(args.proof, args.out), indent=2, ensure_ascii=False, allow_nan=False))


if __name__ == "__main__":
    main()
