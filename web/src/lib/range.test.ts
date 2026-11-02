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
    expect(classesOf("77 - 99")).toEqual(new Set(["77", "88", "99"]));
    expect(classesOf("KA")).toEqual(new Set(["AKs", "AKo"]));
  });

  it("counts combos per class and lets later entries overwrite", () => {
    const { weights, errors } = parseRange("AA, AKs:0.5, AKo, AhKh:1, AKo:0");
    expect(errors).toEqual([]);
    expect(rangeCounts(weights, [])).toEqual({ classes: 2, combos: 6 + 0.5 * 3 + 1 });
  });

  it("accepts finite f32 weight syntax", () => {
    expect(parseRange("AA:.5,KK:5e-1,QQ:+0.5").errors).toEqual([]);
    expect(rangeCounts(parseRange("AA:1e-1").weights, []).combos).toBe(6 * Math.fround(0.1));
  });

  it.each([
    ["0.50000002980232238769531249999999999999", 0.5],
    ["0.5000000298023223876953125", 0.5],
    ["0.50000002980232238769531250000000000001", 0.5000000596046448],
    ["0.50000008940696716308593749999999999999", 0.5000000596046448],
    ["0.5000000894069671630859375", 0.5000001192092896],
    ["0.50000008940696716308593750000000000001", 0.5000001192092896],
    ["7.00649232162408535461864791644958065640130970938257885878534141944895541342930300743319094181060791015625e-46", 0],
    ["7.00649232162408535461864791644958065640130970938257885878534141944895541342930300743319094181060791015626e-46", 2 ** -149],
  ])("rounds decimal %s directly to f32", (decimal, expected) => {
    const { weights, errors } = parseRange(`AhAs:${decimal}`);
    expect(errors).toEqual([]);
    expect(weights.get("AsAh")).toBe(expected);
  });

  it.each([
    ["1.000000059604644775390625", true],
    ["1.00000005960464477539062500000000000001", false],
    ["-0.0000000000000000000000000000000000000000000001", true],
    ["-0.000000000000000000000000000000000000000000001", false],
  ])("validates the rounded f32 weight %s", (decimal, valid) => {
    expect(parseRange(`AA:${decimal}`).errors.length === 0).toBe(valid);
  });

  it.each(["\u0009", "\u0085", "\u00a0", "\u1680", "\u2007", "\u2028", "\u202f", "\u205f", "\u3000"])(
    "accepts Rust whitespace %j at entry, weight and interval boundaries",
    (space) => {
      const parsed = parseRange(`${space}77${space}-${space}99${space}:${space}0.5${space}`);
      expect(parsed.errors).toEqual([]);
      expect(classesOf(`${space}77${space}-${space}99${space}`)).toEqual(new Set(["77", "88", "99"]));
    },
  );

  it.each(["\ufeff", "\u180e", "\u200b"])("rejects non-Rust whitespace %j", (space) => {
    for (const text of [`${space}AA`, `AA${space}`, `AA:${space}0.5`, `77${space}-99`]) {
      expect(parseRange(text).errors).toHaveLength(1);
    }
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
      "Q5s:1.5: weight must be a finite number in 0..1",
      "ZZ: Cannot parse entry",
    ]);
  });

  it("reads random only as the whole string", () => {
    expect(rangeCounts(parseRange("random").weights, []).combos).toBe(1326);
    expect(parseRange("random, AA").errors).toHaveLength(1);
    expect(parseRange(" random ").errors).toHaveLength(1);
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
