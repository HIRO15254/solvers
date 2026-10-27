import { Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { scriptIssueLine, toDiagnostics, tomlIssueLine } from "./diagnostics";

describe("scriptIssueLine", () => {
  it("reads the 1-based line and keeps the rest as the message", () => {
    expect(scriptIssueLine("line 3: unknown identifier \"foo\"")).toEqual({ line: 3, message: 'unknown identifier "foo"' });
    expect(scriptIssueLine("script contains a control character")).toBeNull();
    expect(scriptIssueLine("line 0: override does not name a declared param")).toBeNull();
  });
});

describe("tomlIssueLine", () => {
  const toml = [
    "schema = 'solvers.nlh/v1'", // 1
    "[table.stacks_bb]", // 2
    "CO = 80", // 3
    "[ranges]", // 4
    "BTN = 'AA'", // 5
    "[tree]", // 6
    "script = '''", // 7
    "flop {", // 8
    "  replace bet [33]", // 9
    "}", // 10
    "'''", // 11
    "include_allin = false", // 12
  ].join("\n");

  it("puts a script error on its line inside the multi-line string", () => {
    expect(tomlIssueLine(toml, "tree.script", "line 2: expected one of: 'bet'")).toBe(9);
  });

  it("counts from the opener's line when the script starts on it", () => {
    expect(tomlIssueLine("[tree]\nscript = '''flop {\nbad\n}'''", "tree.script", "line 2: x")).toBe(3);
  });

  it("finds keys under tables and falls back to the deepest key on the path", () => {
    expect(tomlIssueLine(toml, "ranges.BTN", "invalid range")).toBe(5);
    expect(tomlIssueLine(toml, "table.stacks_bb.CO", "must be positive")).toBe(3);
    expect(tomlIssueLine(toml, "tree.allin_threshold", "unknown key")).toBe(6);
    expect(tomlIssueLine(toml, "tree.include_allin", "expected a bool")).toBe(12);
    expect(tomlIssueLine(toml, "spot.board", "missing")).toBeNull();
  });

  it("does not read keys inside a multi-line string", () => {
    const text = "[meta]\nnote = '''\nboard = 1\n'''\n[spot]\nboard = 'Ks 7h 2d'";
    expect(tomlIssueLine(text, "spot.board", "bad board")).toBe(6);
  });
});

describe("toDiagnostics", () => {
  const doc = Text.of(["flop {", "  replace bet [33]", "}"]);

  it("underlines from the first non-blank (or the column) to the line end and drops lines past the end", () => {
    const [first, second, ...rest] = toDiagnostics(doc, [
      { line: 2, message: "a", severity: "error" },
      { line: 2, column: 11, message: "b", severity: "warning" },
      { line: 9, message: "c", severity: "error" },
    ]);
    expect(rest).toEqual([]);
    expect(doc.sliceString(first!.from, first!.to)).toBe("replace bet [33]");
    expect(doc.sliceString(second!.from, second!.to)).toBe("bet [33]");
    expect(second!.severity).toBe("warning");
  });
});
