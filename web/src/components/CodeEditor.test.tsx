import { isValidElement, type ReactNode } from "react";
import { describe, expect, it } from "vitest";
import { highlightToml } from "./CodeEditor";

/** Each line as `[class, text]` pairs; plain text has class "". */
function classes(text: string) {
  return highlightToml(text).map((line) =>
    line.map((node: ReactNode) =>
      isValidElement<{ className: string; children: string }>(node)
        ? [node.props.className, node.props.children]
        : ["", String(node)],
    ),
  );
}

describe("highlightToml", () => {
  it("keeps a multi-line string open across lines and resumes after it closes", () => {
    const lines = classes('script = """\nflop { # not a comment\n}""" # comment\nx = 1');
    expect(lines[0]).toEqual([
      ["tok-var", "script"],
      ["", " "],
      ["tok-punct", "="],
      ["", " "],
      ["tok-str", '"""'],
    ]);
    expect(lines[1]).toEqual([["tok-str", "flop { # not a comment"]]);
    expect(lines[2]).toEqual([
      ["tok-str", '}"""'],
      ["", " "],
      ["tok-com", "# comment"],
    ]);
    expect(lines[3]?.[0]).toEqual(["tok-var", "x"]);
  });

  it("reads headers, dotted and quoted keys, and value kinds", () => {
    const lines = classes('[economics.rake]\na."b c".d = 0.05\nok = true # x\nCO = "80"');
    expect(lines[0]).toEqual([["tok-kw", "[economics.rake]"]]);
    expect(lines[1]?.[0]).toEqual(["tok-var", 'a."b c".d']);
    expect(lines[1]?.at(-1)).toEqual(["tok-num", "0.05"]);
    expect(lines[2]?.filter(([c]) => c !== "" && c !== "tok-punct")).toEqual([
      ["tok-var", "ok"],
      ["tok-num", "true"],
      ["tok-com", "# x"],
    ]);
    expect(lines[3]?.at(-1)).toEqual(["tok-str", '"80"']);
  });
});
