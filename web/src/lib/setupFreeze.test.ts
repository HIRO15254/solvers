import { describe, expect, it, vi } from "vitest";
import type { RunSummary } from "../api/types";
import { freezeRequestDoc, isFreezeBase, loadFreezeBases } from "./setupFreeze";
import { readDoc, writeDoc } from "./config";

function run(runId: string, state: RunSummary["state"], gameKind = "hu-postflop"): RunSummary {
  return { runId, state, gameKind, configHash: "", eventsOffset: 0, resumable: false };
}

const solution = [{ name: "solution.sol", bytes: 10 }];

describe("Setup freeze base choices", () => {
  it("omits the CLI path only from requests using a daemon base", () => {
    const doc = readDoc('schema = "solvers.nlh/v1"\n[profile.freeze_upstream]\nsolution = "local.sol"\nsha256 = "abc"\n[[profile.lock]]\nnode = "x"\nstrategy = { check = 1 }\n');
    const request = freezeRequestDoc(doc, "base");
    expect(request.profile).toEqual({ lock: [{ node: "x", strategy: { check: 1 } }] });
    expect(readDoc(writeDoc(doc))).toEqual(doc);
    expect(freezeRequestDoc(doc, "")).toBe(doc);
    expect((doc.profile as Record<string, unknown>).freeze_upstream).toEqual({ solution: "local.sol", sha256: "abc" });
  });

  it("requires a stopped P1 run and a solution", () => {
    for (const state of ["completed", "canceled", "interrupted"] as const) {
      expect(isFreezeBase(run(state, state), solution)).toBe(true);
      expect(isFreezeBase(run(state, state), [])).toBe(false);
    }
    for (const state of ["queued", "running", "failed"] as const) {
      expect(isFreezeBase(run(state, state), solution)).toBe(false);
    }
    expect(isFreezeBase(run("p2", "completed", "mw-preflop"), solution)).toBe(false);
  });

  it("loads artifacts only for eligible states and surfaces errors", async () => {
    const runs = [run("base", "completed"), run("missing", "canceled"), run("live", "running")];
    const artifacts = vi.fn(async (id: string) => id === "base" ? solution : []);
    expect(await loadFreezeBases(async () => runs, artifacts)).toEqual([runs[0]]);
    expect(artifacts).toHaveBeenCalledTimes(2);
    await expect(loadFreezeBases(async () => runs, async () => { throw new Error("unavailable"); })).rejects.toThrow("unavailable");
  });
});
