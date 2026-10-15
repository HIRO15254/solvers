import { describe, expect, it } from "vitest";
import { DEFAULT_SIZES, readSizeScript, sizeCellAt, sizeScript, type SizeTable } from "./sizes";

const none = () => ({ oop: { bet: [], raise: [] }, ip: { bet: [], raise: [] } });

describe("size table scripts", () => {
  it("reads back every table it writes", () => {
    const lopsided: SizeTable = {
      sizes: { flop: none(), turn: { oop: { bet: [], raise: ["3x"] }, ip: { bet: ["e"], raise: [] } }, river: none() },
      noDonk: true,
      reraiseAllIn: true,
    };
    const empty: SizeTable = { sizes: { flop: none(), turn: none(), river: none() }, noDonk: true, reraiseAllIn: true };
    for (const table of [DEFAULT_SIZES, { ...DEFAULT_SIZES, noDonk: false, reraiseAllIn: false }, lopsided, empty]) {
      expect(readSizeScript(sizeScript(table))).toEqual(table);
    }
    expect(sizeScript(empty)).toBe("");
  });

  it("leaves scripts it cannot write back as scripts", () => {
    const written = sizeScript(DEFAULT_SIZES);
    for (const script of [
      "flop {\n  when donk { remove bet }\n  replace bet [33]\n}\n",
      written.replace("}\nturn {", "  when spr > 3 { replace raise [50] }\n}\nturn {"),
      written.replace("replace raise [50] }", "replace raise [] }"),
      written.replace("turn {", "flop {"),
      written.slice(0, written.lastIndexOf("}")),
    ]) {
      expect(readSizeScript(script)).toBeNull();
    }
  });

  it("finds the cell that wrote a validator-numbered line", () => {
    const script = sizeScript(DEFAULT_SIZES);
    // Line 1 is the header; flop opens on line 2.
    expect(sizeCellAt(script, 3)).toEqual({ street: "flop", role: "oop", kind: "bet" });
    expect(sizeCellAt(script, 11)).toEqual({ street: "turn", role: "ip", kind: "bet" });
    expect(sizeCellAt(script, 19)).toEqual({ street: "river", role: "oop", kind: "raise" });
    for (const line of [1, 2, 7, 8, 99]) expect(sizeCellAt(script, line)).toBeNull();
  });
});
