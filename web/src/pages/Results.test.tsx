import { createElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { ProfileReport } from "../api/types";
import { ProfileDetails } from "./Results";

// The production Cryst bundle requires a browser; these tests exercise the
// profile text and selection rather than the design system's rendering.
vi.mock("../cryst", () => {
  const container = ({ children }: { children?: ReactNode }) => createElement("div", null, children);
  return {
    Card: container, CardContent: container, Button: container, EmptyState: container,
    Icon: container, Select: container, Tabs: container, Alert: container, Badge: container,
    Input: container, Label: container, Popover: container, PopoverContent: container, PopoverTrigger: container,
  };
});

const profile: ProfileReport = {
  summary: {
    matched_nodes: 2, matched_hands: 8, iso_restricted: true,
    selectors: [{ kind: "lock", node: "x [?]", matched_nodes: 2, matched_hands: 8 }],
    resolved_nodes: [
      { node: 3, history: "xx[As]", kind: "lock", entry: 0, confidence: 0.5, partial: true },
      { node: 3, history: "xx[As]", kind: "frequency", entry: 0, confidence: null, partial: null },
      { node: 0, history: "", kind: "frozen", entry: null, confidence: null, partial: null },
    ],
  },
  frozen_nodes: [0], general_sum: true,
  frequencies: [{ entry: 0, node: 3, target: 0.5, achieved: null, error: null, lambda: 0.2 }],
};
const config = { lock: [{ node: "x [?]", hands: "AA", strategy: { check: 1 } }], frequency: [{ action: "check" }] };

describe("profile details", () => {
  it("shows exact resolved-node constraints and unavailable frequency metrics", () => {
    const html = renderToStaticMarkup(<ProfileDetails profile={profile} config={config} history="xx[As]" />).replaceAll("<!-- -->", "");
    expect(html).toContain("confidence 50.0%");
    expect(html).toContain("selected hands");
    expect(html).toContain("AA");
    expect(html).toContain("unavailable");
    expect(html).toContain("Frozen nodes: 1");
  });

  it("does not attribute a wildcard match to a different history", () => {
    const html = renderToStaticMarkup(<ProfileDetails profile={profile} config={config} history="xx[Ah]" />);
    expect(html).not.toContain("confidence 50.0%");
    expect(html).not.toContain("selected hands");
  });

  it("describes hand and category selections, strategies and targets without JSON", () => {
    const report: ProfileReport = {
      ...profile,
      summary: { ...profile.summary, resolved_nodes: [
        { node: 3, history: "xx[As]", kind: "lock", entry: 0, confidence: 0.8, partial: true },
        { node: 3, history: "xx[As]", kind: "incentive", entry: 0, confidence: null, partial: true },
        { node: 3, history: "xx[As]", kind: "frequency", entry: 0, confidence: null, partial: true },
      ] },
      frequencies: [{ entry: 0, node: 3, target: 0.5, achieved: 0.49, error: -0.01, lambda: 0.12345678 }],
    };
    const html = renderToStaticMarkup(<ProfileDetails profile={report} history="xx[As]" config={{
      lock: [{ hands: "AA,KK", strategy: { check: 0.5, "bet 3.3": 0.5 } }],
      incentive: [{ category: "toppair & !flushdraw", action: "bet 3.3", pot: 0.03 }],
      frequency: [{ hands: "AKs", action: "check", target: 0.5 }],
    }} />).replaceAll("<!-- -->", "");
    expect(html).toContain("hands AA,KK");
    expect(html).toContain("strategy check: 50.0%, bet 3.3: 50.0%");
    expect(html).toContain("confidence 80.0%");
    expect(html).toContain("category toppair &amp; !flushdraw");
    expect(html).toContain("action bet 3.3");
    expect(html).toContain("bonus 3.0% of pot");
    expect(html).toContain("hands AKs");
    expect(html).toContain("action check");
    expect(html).toContain("target 50.0%");
    expect(html).toContain("λ 0.1235");
    expect(html).not.toContain('{"');
  });

  it("describes per-combo strategies, all hands and BB incentives", () => {
    const report: ProfileReport = {
      ...profile,
      summary: { ...profile.summary, resolved_nodes: [
        { node: 3, history: "xx[As]", kind: "lock", entry: 0, confidence: 1, partial: true },
        { node: 3, history: "xx[As]", kind: "incentive", entry: 0, confidence: null, partial: false },
      ] },
    };
    const html = renderToStaticMarkup(<ProfileDetails profile={report} history="xx[As]" config={{
      lock: [{ combos: { AsKs: { call: 1 }, AhKh: { fold: 1 } } }],
      incentive: [{ action: "call", bb: 2 }],
    }} />).replaceAll("<!-- -->", "");
    expect(html).toContain("combos AsKs (call: 100.0%), AhKh (fold: 100.0%)");
    expect(html).toContain("all hands");
    expect(html).toContain("bonus 2 BB");
  });
});
