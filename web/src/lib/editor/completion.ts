// Context-aware completion and hover docs for the tree script, standalone or
// inside a TOML `script = '''...'''` string.

import { type Completion, type CompletionContext, type CompletionResult, snippetCompletion } from "@codemirror/autocomplete";
import type { EditorState } from "@codemirror/state";
import { hoverTooltip, type Tooltip } from "@codemirror/view";
import { scriptRegion } from "./languages";
import {
  ACTIONS,
  describeSize,
  KEYWORD_DOCS,
  SIZE_EXAMPLES,
  STREETS,
  textValues,
  VARIABLE_BY_NAME,
  VARIABLES,
  type Variable,
  WAGER_ACTIONS,
  WIDTH_EXAMPLES,
} from "./vocabulary";

const WORD_CHARS = /[A-Za-z0-9_.%-]*$/;
const COMPARISONS: Record<string, true> = { "==": true, "!=": true, "<": true, "<=": true, ">": true, ">=": true };
const CONDITION_STARTS: Record<string, true> = { when: true, if: true, "&&": true, "||": true, "!": true, "(": true };
const LIST_TAKING: Record<string, true> = { add: true, replace: true, force: true, remove: true };
const STREET_WORDS = new Set<string>(STREETS);

/** Significant tokens of a script prefix: words, operators and punctuation; comments dropped, strings kept as `"`. */
export function scriptTokens(text: string): string[] {
  const tokens: string[] = [];
  const pattern = /(#[^\n]*)|("[^"\n]*"?)|(&&|\|\||[<>=!]=?|[{}[\](),])|([A-Za-z0-9_.%-]+)|\s+|./g;
  for (const match of text.matchAll(pattern)) {
    if (match[1] !== undefined) continue;
    if (match[2] !== undefined) tokens.push('"');
    else if (match[3] !== undefined) tokens.push(match[3]);
    else if (match[4] !== undefined) tokens.push(match[4]);
  }
  return tokens;
}

/** `param` and `define` names declared anywhere in the script. */
export function declaredNames(text: string): { params: string[]; defines: string[] } {
  const names = (keyword: string) =>
    [...text.matchAll(new RegExp(String.raw`(?:^|\n)[ \t]*${keyword}[ \t]+([A-Za-z0-9_.%-]+)`, "g"))].map((m) => m[1]!);
  return { params: names("param"), defines: names("define") };
}

const keyword = (label: string, boost = 0): Completion => ({
  label,
  type: "keyword",
  detail: "keyword",
  info: KEYWORD_DOCS[label],
  boost,
});

const action = (label: string): Completion => ({ label, type: "keyword", detail: "action", info: KEYWORD_DOCS[label] });

const variable = (entry: Variable): Completion => ({
  label: entry.name,
  type: "variable",
  detail: entry.board ? `${entry.kind} · board, P1 only` : entry.kind,
  info: entry.doc,
  boost: entry.kind === "bool" ? 1 : 0,
});

const literal = (label: string, info: string, detail = "size"): Completion => ({ label, type: "constant", detail, info });

const STATEMENT_SNIPPETS: readonly Completion[] = [
  snippetCompletion("when ${condition} {\n\t${}\n}", { label: "when … { }", type: "keyword", detail: "block", info: KEYWORD_DOCS.when }),
  snippetCompletion("if ${condition} {\n\t${}\n} else {\n\t${}\n}", {
    label: "if … else …",
    type: "keyword",
    detail: "block",
    info: KEYWORD_DOCS.if,
  }),
  // Numbered fields: CodeMirror reads a bare `${10}` as field number 10, not as the text "10".
  snippetCompletion("replace ${1:bet} [${2:33, 75}]", { label: "replace … [ ]", type: "keyword", detail: "statement", info: KEYWORD_DOCS.replace }),
  snippetCompletion("jam ${1:raise} >= ${2:85%stack}", { label: "jam … >= …", type: "keyword", detail: "statement", info: KEYWORD_DOCS.jam }),
  snippetCompletion("merge ${1:bet} within ${2:10}", { label: "merge … within …", type: "keyword", detail: "statement", info: KEYWORD_DOCS.merge }),
];

const TOP_SNIPPETS: readonly Completion[] = [
  snippetCompletion("${flop} {\n\t${}\n}", { label: "street { }", type: "keyword", detail: "block", info: "Rules for one or more streets." }),
  snippetCompletion("${flop} when ${condition} {\n\t${}\n}", {
    label: "street when … { }",
    type: "keyword",
    detail: "block",
    info: "Rules for the streets where the condition holds.",
  }),
  snippetCompletion("param ${name} = ${value}", { label: "param … = …", type: "keyword", detail: "declaration", info: KEYWORD_DOCS.param }),
  snippetCompletion("define ${name} = ${condition}", { label: "define … = …", type: "keyword", detail: "declaration", info: KEYWORD_DOCS.define }),
];

/** Completions at `pos` of a script text, or null where none apply. */
export function scriptCompletions(text: string, pos: number, explicit: boolean): CompletionResult | null {
  const lineStart = text.lastIndexOf("\n", pos - 1) + 1;
  const lineBefore = text.slice(lineStart, pos);
  // No completion inside a comment or a quoted text literal.
  const lineTokens = lineBefore.replace(/"[^"]*"/g, "");
  if (lineTokens.includes("#") || lineTokens.includes('"')) return null;
  const word = WORD_CHARS.exec(lineBefore)?.[0] ?? "";
  if (word === "" && !explicit) return null;
  const from = pos - word.length;
  const tokens = scriptTokens(text.slice(0, from));
  const [prev3, prev2, prev] = [tokens.at(-3), tokens.at(-2), tokens.at(-1)];
  const { params, defines } = declaredNames(text);
  const paramOptions = params.map((name) => literal(name, "A script param.", "param"));
  const result = (options: readonly Completion[]): CompletionResult | null =>
    options.length ? { from, options: [...options], validFor: /^[A-Za-z0-9_.%-]*$/ } : null;

  const depth = tokens.reduce((sum, token) => sum + (token === "{" ? 1 : token === "}" ? -1 : 0), 0);
  const bracket = tokens.lastIndexOf("[");
  const inList = bracket > tokens.lastIndexOf("]");

  if (prev === "param" || prev === "define" || prev2 === "param" || prev2 === "define") return null;
  if (prev === "=" && prev3 === "param") {
    return result([...SIZE_EXAMPLES.map((size) => literal(size.label, size.doc)), keyword("true"), keyword("false")]);
  }
  const conditionOptions = [
    ...VARIABLES.map(variable),
    ...defines.map((name) => ({ label: name, type: "variable", detail: "define", info: "A named condition." })),
  ];
  if (prev === "=" && prev3 === "define") return result(conditionOptions);

  if (inList) {
    const owner = tokens[bracket - 1];
    if (owner === "in") {
      const name = tokens[bracket - 2] ?? "";
      return result([...textValues(name).map((value) => literal(value, `${name} value`, "text")), ...paramOptions]);
    }
    return result([...SIZE_EXAMPLES.map((size) => literal(size.label, size.doc)), ...paramOptions]);
  }
  if (prev !== undefined && Object.hasOwn(LIST_TAKING, prev)) return result(ACTIONS.map(action));
  if (prev === "jam" || prev === "merge") return result(WAGER_ACTIONS.map(action));
  if (prev2 === "jam" && (prev === "bet" || prev === "raise")) {
    return result([literal(">=", "At or above the threshold.", "bound"), literal(">", "Above the threshold.", "bound")]);
  }
  if (prev3 === "jam" && (prev === ">=" || prev === ">")) {
    return result([...SIZE_EXAMPLES.map((size) => literal(size.label, size.doc)), ...paramOptions]);
  }
  if (prev2 === "merge" && (prev === "bet" || prev === "raise")) return result([keyword("within")]);
  if (prev3 === "merge" && prev === "within") {
    return result([...WIDTH_EXAMPLES.map((width) => literal(width.label, width.doc, "width")), ...paramOptions]);
  }
  if (prev !== undefined && Object.hasOwn(CONDITION_STARTS, prev)) return result(conditionOptions);
  if (prev !== undefined && Object.hasOwn(COMPARISONS, prev) && prev2 !== undefined) {
    const target = VARIABLE_BY_NAME.get(prev2);
    if (target?.kind === "bool") return result([keyword("true"), keyword("false")]);
    if (target?.kind === "text") return result(textValues(target.name).map((value) => literal(value, `${target.name} value`, "text")));
    return result(paramOptions);
  }
  const variableBefore = prev === undefined ? undefined : VARIABLE_BY_NAME.get(prev);
  if (variableBefore && variableBefore.kind !== "bool") return result([keyword("in")]);

  const firstOnLine = lineBefore.slice(0, lineBefore.length - word.length).trim() === "";
  const statementStart =
    prev === undefined ||
    prev === "{" ||
    prev === "}" ||
    prev === "]" ||
    prev === "checkdown" ||
    (prev2 === "remove" && prev !== undefined && (ACTIONS as readonly string[]).includes(prev)) ||
    firstOnLine;
  if (depth <= 0 && (prev === "," || (prev !== undefined && STREET_WORDS.has(prev)))) {
    return result([...STREETS.map((street) => keyword(street)), ...(prev === "," ? [] : [keyword("when", 1)])]);
  }
  if (statementStart && depth <= 0) {
    return result([...STREETS.map((street) => keyword(street, 1)), keyword("param"), keyword("define"), ...TOP_SNIPPETS]);
  }
  if (statementStart) {
    return result([
      ...["add", "remove", "replace", "force", "jam", "merge", "checkdown", "when", "if"].map((name) => keyword(name, 1)),
      ...(prev === "}" ? [keyword("else", 2)] : []),
      ...STATEMENT_SNIPPETS,
    ]);
  }
  if (!explicit) return null;
  return result([...Object.keys(KEYWORD_DOCS).map((name) => keyword(name)), ...conditionOptions]);
}

/** The script text a completion or hover at `pos` works on, with its offset in the document. */
function scriptAt(state: EditorState, pos: number, embedded: boolean): { text: string; offset: number } | null {
  const doc = state.doc.toString();
  if (!embedded) return { text: doc, offset: 0 };
  const region = scriptRegion(doc, pos);
  return region ? { text: doc.slice(region.from, region.to), offset: region.from } : null;
}

/** A completion source for the script language (`embedded`: inside a TOML `script` string). */
export function scriptCompletionSource(embedded: boolean) {
  return (context: CompletionContext): CompletionResult | null => {
    const script = scriptAt(context.state, context.pos, embedded);
    if (!script) return null;
    const result = scriptCompletions(script.text, context.pos - script.offset, context.explicit);
    return result && { ...result, from: result.from + script.offset };
  };
}

/** The hover text for `word` in `script`, or null. */
export function describeWord(script: string, word: string): string | null {
  const known = VARIABLE_BY_NAME.get(word);
  if (known) return `${known.kind}${known.board ? ", board (P1 only)" : ""}. ${known.doc}`;
  if (word in KEYWORD_DOCS) return KEYWORD_DOCS[word] ?? null;
  const size = describeSize(word);
  if (size) return size;
  const declaration = new RegExp(String.raw`((?:^|\n)(?:[ \t]*#[^\n]*\n)*)[ \t]*(param|define)[ \t]+${word.replace(/[.%-]/g, "\\$&")}[ \t]*=[ \t]*([^\n#]*)`).exec(
    script,
  );
  if (!declaration) return null;
  const comments = declaration[1]!
    .split("\n")
    .map((line) => line.replace(/^\s*#\s?/, "").trim())
    .filter(Boolean)
    .join(" ");
  const head = `${declaration[2]} ${word} = ${declaration[3]!.trim()}`;
  return comments ? `${head}. ${comments}` : head;
}

/** Hover docs for words in the script (`embedded`: inside a TOML `script` string). */
export function scriptHover(embedded: boolean) {
  return hoverTooltip((view, pos): Tooltip | null => {
    const script = scriptAt(view.state, pos, embedded);
    if (!script) return null;
    const at = pos - script.offset;
    const before = /[A-Za-z0-9_.%-]*$/.exec(script.text.slice(0, at))?.[0] ?? "";
    const after = /^[A-Za-z0-9_.%-]*/.exec(script.text.slice(at))?.[0] ?? "";
    const word = before + after;
    if (!word) return null;
    const text = describeWord(script.text, word);
    if (!text) return null;
    return {
      pos: pos - before.length,
      end: pos + after.length,
      above: true,
      create: () => {
        const dom = document.createElement("div");
        dom.className = "cm-doc-tooltip";
        const name = document.createElement("code");
        name.textContent = word;
        dom.append(name, document.createTextNode(` ${text}`));
        return { dom };
      },
    };
  });
}
