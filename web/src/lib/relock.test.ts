import { describe, expect, it } from "vitest";
import { parse } from "smol-toml";
import { classesToRange, relockPayload, roundedStrategy, selectionStrategy } from "./profile";
import type { StrategyRow } from "../api/types";

describe("relock", () => {
  it("writes queued locks as parseable profile fragments and payload flags", () => {
    const payload = relockPayload([
      { node: "root", hands: "AA,AKs", strategy: { check: 0.25, "bet 3.3": 0.75 }, confidence: 0.8 },
      { node: "x", category: "toppair & !flushdraw", strategy: { check: 1 } },
    ], true, true);
    expect(payload).toMatchObject({ replace: true, freezeUpstream: true, clearProfile: false });
    expect(parse(payload.profileToml)).toEqual({ profile: { lock: [
      { node: "root", hands: "AA,AKs", strategy: { check: 0.25, "bet 3.3": 0.75 }, confidence: 0.8 },
      { node: "x", category: "toppair & !flushdraw", strategy: { check: 1 } },
    ] } });
  });
  it("converts grid selection into deterministic range syntax", () => {
    expect(classesToRange(new Set(["AKo", "AA", "AKs"]))).toBe("AA,AKs,AKo");
    expect(classesToRange([])).toBe("");
  });
  it("rounds prefill to thousandths and submits the displayed values", () => {
    const values = roundedStrategy([0.46029698848724365, 0.2698, 0.26990301151275635]);
    expect(values).toEqual(["0.460", "0.270", "0.270"]);
    const strategy = Object.fromEntries(["check", "bet", "raise"].map((action, i) => [action, Number(values[i])]));
    const payload = relockPayload([{ node: "root", strategy }], false, false);
    expect(parse(payload.profileToml)).toEqual({ profile: { lock: [{ node: "root", strategy }] } });
    expect(roundedStrategy([0.5006, 0.4996, 0])).toEqual(["0.501", "0.499", "0.000"]);
    expect(roundedStrategy([0, 0])).toEqual(["0", "0"]);
  });
  it("prefills reach weighted strategy for selected classes", () => {
    const rows: StrategyRow[] = [
      { history: "", actor: "oop", combo: "AsAh", weight: 1, probabilities: [1, 0] },
      { history: "", actor: "oop", combo: "AdAc", weight: 3, probabilities: [0, 1] },
      { history: "", actor: "oop", combo: "AsKs", weight: 2, probabilities: [1, 0] },
    ];
    expect(selectionStrategy(rows, ["check", "bet"], new Set(["AA"]))).toEqual([0.25, 0.75]);
    expect(selectionStrategy(rows, ["check", "bet"])).toEqual([0.5, 0.5]);
  });
});
