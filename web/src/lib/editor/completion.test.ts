import { CompletionContext } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { describeWord, scriptCompletions, scriptCompletionSource } from "./completion";

/** Labels offered with the cursor at `|`. */
function labels(source: string, explicit = true): string[] | null {
  const pos = source.indexOf("|");
  const text = source.slice(0, pos) + source.slice(pos + 1);
  return scriptCompletions(text, pos, explicit)?.options.map((option) => option.label) ?? null;
}

describe("scriptCompletions", () => {
  it("offers streets and declarations at the top level and statements inside a block", () => {
    expect(labels("|")).toEqual(expect.arrayContaining(["flop", "param", "define", "street { }"]));
    expect(labels("flop {\n  |\n}")).toEqual(expect.arrayContaining(["replace", "jam", "merge", "checkdown", "when", "if"]));
    expect(labels("flop {\n  |\n}")).not.toContain("flop");
    expect(labels("flop { if cbet { checkdown } |}")).toContain("else");
  });

  it("follows a statement's grammar: actions, the jam bound, the merge keyword and their sizes", () => {
    expect(labels("flop { replace |")).toEqual(["fold", "check", "call", "bet", "raise"]);
    expect(labels("flop { jam |")).toEqual(["bet", "raise"]);
    expect(labels("flop { jam raise |")).toEqual([">=", ">"]);
    expect(labels("param t = 85%stack\nflop { jam raise >= |")).toEqual(expect.arrayContaining(["85%stack", "a", "t"]));
    expect(labels("flop { merge bet |")).toEqual(["within"]);
    expect(labels("flop { merge bet within |")).toEqual(["10", "0.5bb", "5%stack", "5%effective"]);
    expect(labels("param cb = 33\nflop { replace bet [50, |")).toEqual(expect.arrayContaining(["33", "e", "min", "cb"]));
  });

  it("offers variables and defines in conditions, and values after a comparison", () => {
    const condition = labels("define wet = flush_possible\nflop when |");
    expect(condition).toEqual(expect.arrayContaining(["cbet", "spr", "paired", "wet"]));
    expect(labels("flop when position == |")).toEqual(["UTG", "UTG1", "UTG2", "LJ", "HJ", "CO", "BTN", "SB", "BB"]);
    expect(labels("flop when position in [BTN, |")).toContain("CO");
    expect(labels("flop when paired == |")).toEqual(["true", "false"]);
    expect(labels("flop when cbet && |")).toContain("donk");
  });

  it("filters by the word being typed and stays quiet in comments, strings and empty unrequested spots", () => {
    const result = scriptCompletions("flop { mer", 10, false);
    expect(result?.from).toBe(7);
    expect(labels("flop { # rep|")).toBeNull();
    expect(labels('flop when position == "B|')).toBeNull();
    expect(labels("flop { |", false)).toBeNull();
  });
});

describe("scriptCompletionSource in TOML", () => {
  const complete = (text: string) => {
    const pos = text.indexOf("|");
    const doc = text.slice(0, pos) + text.slice(pos + 1);
    const state = EditorState.create({ doc });
    return scriptCompletionSource(true)(new CompletionContext(state, pos, true));
  };

  it("completes only inside the script string, at document positions", () => {
    const result = complete("[tree]\nscript = '''\nflop { mer|\n'''\n");
    expect(result?.from).toBe("[tree]\nscript = '''\nflop { ".length);
    expect(result?.options.map((option) => option.label)).toContain("merge");
    expect(complete("[tree]\ninclude_|allin = false")).toBeNull();
  });
});

describe("describeWord", () => {
  const script = "# The c-bet size.\n# Half pot.\nparam cb = 50\ndefine wet = flush_possible\nflop { replace bet [cb] }";

  it("explains variables, keywords, sizes and the script's own declarations", () => {
    expect(describeWord(script, "merge")).toMatch(/within DIFF/);
    expect(describeWord(script, "85%stack")).toMatch(/85% of the actor's maximum/);
    expect(describeWord(script, "cb")).toBe("param cb = 50. The c-bet size. Half pot.");
    expect(describeWord(script, "wet")).toBe("define wet = flush_possible");
    expect(describeWord(script, "unknown")).toBeNull();
  });
});
