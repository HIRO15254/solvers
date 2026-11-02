import { createElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { RunSummary } from "../api/types";
import { ResultsButton } from "./Runs";

vi.mock("../cryst", () => {
  const container = ({ children }: { children?: ReactNode }) => createElement("div", null, children);
  return {
    Card: container, CardContent: container, Button: container, EmptyState: container,
    Icon: container, Select: container, Tabs: container, Alert: container, Badge: container,
    Input: container, Label: container, Popover: container, PopoverContent: container, PopoverTrigger: container,
  };
});

describe("run Results button", () => {
  it("only offers Results for terminal heads-up runs with a solution", () => {
    const run: RunSummary = { runId: "resume", state: "completed", gameKind: "hu-postflop", configHash: "x", eventsOffset: 0, resumable: true };
    expect(renderToStaticMarkup(<ResultsButton run={run} hasSolution />)).toContain("Results");
    expect(renderToStaticMarkup(<ResultsButton run={{ ...run, state: "running" }} hasSolution />)).toBe("");
    expect(renderToStaticMarkup(<ResultsButton run={{ ...run, state: "queued" }} hasSolution />)).toBe("");
    expect(renderToStaticMarkup(<ResultsButton run={run} hasSolution={false} />)).toBe("");
    expect(renderToStaticMarkup(<ResultsButton run={{ ...run, gameKind: "other" }} hasSolution />)).toBe("");
  });
});
