import { describe, expect, it } from "vitest";
import type { PreviewTreeRow, StartState } from "../api/types";
import { describeAction, outcome, rootStacks, sizePercent, splitStreets, wagersAt } from "./tree";

const row = (history: string, pot: number, actor: "oop" | "ip", actions: string[], kinds: string[]): PreviewTreeRow => ({
  history,
  kind: "action",
  street: "flop",
  actor,
  pot,
  actions,
  children: actions.map((_, i) => `${history}#${i}`),
  child_kinds: kinds as PreviewTreeRow["child_kinds"],
});

// The rows `solvers tree --rest-of-street` gives for examples/hu-postflop/flop_srp.toml (pot 5.5, 97.5 behind).
const stacks = { oop: 97.5, ip: 97.5 };

describe("wagersAt", () => {
  it("tracks the street's wagers and resets them, keeping what went in, at each deal", () => {
    expect(wagersAt("xr1.815r6.38", 1000)).toEqual({ wager: { oop: 6.38, ip: 1.815 }, earlier: { oop: 0, ip: 0 } });
    expect(wagersAt("r2c[8s]r3", 1000)).toEqual({ wager: { oop: 3, ip: 0 }, earlier: { oop: 2, ip: 2 } });
    expect(wagersAt("xx[dealN]x", 1000)).toEqual({ wager: { oop: 0, ip: 0 }, earlier: { oop: 0, ip: 0 } });
    expect(wagersAt("r10cc[8s]", 3)).toEqual({ wager: { oop: 0, ip: 0 }, earlier: { oop: 10 / 3, ip: 10 / 3 } });
  });
});

describe("describeAction", () => {
  it("reads a bet as a share of the pot and a raise over the pot after calling, as the size literals do", () => {
    const root = row("", 5.5, "oop", ["check", "bet 1.815"], ["action", "action"]);
    expect(describeAction(root, 1, stacks, 1000)).toMatchObject({ kind: "bet", amount: 1.815, allIn: false });
    expect(describeAction(root, 1, stacks, 1000).potPercent).toBeCloseTo(33, 6);
    const facing = row("xr1.815", 7.315, "oop", ["fold", "call", "raise to 6.38"], ["terminal", "chance", "action"]);
    expect(describeAction(facing, 1, stacks, 1000)).toMatchObject({ kind: "call", amount: 1.815, allIn: false });
    const raise = describeAction(facing, 2, stacks, 1000);
    expect(raise.potPercent).toBeCloseTo(50, 6);
    expect(raise.times).toBeCloseTo(6.38 / 1.815, 6);
  });

  it("marks a wager or call of every chip behind as all-in, counting earlier streets", () => {
    const shove = row("xr1.815r6.38", 13.695, "ip", ["fold", "call", "raise to 97.5"], ["terminal", "chance", "action"]);
    expect(describeAction(shove, 2, stacks, 1000).allIn).toBe(true);
    const call = row("xr1.815r6.38r97.5", 109.38, "oop", ["fold", "call"], ["terminal", "chance"]);
    expect(describeAction(call, 1, stacks, 1000)).toMatchObject({ kind: "call", amount: 91.12, allIn: true });
    // On the turn after 10 went in on the flop, 87.5 is everything left.
    const turn = row("r10c[8s]", 25.5, "oop", ["check", "bet 50", "bet 87.5"], ["action", "action", "action"]);
    expect(describeAction(turn, 1, stacks, 1000).allIn).toBe(false);
    expect(describeAction(turn, 2, stacks, 1000).allIn).toBe(true);
    expect(describeAction(turn, 2, null, 1000).allIn).toBe(false);
    const chips = row("", 5, "oop", ["bet 10c"], ["action"]);
    expect(describeAction(chips, 0, { oop: 10 / 3, ip: 10 / 3 }, 3)).toMatchObject({ amount: 10 / 3, allIn: true });
  });
});

describe("outcome", () => {
  it("tells a further decision, a deal, a fold and a showdown apart", () => {
    const river = row("x", 5, "ip", ["fold", "check", "bet 1", "raise to 3"], ["terminal", "terminal", "action", "chance"]);
    expect([0, 1, 2, 3].map((i) => outcome(river, i))).toEqual(["fold", "showdown", "decision", "deal"]);
  });
});

describe("rootStacks", () => {
  it("reads each role's stack behind from the start's seats", () => {
    const start = {
      oop: { position: "BB", seat: 6 },
      ip: { position: "BTN", seat: 4 },
      seats: [
        { seat: 4, position: "BTN", starting_stack: 100, remaining_stack: 97.5, folded: false },
        { seat: 6, position: "BB", starting_stack: 80, remaining_stack: 77.5, folded: false },
      ],
    } as StartState;
    expect(rootStacks(start)).toEqual({ oop: 77.5, ip: 97.5 });
    expect(rootStacks(null)).toBeNull();
  });
});

describe("sizePercent", () => {
  it("writes whole percents from 10 and one decimal below", () => {
    expect([33.0000001, 9.96, 2.5].map(sizePercent)).toEqual(["33%", "10%", "2.5%"]);
  });
});

describe("splitStreets", () => {
  it("splits concatenated selections back at each requested root", () => {
    const rows = ["", "x", "r1", "xx[8s]", "xx[8s]x"].map((history) => row(history, 1, "oop", [], []));
    expect(splitStreets(rows, ["", "xx[8s]"]).map((street) => street.map((r) => r.history))).toEqual([
      ["", "x", "r1"],
      ["xx[8s]", "xx[8s]x"],
    ]);
  });
});
