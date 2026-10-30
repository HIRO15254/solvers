import { describe, expect, it, vi } from "vitest";
import { api } from "../api/client";
import type { RunSummary } from "../api/types";
import { runMeta } from "./runMeta";

vi.mock("../api/client", () => ({ api: { artifactText: vi.fn(), validate: vi.fn() } }));

describe("run profile metadata", () => {
  it("shows BB and chip-denominated common stacks in the economics badge", async () => {
    vi.mocked(api.validate).mockRejectedValue(new Error("no validation"));
    for (const [hash, table, economics, expected] of [
      ["stack-bb", "stack_bb = 100", "", "100bb · No rake"],
      ["stack-chips", "stack_chips = 301\nbb_chips = 3", "", "301c · No rake"],
      ["stack-chips-rake", "stack_chips = 50000", "[economics.rake]\nrate = 0.05", "50000c · rake 5%"],
      ["stack-chips-icm", "stack_chips = 50000", "[economics]\nkind = 'tournament'", "50000c · ICM"],
      ["no-common-stack", "", "", "No rake"],
    ] as const) {
      vi.mocked(api.artifactText).mockResolvedValueOnce(`schema = 'solvers.nlh/v1'\n[table]\n${table}\n${economics}\n`);
      const run = { runId: hash, configHash: hash, gameKind: "hu-postflop" } as RunSummary;
      expect((await runMeta(run)).economics).toBe(expected);
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
