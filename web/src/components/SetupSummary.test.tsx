import { createElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { ValidationCard } from "./SetupSummary";

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
