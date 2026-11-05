"""Diagnose joint-reach seat bias, checkdown payouts and rake on the benchmark.

usage: bench_bias.py REACH --bands FILE --p1 DIR --l1 ROOT VARIANT
       --flop-ev FILE --class-error FILE --json OUT --markdown OUT

Within each flop, use reach_leaf * matchup_mass on classes present in P1.
Average flops with the mean of both seats' matchup totals; average lines with
reference reach probability. Percent-pot aggregates average each line's ratio
(not the ratio of the aggregate BB value to the aggregate pot).
"""
import argparse
import csv
import json
import math
from pathlib import Path

from bench_metric import flop_key, read_bands

ROLES = ("caller", "raiser")
METRICS = ("delta_caller", "delta_raiser", "delta_sum", "identity_residual",
           "current_caller", "current_raiser", "current_sum")


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def read_diag(path):
    """Read all named diagnosis columns; return canonical flop -> seat -> class."""
    result = {}
    flop = seat = header = None
    for row in Path(path).read_text(encoding="utf-8").splitlines():
        if row.startswith("flop "):
            flop = flop_key(row[5:])
            result[flop] = {}
            seat = header = None
        elif row.startswith("hero "):
            seat = int(row.split()[1])
            result[flop][seat] = {}
            header = None
        elif row.startswith("class,"):
            header = next(csv.reader([row]))
        elif header is not None and "," in row:
            cells = dict(zip(header, next(csv.reader([row]))))
            name = cells.pop("class")
            result[flop][seat][name] = {k: float(v) for k, v in cells.items()}
    return result


def p1_tasks(root):
    """Index tasks by the recorded action line rather than assuming leaf numbers."""
    tasks = {}
    for task in sorted(Path(root).glob("L*-raise")):
        if not task.is_dir():
            continue
        for path in sorted(task.glob("*.json")):
            data = load(path)
            line = data["line"]
            previous = tasks.setdefault(line, (task, {}))
            if previous[0] != task:
                raise ValueError(f"duplicate P1 line in {previous[0]} and {task}: {line}")
            key = flop_key(data["flop"])
            if key in previous[1]:
                raise ValueError(f"duplicate P1 flop in {task}: {key}")
            previous[1][key] = data
    return {line: data for line, (_, data) in tasks.items()}


def rake(pot):
    return math.floor(min(0.05 * pot, 4.0) * 1000) / 1000


def classify(delta_pct, current_pct, residual_pct):
    if residual_pct > 0.2:
        return "L1の払い戻しかrakeの計算がP1と合わない"
    if abs(delta_pct) <= 0.3 and abs(current_pct) > 0.6:
        return "集計の重みによる見かけ"
    if delta_pct < -0.3:
        return "L1の方がrakeを多く払う（potを大きくする戦略）"
    if delta_pct > 0.3:
        return "L1の方が払うrakeが少ない"
    return "偏りなし"


def aggregate(lines):
    probability = sum(row["probability"] for row in lines)
    bb = {key: sum(row["probability"] * row[key] for row in lines) / probability
          for key in METRICS}
    pct = {key: sum(row["probability"] * row[key] / row["pot"] * 100 for row in lines) / probability
           for key in METRICS}
    residual = max(abs(row["identity_residual"]) / row["pot"] * 100 for row in lines)
    return dict(lines=len(lines), probability=probability, bb=bb, pct_pot=pct,
                max_identity_residual_pct_pot=residual,
                max_flop_identity_residual_pct_pot=max(row["max_flop_identity_residual_pct_pot"] for row in lines),
                max_mass_ratio_deviation=max(row["max_mass_ratio_deviation"] for row in lines),
                max_p1_sum_error_bb=max(row["max_p1_sum_error_bb"] for row in lines),
                p1_sum_ok=all(row["p1_sum_ok"] for row in lines),
                classification=classify(pct["delta_sum"], pct["current_sum"], residual))


def analyze(reach, bands, tasks, root, variant, flop_ev, current):
    leaves = {leaf["line"]: leaf for leaf in reach["leaves"]}
    reference = {row["line"]: row for row in flop_ev}
    means = {(row["leaf"], row["seat"]): row["mean"] for row in current["variants"]["E2"]["leaf_seats"]}
    lines = []
    for band, action_lines, flops in bands:
        for index, line in enumerate(action_lines, 1):
            leaf, ref = leaves[line], reference[line]
            diag_path = Path(root) / band / variant / f"diag{index}.txt"
            diag = read_diag(diag_path)
            records = []
            for flop in flops:
                key = flop_key(flop)
                p1 = tasks[line][key]
                seats = {}
                p1_sum = 0.0
                for seat_text, seat_info in leaf["seats"].items():
                    seat = int(seat_text)
                    p1_seat = p1["seats"][seat_text]
                    values = diag[key][seat]
                    total = l1 = truth = checkdown = 0.0
                    used = 0
                    for name, reach_leaf in zip(reach["classes"], seat_info["reach_leaf"]):
                        if reach_leaf <= 1e-6 or name not in p1_seat["classes"]:
                            continue
                        cell = values[name]
                        if cell["matchup_mass"] <= 0:
                            continue
                        weight = reach_leaf * cell["matchup_mass"]
                        total += weight
                        l1 += weight * cell["ev_l1"]
                        truth += weight * p1_seat["classes"][name][0]
                        checkdown += weight * cell["ev_checkdown"]
                        used += 1
                    if total <= 0:
                        raise ValueError(f"no joint mass: {leaf['leaf']} {flop} seat {seat}")
                    seats[seat_info["role"]] = dict(seat=seat, D=total, L=l1 / total,
                                                     P=truth / total, K=checkdown / total,
                                                     delta=(l1 - truth) / total, classes=used,
                                                     contribution=p1_seat["contribution"])
                    p1_sum += p1_seat["mean"] + p1_seat["contribution"]
                if set(seats) != set(ROLES):
                    raise ValueError(f"expected caller and raiser: {line}")
                residual = sum(s["K"] + s["contribution"] for s in seats.values()) - (ref["pot"] - rake(ref["pot"]))
                sum_error = abs(p1_sum - (p1["summary"]["ev_oop"] + p1["summary"]["ev_ip"]))
                records.append(dict(flop=flop, seats=seats,
                                    weight=sum(s["D"] for s in seats.values()) / 2,
                                    identity_residual=residual, p1_sum_error_bb=sum_error,
                                    mass_ratio=seats["caller"]["D"] / seats["raiser"]["D"]))
            total = sum(row["weight"] for row in records)
            row = dict(leaf=leaf["leaf"], line=line, band=band, pot=ref["pot"],
                       probability=ref["probability"], flops=records)
            for role in ROLES:
                row[f"delta_{role}"] = sum(r["weight"] * r["seats"][role]["delta"] for r in records) / total
                seat = records[0]["seats"][role]["seat"]
                row[f"current_{role}"] = means[(leaf["leaf"], seat)]
            row["delta_sum"] = row["delta_caller"] + row["delta_raiser"]
            row["current_sum"] = row["current_caller"] + row["current_raiser"]
            row["identity_residual"] = sum(r["weight"] * r["identity_residual"] for r in records) / total
            row["max_flop_identity_residual_pct_pot"] = max(abs(r["identity_residual"]) for r in records) / row["pot"] * 100
            row["max_mass_ratio_deviation"] = max(abs(r["mass_ratio"] - 1) for r in records)
            row["max_p1_sum_error_bb"] = max(r["p1_sum_error_bb"] for r in records)
            row["p1_sum_ok"] = row["max_p1_sum_error_bb"] <= 0.02
            lines.append(row)
    return dict(format="p2-seat-bias", version=1, variant=variant, lines=lines,
                bands={band: aggregate([r for r in lines if r["band"] == band]) for band, _, _ in bands},
                overall=aggregate(lines),
                identity_failures=[dict(leaf=r["leaf"], line=r["line"], band=r["band"],
                                        residual_bb=r["identity_residual"],
                                        residual_pct_pot=r["identity_residual"] / r["pot"] * 100)
                                   for r in lines if abs(r["identity_residual"]) / r["pot"] > 0.002])


def markdown(result):
    rows = ["# 両席の値の偏り（E2）", "",
            "各flopの重みは reach_leaf × matchup_mass（P1にあるclassのみ）。flop間は両席のDの平均、line間は参照の到達確率で平均。% potはlineごとの比を到達確率で平均する。",
            "恒等式の残差はcheckdown値＋P1 contributionの和からpot−rake(pot)を引く。分類の残差判定はflop加重平均後のlineの最大絶対値。Dのずれは全line・flopの max |D_caller/D_raiser−1|。", ""]
    if result["identity_failures"]:
        rows += ["**警告：恒等式の残差がpotの0.2%を超えるlineがある。新しいAWS起動を止め、判断が必要。**", ""]
    rows += ["|帯|Δ caller (% pot)|Δ raiser (% pot)|Δ sum (% pot)|現在の和 (% pot)|最大残差 (% pot)|最大D比のずれ|分類|",
             "|---|---:|---:|---:|---:|---:|---:|---|"]
    for band, row in list(result["bands"].items()) + [("全体", result["overall"])]:
        pct = row["pct_pot"]
        rows.append(f"|{band}|{pct['delta_caller']:.6f}|{pct['delta_raiser']:.6f}|{pct['delta_sum']:.6f}|{pct['current_sum']:.6f}|{row['max_identity_residual_pct_pot']:.6f}|{row['max_mass_ratio_deviation']:.9g}|{row['classification']}|")
    rows += ["", "## BBでの到達確率加重平均", "",
             "|帯|Δ caller|Δ raiser|Δ sum|現在 caller|現在 raiser|現在の和|残差|P1合計の最大誤差|P1合計 ≤0.02 BB|",
             "|---|---:|---:|---:|---:|---:|---:|---:|---:|---|"]
    for band, row in list(result["bands"].items()) + [("全体", result["overall"])]:
        cells = "|".join(f"{row['bb'][key]:.6f}" for key in ("delta_caller", "delta_raiser", "delta_sum", "current_caller", "current_raiser", "current_sum", "identity_residual"))
        rows.append(f"|{band}|{cells}|{row['max_p1_sum_error_bb']:.9g}|{row['p1_sum_ok']}|")
    rows += ["", "## Lineごとの証拠", "",
             "|leaf|帯|pot|Δ caller (% pot)|Δ raiser (% pot)|Δ sum (% pot)|残差 (BB)|残差 (% pot)|flop最大絶対残差 (% pot)|最大D比のずれ|",
             "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for row in result["lines"]:
        cells = "|".join(f"{row[key] / row['pot'] * 100:.6f}" for key in ("delta_caller", "delta_raiser", "delta_sum"))
        rows.append(f"|{row['leaf']}|{row['band']}|{row['pot']:g}|{cells}|{row['identity_residual']:.9f}|{row['identity_residual'] / row['pot'] * 100:.6f}|{row['max_flop_identity_residual_pct_pot']:.6f}|{row['max_mass_ratio_deviation']:.9g}|")
    rows += ["", "完全なline文字列、flop別のD・L・P・K、現在のseat別平均、P1合計の検査はJSONに保存。", ""]
    return "\n".join(rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reach")
    parser.add_argument("--bands", required=True)
    parser.add_argument("--p1", required=True)
    parser.add_argument("--l1", nargs=2, required=True, metavar=("ROOT", "VARIANT"))
    parser.add_argument("--flop-ev", required=True)
    parser.add_argument("--class-error", required=True)
    parser.add_argument("--json", required=True)
    parser.add_argument("--markdown", required=True)
    args = parser.parse_args()
    result = analyze(load(args.reach), read_bands(args.bands), p1_tasks(args.p1), *args.l1,
                     load(args.flop_ev), load(args.class_error))
    result["inputs"] = vars(args)
    for path, text in ((args.json, json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False) + "\n"),
                       (args.markdown, markdown(result))):
        Path(path).parent.mkdir(parents=True, exist_ok=True)
        Path(path).write_text(text, encoding="utf-8", newline="\n")
    for band, row in list(result["bands"].items()) + [("overall", result["overall"])]:
        print(band, json.dumps({"classification": row["classification"], "pct_pot": row["pct_pot"],
                               "max_identity_residual_pct_pot": row["max_identity_residual_pct_pot"],
                               "max_mass_ratio_deviation": row["max_mass_ratio_deviation"]}, ensure_ascii=False))
    for row in result["identity_failures"]:
        print("IDENTITY_FAILURE", json.dumps(row, ensure_ascii=False))


if __name__ == "__main__":
    main()
