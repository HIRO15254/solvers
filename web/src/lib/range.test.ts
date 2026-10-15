import { describe, expect, it } from "vitest";
import { classCells, classOf, formatRange, parseRange, rangeCounts } from "./range";

const classesOf = (text: string) =>
  new Set([...parseRange(text).weights].filter(([, w]) => w > 0).map(([combo]) => classOf(combo)));

describe("parseRange", () => {
  it("expands plus, kicker and same-gap spans", () => {
    expect(classesOf("TT+")).toEqual(new Set(["TT", "JJ", "QQ", "KK", "AA"]));
    expect(classesOf("K9s+")).toEqual(new Set(["K9s", "KTs", "KJs", "KQs"]));
    expect(classesOf("ATs-A8s")).toEqual(new Set(["ATs", "A9s", "A8s"]));
    expect(classesOf("T9s-87s")).toEqual(new Set(["T9s", "98s", "87s"]));
    expect(classesOf("77-99")).toEqual(new Set(["77", "88", "99"]));
    expect(classesOf("KA")).toEqual(new Set(["AKs", "AKo"]));
  });

  it("counts combos per class and lets later entries overwrite", () => {
    const { weights, errors } = parseRange("AA, AKs:0.5, AKo, AhKh:1, AKo:0");
    expect(errors).toEqual([]);
    expect(rangeCounts(weights, [])).toEqual({ classes: 2, combos: 6 + 0.5 * 3 + 1 });
  });

  it("removes combos that share a card with the board", () => {
    const { weights } = parseRange("AA,KK");
    expect(rangeCounts(weights, ["As", "7h", "2d"]).combos).toBe(3 + 6);
    expect(classCells(weights, ["As"]).get("AA")).toMatchObject({ weight: 1, blocked: 3 });
  });

  it("reports entries the grammar rejects", () => {
    const { errors } = parseRange("AAs, AhAh, AKs-QJo, Q5s:1.5, ZZ");
    expect(errors).toEqual([
      "AAs: Pairs cannot use s/o",
      "AhAh: the same card cannot be used twice",
      "AKs-QJo: Both ends of an interval must use the same s/o suffix",
      "Q5s:1.5: weight must be a decimal number in 0..1",
      "ZZ: Cannot parse entry",
    ]);
  });

  it("reads random only as the whole string", () => {
    expect(rangeCounts(parseRange("random").weights, []).combos).toBe(1326);
    expect(parseRange("random, AA").errors).toHaveLength(1);
  });
});

describe("formatRange", () => {
  it("compresses runs into the shortest entries", () => {
    expect(formatRange(parseRange("22+,A2s+,KTs-K8s,QJo").weights)).toBe("22+,A2s+,KTs-K8s,QJo");
  });

  it("round-trips mixed weights and partial classes", () => {
    const text = "99-22:0.5,AJs-A2s,KJs-K2s,AhKh,AsKd:0.25,T8o+,98o:0.75";
    const weights = parseRange(text).weights;
    expect(parseRange(formatRange(weights)).weights).toEqual(new Map([...weights].filter(([, w]) => w > 0)));
  });
});
