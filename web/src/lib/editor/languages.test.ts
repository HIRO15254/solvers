import { ensureSyntaxTree, getIndentation } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { highlightTree, tagHighlighter, tags as t } from "@lezer/highlight";
import { describe, expect, it } from "vitest";
import { actionTag, braceBlock, scriptRegion, scriptSupport, tomlSupport } from "./languages";

const classes = tagHighlighter([
  { tag: t.keyword, class: "kw" },
  { tag: t.heading, class: "head" },
  { tag: actionTag, class: "act" },
  { tag: [t.number, t.bool], class: "num" },
  { tag: t.definition(t.variableName), class: "def" },
  { tag: t.variableName, class: "var" },
  { tag: t.propertyName, class: "key" },
  { tag: t.string, class: "str" },
  { tag: t.lineComment, class: "com" },
  { tag: t.invalid, class: "bad" },
]);

/** Each highlighted token as `text:class`, in order. */
function tokens(text: string, language: "script" | "toml"): string[] {
  const state = EditorState.create({ doc: text, extensions: language === "script" ? scriptSupport() : tomlSupport() });
  const tree = ensureSyntaxTree(state, text.length, 5000)!;
  const out: string[] = [];
  highlightTree(tree, classes, (from, to, cls) => out.push(`${text.slice(from, to)}:${cls}`));
  return out;
}

describe("script highlighting", () => {
  it("tells keywords, actions, variables, sizes, definitions and comments apart", () => {
    expect(
      tokens('param cb = 33\nflop when cbet && position == "BTN" { merge bet within 10 jam raise >= 85%stack } # end', "script"),
    ).toEqual([
      "param:kw",
      "cb:def",
      "33:num",
      "flop:kw",
      "when:kw",
      "cbet:var",
      "position:var",
      '"BTN":str',
      "merge:kw",
      "bet:act",
      "within:kw",
      "10:num",
      "jam:kw",
      "raise:act",
      "85%stack:num",
      "# end:com",
    ]);
  });

  it("indents inside blocks and dedents a closing brace", () => {
    const text = "flop {\n  when cbet {\nx\n}\n}";
    const state = EditorState.create({ doc: text, extensions: scriptSupport() });
    ensureSyntaxTree(state, text.length, 5000);
    expect(getIndentation(state, state.doc.line(2).from)).toBe(2);
    expect(getIndentation(state, state.doc.line(3).from)).toBe(4);
    expect(getIndentation(state, state.doc.line(4).from)).toBe(2);
    expect(getIndentation(state, state.doc.line(5).from)).toBe(0);
  });
});

describe("TOML highlighting", () => {
  it("reads a multi-line script string as script and resumes TOML after it closes", () => {
    const text = 'script = """\nflop { checkdown } # rule\n}""" # comment\nx = 1';
    expect(tokens(text, "toml")).toEqual([
      "script:key",
      '""":str',
      "flop:kw",
      "checkdown:kw",
      "# rule:com",
      '""":str',
      "# comment:com",
      "x:key",
      "1:num",
    ]);
  });

  it("keeps other multi-line strings plain and reads headers, keys and values", () => {
    const text = "[meta]\nnote = '''\nflop {\n'''\n[economics.rake]\na.\"b c\".d = 0.05\nok = true";
    expect(tokens(text, "toml")).toEqual([
      "[meta]:head",
      "note:key",
      "''':str",
      "flop {:str",
      "''':str",
      "[economics.rake]:head",
      'a."b c".d:key',
      "0.05:num",
      "ok:key",
      "true:num",
    ]);
  });
});

describe("braceBlock", () => {
  const text = "flop { # {\n  when cbet {\n    checkdown\n  }\n}\nturn { checkdown }\n";
  const line = (n: number) => {
    const lines = text.split("\n");
    const from = lines.slice(0, n - 1).join("\n").length + (n > 1 ? 1 : 0);
    return [from, from + lines[n - 1]!.length] as const;
  };

  it("folds from an opening brace to its matching close on a later line, ignoring comments", () => {
    const outer = braceBlock(text, ...line(1));
    expect(outer && text.slice(outer.from, outer.to)).toBe(" # {\n  when cbet {\n    checkdown\n  }\n");
    const inner = braceBlock(text, ...line(2));
    expect(inner && text.slice(inner.from, inner.to)).toBe("\n    checkdown\n  ");
  });

  it("does not fold a block closed on its own line or a line without a brace", () => {
    expect(braceBlock(text, ...line(6))).toBeNull();
    expect(braceBlock(text, ...line(3))).toBeNull();
  });
});

describe("scriptRegion", () => {
  const toml = "[tree]\nscript = '''\nflop { checkdown }\n'''\n[tree.params]\ncb = 40";

  it("covers the script text between the delimiters, after the opening newline", () => {
    const at = toml.indexOf("checkdown");
    const region = scriptRegion(toml, at);
    expect(region && toml.slice(region.from, region.to)).toBe("flop { checkdown }\n");
  });

  it("is null outside the script", () => {
    expect(scriptRegion(toml, toml.indexOf("cb = 40"))).toBeNull();
  });
});
