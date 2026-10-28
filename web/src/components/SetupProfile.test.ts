import { describe, expect, it } from "vitest";
import { readDoc, writeDoc } from "../lib/config";
import { changeProfileEntry, replaceProfileLocks } from "../lib/profile";

describe("profile document editing", () => {
  it("round-trips locks, combo strategies, incentives and frequency targets as TOML", () => {
    let doc = readDoc('schema = "solvers.nlh/v1"\n[meta]\nname = "spot"\n');
    doc = changeProfileEntry(doc, "lock", 0, { node: "x b3.3", category: "toppair & !flushdraw", strategy: { check: 0.5, "bet 3.3": 0.5 }, confidence: 0.8 });
    doc = changeProfileEntry(doc, "lock", 1, { node: "x [?]", combos: { AsKs: { call: 1 }, AhKh: { fold: 1 } } });
    doc = changeProfileEntry(doc, "incentive", 0, { node: "x", hands: "AA,KK", action: "bet 3.3", pot: 0.03 });
    doc = changeProfileEntry(doc, "frequency", 0, { node: "x", category: "toppair", action: "check", target: 0.5 });
    expect(readDoc(writeDoc(doc))).toEqual(doc);
  });

  it("edits and removes entries while preserving other profile and document keys", () => {
    let doc = readDoc('schema = "solvers.nlh/v1"\n[profile.freeze_upstream]\nsolution = "base.sol"\n[[profile.lock]]\nnode = "x"\nstrategy = { check = 1.0 }\n[[profile.lock]]\nnode = "b"\nstrategy = { fold = 1.0 }\n');
    doc = changeProfileEntry(doc, "lock", 1, { node: "b", hands: "AA", strategy: { call: 0.4 } });
    doc = changeProfileEntry(doc, "lock", 0, null);
    expect(readDoc(writeDoc(doc))).toEqual(doc);
    expect(doc.profile).toEqual({ freeze_upstream: { solution: "base.sol" }, lock: [{ node: "b", hands: "AA", strategy: { call: 0.4 } }] });
    doc = changeProfileEntry(doc, "lock", 0, null);
    expect(doc.profile).toEqual({ freeze_upstream: { solution: "base.sol" } });
  });

  it("replaces all locks for an explicitly imported strategy without changing other profile entries", () => {
    const doc = readDoc('schema = "solvers.nlh/v1"\n[[profile.lock]]\nnode = "x [?]"\nstrategy = { check = 1 }\n[[profile.lock]]\nnode = "b"\nstrategy = { fold = 1 }\n[[profile.incentive]]\nnode = "x"\naction = "check"\npot = 0.1\n');
    const updated = replaceProfileLocks(doc, "x[As]", { AsKs: { check: 1 } });
    expect(updated.profile).toEqual({
      lock: [{ node: "x[As]", combos: { AsKs: { check: 1 } } }],
      incentive: [{ node: "x", action: "check", pot: 0.1 }],
    });
    expect(readDoc(writeDoc(updated))).toEqual(updated);
    expect(doc.profile).toEqual({
      lock: [{ node: "x [?]", strategy: { check: 1 } }, { node: "b", strategy: { fold: 1 } }],
      incentive: [{ node: "x", action: "check", pot: 0.1 }],
    });
  });
});
