import { describe, expect, it, vi } from "vitest";
import { api } from "../api/client";
import type { RunSummary } from "../api/types";
import { runMeta } from "./runMeta";

vi.mock("../api/client", () => ({ api: { artifactText: vi.fn(), validate: vi.fn() } }));

describe("run profile metadata", () => {
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
});
