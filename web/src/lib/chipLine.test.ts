import { describe, expect, it } from "vitest";
import { amountChips, amountDisplay, amountInput, lineToken, normalizedWager, replayedLine, writtenLine } from "./format";
import type { LineStep } from "../api/types";

describe("line tokens", () => {
  it("writes exact chip amounts in the canonical grammar", () => {
    const raise = (to: number) => ({ action: { "raise-to": { to } } }) as LineStep;
    expect(lineToken(raise(10 / 3), 3)).toBe("r10c");
    expect(lineToken(raise(2.5), 1000)).toBe("r2.5");
  });

  it("keeps a fractional-chip BB wager for daemon validation", () => {
    expect(writtenLine("BTN r1.5", "", ["r1.4"], ["r1.5"])).toBe("BTN r1.4");
    expect(writtenLine("BTN r2 / BB b3", "BTN r1.9", ["r2", "b2.8"], ["r2", "b3"])).toBe("BTN r1.9 / BB b2.8");
    expect(writtenLine("BTN r2, BB c", "BTN r1.9", ["r2", "c"], ["r2", "c"])).toBe("BTN r1.9, BB c");
    expect(writtenLine("BTN r2", "BTN r1.9", ["r2", "f"], ["r2", "f"])).toBe("BTN r1.9");
  });

  it("uses only the successfully replayed line after a failed replay", () => {
    const requested = "BTN r2.5, BB c";
    const fallback = replayedLine("", requested, false);
    expect(writtenLine("BTN r3", fallback, ["r3"], ["r3"])).toBe("BTN r3");
  });

  it("normalizes a typed wager before replay", () => {
    expect(normalizedWager("2.50")).toBe("2.5");
  });

  it("associates a canonical replay with the written line after reload", () => {
    expect(replayedLine("BTN r2.5, BB c", "BTN r2500c, BB c", true)).toBe("BTN r2500c, BB c");
  });

  it("reads and displays BB and exact chip amounts", () => {
    expect(amountDisplay("2500c", 1000)).toBe("2.5");
    expect(amountDisplay(2.5, 1000)).toBe("2.5");
    expect(amountDisplay("2.5bb", 1000)).toBe("2.5");
    expect(amountDisplay("8c", 3)).toBe("8c");
    expect(amountDisplay("2.5bb", 3, "down")).toBe("7c");
    expect(amountChips("2.5bb", 3, "up")).toBe(8n);
    expect(amountDisplay("9007199254740993000c", 1000)).toBe("9007199254740993");
    expect(amountInput("2.50")).toBe(2.5);
    expect(amountInput("2.5bb")).toBe("2.5bb");
    expect(amountInput("250c")).toBe("250c");
  });
});
