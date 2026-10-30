import { describe, expect, it, vi } from "vitest";
import { readDoc, writeDoc } from "../lib/config";
import { treeParamValue } from "./SetupTree";

vi.mock("../cryst", () => ({}));

describe("tree parameter overrides", () => {
  it("preserves size units and writes bare numeric overrides as BB", () => {
    const params = {
      bare: treeParamValue("33", "number")!,
      pot: treeParamValue("33%", "number")!,
      chips: treeParamValue("300c", "number")!,
      bb: treeParamValue("2.5bb", "number")!,
      stack: treeParamValue("85%stack", "token")!,
    };
    expect(params).toEqual({ bare: 33, pot: "33%", chips: "300c", bb: "2.5bb", stack: "85%stack" });
    expect(readDoc(writeDoc({ tree: { params } })).tree).toEqual({ params });
    expect(treeParamValue("  ", "number")).toBeUndefined();
  });
});
