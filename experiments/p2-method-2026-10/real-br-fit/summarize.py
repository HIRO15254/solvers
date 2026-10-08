"""Summarize l0_real outputs with fitted best responses (JSON version 3) as Markdown tables.

Python standard library only.

usage: python summarize.py b1 <dir> [<s4-1a2 b1 dir>]
           heads-up: L0 is the input game there, so every response's held-out gain is at most L0's exact gain;
           <case>_fit_deals.json evaluates L0's best responses on the fitting deals
       python summarize.py b3 <dir> [<s4-1a2 b3 dir>]
           6-max: L0 gains, real gains of the L0 best responses, and the fitted responses' in-sample and held-out gains
With the S4-1a2 directory, also check that the quantities S4-1a2 measured on the same deals are bit-identical
(only the L0 section when the number of evaluation deals differs, as in B3).
"""
import json
import sys
from pathlib import Path

SIX_MAX = ["BTN", "SB", "BB", "UTG", "HJ", "CO"]
HEADS_UP = ["BTN/SB", "BB"]
B3_RUNS = ["20bb_cd", "20bb_cd_s1", "20bb_300k_s0", "20bb_300k_s1"]


def load(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def names(data):
    return HEADS_UP if len(data["real"]["seats"]) == 2 else SIX_MAX


def fitted_sets(data):
    """(label, response-set index, in-sample seats) for the pure and gated fitted responses."""
    sets = [("pure", data["response_sets"].index("fitted"), data["fit"]["seats"])]
    for gated in data["fit"]["gated"]:
        label = f"gated-z{gated['threshold']:g}"
        sets.append((f"z={gated['threshold']:g}", data["response_sets"].index(label), gated["seats"]))
    return sets


def cases(directory, prefix):
    return [p for p in sorted(Path(directory).glob(f"{prefix}*.json"))
            if not p.name.endswith((".profile.json", "_fit_deals.json"))]


def z_score(difference, stderr):
    return difference / stderr if stderr > 0 else 0.0


def b1(directory, previous=None):
    print("Gains minus L0's exact gain (z = difference / stderr). In HU every response's expected held-out gain is at "
          "most L0's. On the fitting deals the pure response's in-sample gain is at least the L0 best response's gain.")
    print()
    sets = None
    worst = float("-inf")
    least_margin = float("inf")
    worst_value = 0.0
    rows = []
    for path in cases(directory, "b1_"):
        data = load(path)
        sets = fitted_sets(data)
        on_fit = load(path.with_name(f"{path.stem}_fit_deals.json"))
        assert (on_fit["real"]["deals"], on_fit["real"]["seed"]) == (data["fit"]["deals"], data["fit"]["seed"])
        for seat, l0, real, fit_real in zip(names(data), data["l0"]["seats"], data["real"]["seats"],
                                            on_fit["real"]["seats"]):
            cells = []
            for label, s, in_sample in [("L0 BR", 0, None)] + sets:
                gain = real["responses"][s]["gain"]
                z = z_score(gain["mean"] - l0["gain"], gain["stderr"])
                worst = max(worst, z)
                cells.append(f"{gain['mean'] - l0['gain']:+.4f} ({z:+.1f})")
            fitted = data["fit"]["seats"][l0["seat"]]
            l0_on_fit = fit_real["responses"][0]["gain"]
            least_margin = min(least_margin, fitted["gain"] - l0_on_fit["mean"])
            worst_value = max(worst_value, abs(fitted["value"] - fit_real["value"]["mean"]))
            fit_cell = (f"{l0_on_fit['mean'] - l0['gain']:+.4f} "
                        f"({z_score(l0_on_fit['mean'] - l0['gain'], l0_on_fit['stderr']):+.1f})")
            rows.append(f"| {path.stem} | {seat} | {l0['gain']:.4f} | {fit_cell} | {fitted['gain'] - l0['gain']:+.4f} | "
                        + " | ".join(cells) + " |")
        if previous is not None:
            identical(data, load(Path(previous) / path.name), path.stem)
    labels = ["L0 BR"] + [label for label, _, _ in sets]
    print("| case | seat | L0 gain | fitting deals: L0 BR | fitting deals: pure | held-out: " + " | ".join(labels) + " |")
    print("|---|---|---|---|---|" + "---|" * len(labels))
    print("\n".join(rows))
    print()
    print(f"largest z of a held-out gain over L0's exact gain: {worst:+.2f}")
    print(f"smallest margin of the pure in-sample gain over the L0 best response's gain on the fitting deals: "
          f"{least_margin:+.3g}")
    print(f"largest difference of the profile's in-sample value from its evaluation on the fitting deals: "
          f"{worst_value:.3g}")
    print()


def b3(directory, previous=None):
    for name in [n for n in B3_RUNS if (Path(directory) / f"{n}.json").exists()]:
        data = load(Path(directory) / f"{name}.json")
        real, fit, timings = data["real"], data["fit"], data["timings"]
        sets = fitted_sets(data)
        print(f"### {name}")
        print()
        print(f"fit: {fit['deals']} deals of seed {fit['seed']}, {timings['fit']:.0f} s "
              f"({fit['deals'] / timings['fit']:,.0f} deals/s); evaluation: {real['deals']} deals of seed "
              f"{real['seed']}, {timings['real']:.0f} s ({real['deals'] / timings['real']:,.0f} deals/s)")
        print()
        print("Each fitted cell: in-sample gain → held-out gain ± stderr.")
        print()
        labels = [label for label, _, _ in sets]
        print("| seat | L0 gain | real gain of the L0 BR | " + " | ".join(labels) + " |")
        print("|---|---|---|" + "---|" * len(sets))
        for seat, l0, r in zip(names(data), data["l0"]["seats"], real["seats"]):
            l0_br = r["responses"][0]["gain"]
            cells = [f"{in_sample[l0['seat']]['gain']:.4f} → {r['responses'][s]['gain']['mean']:.4f} ± "
                     f"{r['responses'][s]['gain']['stderr']:.4f}" for _, s, in_sample in sets]
            print(f"| {seat} | {l0['gain']:.4f} | {l0_br['mean']:.4f} ± {l0_br['stderr']:.4f} | " + " | ".join(cells) + " |")
        l0_br = real["response_gain_sums"][0]
        cells = [f"{sum(x['gain'] for x in in_sample):.4f} → {real['response_gain_sums'][s]['mean']:.4f} ± "
                 f"{real['response_gain_sums'][s]['stderr']:.4f}" for _, s, in_sample in sets]
        print(f"| sum | {data['l0']['nash_conv']:.4f} | {l0_br['mean']:.4f} ± {l0_br['stderr']:.4f} | " + " | ".join(cells) + " |")
        deviations = [f"{label} {sum(x.get('deviations', 0) for x in in_sample):,}" for label, _, in_sample in sets[1:]]
        print()
        print("deviating cells: " + ", ".join(deviations))
        print()
        if previous is not None:
            identical(data, load(Path(previous) / f"{name}.json"), name)
    sweep(directory)
    threads(directory)


def identical(data, old, name):
    """The L0 section and every quantity S4-1a2 measured must be bit-identical on the same deals. With a different
    number of evaluation deals only the L0 section is comparable."""
    real, before = data["real"], old["real"]
    if (real["deals"], real["seed"]) != (before["deals"], before["seed"]):
        same = data["l0"] == old["l0"]
        print(f"{name}: L0 section {'bit-identical' if same else 'DIFFERS'} to S4-1a2 "
              f"(evaluation deals differ: {real['deals']} vs {before['deals']})")
        print()
        return
    same = data["l0"] == old["l0"] and all(
        real[key] == before[key] for key in ("value_sum", "deals", "seed", "mean_deal_attempts"))
    for seat, previous_seat in zip(real["seats"], before["seats"]):
        same &= seat["value"] == previous_seat["value"]
        same &= seat["value_by_active_count"] == previous_seat["value_by_active_count"]
        same &= seat["responses"][0]["value"] == previous_seat["l0_best_response_value"]
        same &= seat["responses"][0]["gain"] == previous_seat["l0_best_response_gain"]
    same &= real["response_gain_sums"][0] == before["l0_best_response_gain_sum"]
    print(f"{name}: S4-1a2 quantities on the same deals {'bit-identical' if same else 'DIFFER'}")
    print()


def sweep(directory):
    """The 300k seed-0 bracket as the number of fitting deals grows (same evaluation deals)."""
    paths = [p for p in Path(directory).glob("20bb_300k_s0*.json") if "_threads" not in p.stem]
    if len(paths) < 2:
        return
    runs = sorted((load(p) for p in paths), key=lambda d: d["fit"]["deals"])
    labels = [label for label, _, _ in fitted_sets(runs[0])]
    print("### fitting deals (20bb_300k_s0)")
    print()
    print("Sums over seats: in-sample → held-out ± stderr.")
    print()
    print("| fitting deals | fit s | " + " | ".join(labels) + " |")
    print("|---|---|" + "---|" * len(labels))
    for data in runs:
        cells = [f"{sum(x['gain'] for x in in_sample):.4f} → {data['real']['response_gain_sums'][s]['mean']:.4f} ± "
                 f"{data['real']['response_gain_sums'][s]['stderr']:.4f}" for _, s, in_sample in fitted_sets(data)]
        print(f"| {data['fit']['deals']:,} | {data['timings']['fit']:.0f} | " + " | ".join(cells) + " |")
    print()


def threads(directory):
    """A thread-count rerun must reproduce everything but the timings."""
    for path in sorted(Path(directory).glob("*_threads*.json")):
        other = load(path)
        base = load(Path(directory) / f"{path.stem.rsplit('_threads', 1)[0]}.json")
        keys = ("l0", "real", "fit", "response_sets", "game_fingerprint", "tables", "k4")
        same = all(base[key] == other[key] for key in keys)
        print(f"{path.stem}: {'bit-identical' if same else 'DIFFERS'} to the default thread count")
        print()


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    {"b1": b1, "b3": b3}[sys.argv[1]](sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None)
