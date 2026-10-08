"""Validate retained measurements and generate the compact decision record."""
import hashlib
import json
from pathlib import Path
import struct

EXP = Path(__file__).resolve().parents[1]

def main():
    manifest = json.loads((EXP / "manifest.json").read_text(encoding="utf-8"))
    benchmarks = []
    for label in ["accepted", "other-storage"]:
        result = json.loads((EXP / "raw" / label / "result.json").read_text(encoding="utf-8"))
        for version in ["old", "new"]:
            assert result["binarySha256"][version] == manifest["binaries"][version + "Bench"]["sha256"]
        for row in result["summary"]:
            records = [r for r in result["records"] if r["case"] == row["case"] and r["storage"] == row["storage"]]
            for version in ["old", "new"]:
                assert len([r for r in records if r["version"] == version]) >= 3
            assert len({struct.pack("!d", r["report"]["nashConv"]) for r in records}) == 1
            benchmarks.append(row)
    for case in ["c_turn2", "c_flop1"]:
        assert next(r for r in benchmarks if r["case"] == case and r["storage"] == "f32")["ratioNewOld"] < 1.0
    solves = []
    for label in ["full-solve", "full-c_river-i16", "full-c_river-i16-f32avg"]:
        result = json.loads((EXP / "raw" / label / "result.json").read_text(encoding="utf-8"))
        assert result["identicalMetrics"] and result["identicalExports"] and result["identicalFullPayloadExceptWallSeconds"]
        old, new = result["records"]
        for record in result["records"]:
            assert record["binarySha256"] == manifest["binaries"][record["version"] + "Cli"]["sha256"]
            for data in record["exports"].values():
                assert hashlib.sha256(Path(data["retainedPath"]).read_bytes()).hexdigest() == data["sha256"]
        metrics = {k: old["metrics"][k] for k in ["evP0", "evP1", "explP0", "explP1", "nashConv"]}
        assert all(struct.pack("!d", v) == struct.pack("!d", new["metrics"][k]) for k, v in metrics.items())
        assert old["solution"]["normalizedPayloadSha256"] == new["solution"]["normalizedPayloadSha256"]
        solves.append(dict(label=label, iterations=old["metrics"]["iterations"], metrics=metrics,
                           metricBits={k: struct.pack("!d", v).hex() for k, v in metrics.items()},
                           normalizedPayloadSha256=old["solution"]["normalizedPayloadSha256"],
                           cliExportSha256={k: v["sha256"] for k, v in old["exports"].items()}))
    validation = json.loads((EXP / "validation.json").read_text(encoding="utf-8"))
    assert all(v["exitCode"] == 0 for v in validation["checks"])
    compact = dict(decision="adopt", benchmarks=benchmarks, solves=solves, validation=validation)
    (EXP / "result.json").write_text(json.dumps(compact, indent=2), encoding="utf-8")
    table = ["f32、8 threads、15標本の中央値（秒/評価）:", "",
             "| case | OLD | NEW | NEW/OLD |", "|---|---:|---:|---:|"]
    for r in benchmarks:
        if r["storage"] == "f32":
            table.append(f"| {r['case']} | {r['medianEvalSeconds']['old']:.7f} | {r['medianEvalSeconds']['new']:.7f} | {r['ratioNewOld']:.3f} |")
    table += ["", "追加storageのTurn2 / River（同じ3 process × 3評価）:", "",
              "| case | storage | OLD | NEW | NEW/OLD |", "|---|---|---:|---:|---:|"]
    for r in benchmarks:
        if r["storage"] != "f32":
            table.append(f"| {r['case']} | {r['storage']} | {r['medianEvalSeconds']['old']:.7f} | {r['medianEvalSeconds']['new']:.7f} | {r['ratioNewOld']:.3f} |")
    table += ["", "全runで同じstorage・同じcaseのNashConvはbit一致。生の標本、process内中央値、17桁値・bit列は`raw/*/result.json`とlogにある。",
              "", "Full solveのOLD = NEW（17桁、EV単位BB）:", "",
              "| case / storage | iteration | EV P0 | EV P1 | NashConv |", "|---|---:|---:|---:|---:|"]
    for r in solves:
        m = r["metrics"]
        table.append(f"| {r['label']} | {r['iterations']} | {m['evP0']:.17g} | {m['evP1']:.17g} | {m['nashConv']:.17g} |")
    table += ["", "Full Flopは両者とも200反復で停止。全decoded `.sol`（計時8 bytesだけ除外）のSHA-256は",
              "`" + solves[0]["normalizedPayloadSha256"] + "`で一致。",
              "root strategy / EVのCLI JSON exportもbytes一致。`.sol`自体のcompressed bytesは計時fieldのため異なる。",
              "", "採用: Turn2とFlop1の評価中央値が改善し、3 storageの厳密一致と保存内容の一致を確認できた。",
              "f32のTurn2は13.6%、Flop1は17.6%短縮。f32 Riverは3.0%長いが、絶対差は16.1 µsで、",
              "process内中央値の範囲（OLD 0.420–0.747 ms / NEW 0.363–0.744 ms）より十分小さい。Riverの改善は主張しない。",
              "共有負荷で値は大きく揺れるため、この差を他CPUや専用機へ外挿しない。",
              "速度判断は交互計測の中央値だけに基づく。full solveの単一組の時間比から全体速度の結論は出さない。",
              "", "集計は[result.json](result.json)、再生成は`scripts/summarize.py`。"]
    report = EXP / "README.md"
    text = report.read_text(encoding="utf-8").replace("最終計測後に追記。", "\n".join(table))
    report.write_text(text, encoding="utf-8")
    print(json.dumps(compact, indent=2))

if __name__ == "__main__":
    main()
