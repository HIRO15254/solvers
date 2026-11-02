import { describe, expect, it } from "vitest";
import { readDoc, writeDoc } from "./config";

describe("writeDoc", () => {
  it("writes a multi-line script as a literal and reads it back unchanged", () => {
    const script = 'flop {\n  when donk { remove bet }\n  replace bet [33, "x"]\n}\n';
    const text = writeDoc({ tree: { script } });
    expect(text).toContain("script = '''\nflop {\n");
    expect(readDoc(text)).toEqual({ schema: "solvers.nlh/v1", tree: { script } });
  });

  it("writes CRLF scripts as LF multi-line literals", () => {
    const text = writeDoc({ tree: { script: "flop {\r\n  replace bet [33%]\r\n}\r\n" } });
    expect(text).toContain("script = '''\nflop {\n");
    expect(readDoc(text).tree).toEqual({ script: "flop {\n  replace bet [33%]\n}\n" });
  });

  it("keeps strings a literal cannot hold as quoted strings", () => {
    for (const value of ["a\n'''b", "a\nb'", "SOLVERSLITERAL0N\nb"]) {
      const text = writeDoc({ meta: { name: value }, tree: { script: "x\ny" } });
      expect(readDoc(text)).toEqual({ schema: "solvers.nlh/v1", meta: { name: value }, tree: { script: "x\ny" } });
    }
  });
});
