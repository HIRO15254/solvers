import { describe, expect, it, vi } from "vitest";
import { api } from "../api/client";
import type { RunSummary } from "../api/types";
import { runMeta } from "./runMeta";

vi.mock("../api/client", () => ({ api: { artifactText: vi.fn(), validate: vi.fn() } }));

describe("run profile metadata", () => {
  it("shows common stacks in the table's display form in the economics badge", async () => {
    vi.mocked(api.validate).mockRejectedValue(new Error("no validation"));
    for (const [hash, table, economics, expected] of [
      ["stack-bb", "stack = 100", "", "100bb · No rake"],
      ["stack-chips", "stack = '301c'\nbb_chips = 3", "", "301c · No rake"],
      ["stack-chips-rake", "stack = '50000c'", "[economics.rake]\nrate = 0.05", "50bb · rake 5%"],
      ["stack-chips-icm", "stack = '50000c'", "[economics]\nkind = 'tournament'", "50bb · ICM"],
      ["no-common-stack", "", "", "No rake"],
    ] as const) {
      vi.mocked(api.artifactText).mockResolvedValueOnce(`schema = 'solvers.nlh/v1'\n[table]\n${table}\n${economics}\n`);
      const run = { runId: hash, configHash: hash, gameKind: "hu-postflop" } as RunSummary;
      expect((await runMeta(run)).economics).toBe(expected);
    }
  });

  it("renders persisted chip wagers in the table's display form", async () => {
    vi.mocked(api.validate).mockRejectedValue(new Error("no validation"));
    for (const [hash, bbChips, line, expected] of [
      ["line-decimal", 1000, "BTN r2500c, BB c / BB b1250c, BTN c", "BTN r2.5, BB c / BB b1.25, BTN c"],
      ["line-chips", 3, "BTN r6c, BB c / BB b15c, BTN c", "BTN r6c, BB c / BB b15c, BTN c"],
      ["line-integer", 1, "BTN r6c, BB c", "BTN r6, BB c"],
    ] as const) {
      vi.mocked(api.artifactText).mockResolvedValueOnce(
        `schema = 'solvers.nlh/v1'\n[table]\nbb_chips = ${bbChips}\n[spot]\nline = '${line}'\n`,
      );
      const run = { runId: hash, configHash: hash, gameKind: "hu-postflop" } as RunSummary;
      expect((await runMeta(run)).line).toBe(expected);
    }
  });

  it("marks incentive and frequency runs as general-sum, but not lock-only runs", async () => {
    vi.mocked(api.validate).mockRejectedValue(new Error("no validation"));
    for (const [hash, profile, expected] of [
      ["none", "", false],
      ["lock", "[[profile.lock]]\nnode = 'root'\n", false],
      ["incentive", "[[profile.incentive]]\nnode = 'root'\n", true],
      ["frequency", "[[profile.frequency]]\nnode = 'root'\n", true],
    ] as const) {
      vi.mocked(api.artifactText).mockResolvedValueOnce(`schema = 'solvers.nlh/v1'\n${profile}`);
      const run = { runId: hash, configHash: hash, gameKind: "hu-postflop" } as RunSummary;
      expect((await runMeta(run)).generalSum).toBe(expected);
    }
  });

  const frozenToml =
    "schema = 'solvers.nlh/v1'\n[profile.freeze_upstream]\nsolution = '/runs/base/solution.sol'\nsha256 = 'ab'\n[[profile.lock]]\nnode = 'x'\n";
  const validated = { summary: { start: { pot: 2 }, utilityUnit: "BB" } } as never;

  it("validates a frozen run against the run its solution path names, not its relock source", async () => {
    vi.mocked(api.validate).mockReset().mockResolvedValueOnce(validated);
    vi.mocked(api.artifactText).mockResolvedValueOnce(frozenToml);
    const run = { runId: "frozen", configHash: "frozen", gameKind: "hu-postflop", relockedFrom: "parent" } as RunSummary;
    const meta = await runMeta(run);
    const [toml, , base] = vi.mocked(api.validate).mock.calls[0]!;
    expect(toml).not.toContain("freeze_upstream");
    expect(toml).toContain("[[profile.lock]]");
    expect(base).toBe("base");
    expect(meta.start?.pot).toBe(2);
  });

  it("keeps the start state of a frozen run whose base run is gone", async () => {
    vi.mocked(api.validate).mockReset().mockRejectedValueOnce(new Error("unknown run")).mockResolvedValueOnce(validated);
    vi.mocked(api.artifactText).mockResolvedValueOnce(frozenToml);
    const run = { runId: "orphan", configHash: "orphan", gameKind: "hu-postflop" } as RunSummary;
    const meta = await runMeta(run);
    expect(vi.mocked(api.validate).mock.calls.map((call) => call[2])).toEqual(["base", undefined]);
    expect(meta.start?.pot).toBe(2);
  });
});
