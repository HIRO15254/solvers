import { createElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Doc } from "../lib/config";
import { DocContext } from "./DocFields";
import { SetupTable } from "./SetupTable";

vi.mock("../cryst", () => {
  const container = ({ children }: { children?: ReactNode }) => createElement("div", null, children);
  return {
    Button: container, Card: container, Label: container, SegmentedControl: container,
    Select: container, Dialog: container, Icon: container, Popover: container, PopoverTrigger: container, PopoverContent: container,
    Input: ({ value, label }: { value?: string; label?: string }) => createElement("input", { value, "aria-label": label, readOnly: true }),
  };
});

function renderTable(doc: Doc, issue: { path: string; message: string } | null = null) {
  return renderToStaticMarkup(
    <DocContext.Provider value={{ doc, issue, setDoc: () => {} }}><SetupTable /></DocContext.Provider>,
  );
}

describe("setup table amounts", () => {
  it("reads chip-denominated stack and rake fields and associates new-key issues", () => {
    const html = renderTable({ table: { players: 2, stack: "100c", bb_chips: 3 }, economics: { rake: { cap: "2c", rounding_unit: "1c" } } }, { path: "economics.rake.cap", message: "invalid cap" });
    expect(html).toContain('value="100c"');
    expect(html).toContain('value="2c"');
    expect(html).toContain('value="1c"');
  });

  it("recognizes chip-denominated BB antes and tournament outside stacks", () => {
    const html = renderTable({ table: { players: 2, stack: 100, bb_ante: "3c" }, economics: { kind: "tournament", outside_field: ["9c", "2bb"] } });
    expect(html).toContain('value="0.003"');
    expect(html).toContain('value="0.009, 2"');
  });
});
