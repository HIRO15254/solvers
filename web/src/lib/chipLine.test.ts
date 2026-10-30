import { describe, expect, it } from "vitest";
import { lineToken } from "./format";
import type { LineStep } from "../api/types";

describe("line tokens", () => {
  it("writes exact chip amounts in the canonical grammar", () => {
    const raise = (to: number) => ({ action: { "raise-to": { to } } }) as LineStep;
    expect(lineToken(raise(10 / 3), 3)).toBe("r10c");
    expect(lineToken(raise(2.5), 1000)).toBe("r2.5");
  });
});
