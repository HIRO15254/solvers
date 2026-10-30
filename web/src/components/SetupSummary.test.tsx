import { createElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { LineCursor } from "../api/types";
import { SpotCard, ValidationCard } from "./SetupSummary";

vi.mock("../cryst", () => {
  const container = ({ children }: { children?: ReactNode }) => createElement("div", null, children);
  return {
    Alert: ({ title, children }: { title?: ReactNode; children?: ReactNode }) => createElement("div", null, title, children),
    Badge: container, Button: container, Card: container, CardAction: container,
    CardContent: container, CardHeader: container, CardTitle: container, Icon: container,
    IconButton: container, Input: container, Progress: container,
  };
});

describe("validation warnings", () => {
  it("displays the daemon's rounding warnings", () => {
    const warnings = ["table.sb: 0.5 BB rounded to 2 chips (nearest)", "spot.line: 1.4 BB rounded to 4 chips (down)"];
    const html = renderToStaticMarkup(<ValidationCard
      validation={{ state: "ok", response: { effectiveConfigToml: "", summary: {
        status: "ok", schema: "solvers.nlh/v1", gameKind: "holdem", product: "postflop",
        amountUnit: "BB", utilityUnit: "chips", warnings,
      } } }}
      storageAuto={false} errorStep={null} onRevalidate={() => {}}
    />);
    expect(html).toContain("2 warnings");
    for (const warning of warnings) expect(html).toContain(warning);
    expect(html).not.toContain("No warnings");
  });
});

describe("spot table amounts", () => {
  it.each([
    { bbChips: 3, pot: 5 / 3, stacks: [7 / 3, 8 / 3], wager: 1 / 3, potText: "5c", effective: "7c", stack: "8c", wagerText: "1c", label: "pot 5c" },
    { bbChips: 1000, pot: 1.625, stacks: [2.125, 2.5], wager: 0.125, potText: "1.625", effective: "2.125", stack: "2.5", wagerText: "0.125", label: "pot 1.625 BB" },
  ])("formats replayed amounts at $bbChips chips per BB", ({ bbChips, pot, stacks, wager, potText, effective, stack, wagerText, label }) => {
    const cursor: LineCursor = {
      bbChips, pot, moves: [], line: "", actions: [], street: "flop", complete: true,
      boardCards: 0, players: 2, next: null, terminal: false, button: 1,
      seats: [
        { seat: 1, position: "BTN", stack: stacks[0]!, wager, committed: 0, status: "active" },
        { seat: 2, position: "BB", stack: stacks[1]!, wager: 0, committed: 0, status: "active" },
      ],
    };
    const html = renderToStaticMarkup(<SpotCard cursor={cursor} board={[]} ignored={[]}
      roles={{ oop: "BTN", ip: "BB" }} name="" autoName="Spot" selected={[]}
      onName={() => {}} onToggle={() => {}} onBoardDialog={() => {}} />);
    expect(html).toContain(`pot ${potText}`);
    expect(html).toContain(`eff ${effective}`);
    expect(html).toContain(`>${stack}</span>`);
    expect(html).toContain(`>${wagerText}</span>`);
    expect(html).toContain(label);
    expect(html).toContain(`SPR ${(stacks[0]! / pot).toFixed(2)}`);
  });
});
