import { createElement, type ReactNode } from "react";
import type * as ReactModule from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { PreviewTreeRow } from "../api/types";
import { TreeCard } from "./TreePreview";

const fixture = vi.hoisted(() => ({ rows: [] as unknown[], stateIndex: 0 }));
vi.mock("react", async (importOriginal) => {
  const react = await importOriginal<typeof ReactModule>();
  return {
    ...react,
    useState: (initial: unknown) => {
      const state = react.useState(initial);
      return fixture.stateIndex++ === 1 ? [fixture.rows, state[1]] : state;
    },
  };
});
vi.mock("../cryst", () => {
  const container = ({ children }: { children?: ReactNode }) => createElement("div", null, children);
  return { Button: container, Card: container, CardAction: container, CardContent: container,
    CardHeader: container, CardTitle: container, EmptyState: container, Icon: container, IconButton: container };
});

function renderPreview(bbChips: number, pot: number, bet: string, history: string): string {
  const rows: PreviewTreeRow[] = [
    { history: "", kind: "action", street: "flop", actor: "oop", pot,
      actions: [`bet ${bet}`], children: [history], child_kinds: ["action"] },
    { history, kind: "action", street: "flop", actor: "ip", pot: pot + Number.parseFloat(bet),
      actions: ["call", `raise to ${bet}`], children: ["call", "raise"], child_kinds: ["chance", "terminal"] },
  ];
  fixture.rows = [rows];
  fixture.stateIndex = 0;
  return renderToStaticMarkup(<TreeCard toml="" ready roles={null} start={null} bbChips={bbChips} />);
}

describe("tree preview amounts", () => {
  it("shows chips in street headers, pots, bets, calls and raises for non-terminating scales", () => {
    const html = renderPreview(3, 5 / 3, "0.6666666666666666", "r2c");
    expect(html).toContain('class="mono meta muted">5c');
    expect(html).toContain("pot 5c");
    expect(html).toContain("pot 7c");
    expect(html).toContain("Bet 2c");
    expect(html).toContain("Call 2c");
    expect(html).toContain("Raise to 2c");
    expect(html).toContain('aria-label="Call 2c"');
  });

  it("retains rounded BB formatting for terminating scales", () => {
    const html = renderPreview(1000, 1.2345678, "0.6666666666666666", "r0.6666666666666666");
    expect(html).toContain('class="mono meta muted">1.234568');
    expect(html).toContain("pot 1.234568");
    expect(html).toContain("Bet 0.666667");
    expect(html).toContain("Call 0.666667");
    expect(html).toContain("Raise to 0.666667");
  });
});
