// CodeMirror languages for the tree script and for TOML whose `script` key
// embeds it. Both are stream tokenizers: they color and indent, while the
// daemon stays the parser of record and reports errors.

import { foldService, indentUnit, StreamLanguage, type StringStream } from "@codemirror/language";
import type { EditorState, Extension } from "@codemirror/state";
import { Tag } from "@lezer/highlight";
import { ACTIONS, CONSTANTS, CONTROL, DECLARATIONS, EFFECTS, isSizeLiteral, STREETS, VARIABLE_BY_NAME } from "./vocabulary";

/** Highlight tag of `fold` ... `raise`. */
export const actionTag = Tag.define();

const KEYWORD_WORDS = new Set<string>([...DECLARATIONS, ...CONTROL, ...STREETS, ...EFFECTS]);
const ACTION_WORDS = new Set<string>(ACTIONS);
const CONSTANT_WORDS = new Set<string>(CONSTANTS);
const WORD = /^[A-Za-z0-9_.%-]+/;
const OPERATOR = /^(?:&&|\|\||[<>=!]=?)/;

export interface ScriptState {
  /** Open `{` blocks at this point. */
  depth: number;
  /** The previous word, to mark a `param` / `define` name as a definition. */
  previous: string;
}

const startScript = (): ScriptState => ({ depth: 0, previous: "" });

/** One script token. `stop`, when set, is a delimiter the token may not run into (the TOML string's end). */
function scriptToken(stream: StringStream, state: ScriptState, stop?: string): string | null {
  if (stream.eatSpace()) return null;
  if (stream.peek() === "#") {
    if (stop) {
      const end = stream.string.indexOf(stop, stream.pos);
      if (end !== -1) {
        stream.pos = end;
        return "lineComment";
      }
    }
    stream.skipToEnd();
    return "lineComment";
  }
  if (stream.eat('"')) {
    while (!stream.eol()) {
      if (stop && stream.match(stop, false)) break;
      if (stream.next() === '"') break;
    }
    return "string";
  }
  if (stream.eat("{")) {
    state.depth += 1;
    state.previous = "{";
    return "brace";
  }
  if (stream.eat("}")) {
    state.depth = Math.max(0, state.depth - 1);
    state.previous = "}";
    return "brace";
  }
  if (stream.eat("[") || stream.eat("]")) return "squareBracket";
  if (stream.eat("(") || stream.eat(")")) return "paren";
  if (stream.eat(",")) return "separator";
  if (stream.match(OPERATOR)) return "operator";
  const word = stream.match(WORD);
  if (word && typeof word !== "boolean") {
    const text = word[0];
    const previous = state.previous;
    state.previous = text;
    if (previous === "param" || previous === "define") return "variableName.definition";
    if (KEYWORD_WORDS.has(text)) return "keyword";
    if (ACTION_WORDS.has(text)) return "action";
    if (CONSTANT_WORDS.has(text)) return "bool";
    if (VARIABLE_BY_NAME.has(text)) return "variableName.standard";
    if (isSizeLiteral(text) || /^-?\d/.test(text)) return "number";
    return "variableName";
  }
  stream.next();
  return "invalid";
}

function scriptIndent(state: ScriptState, textAfter: string, unit: number): number {
  return Math.max(0, state.depth - (/^\s*\}/.test(textAfter) ? 1 : 0)) * unit;
}

const languageData = {
  commentTokens: { line: "#" },
  closeBrackets: { brackets: ["(", "[", "{", '"'] },
  indentOnInput: /^\s*\}$/,
};

const tokenTable = { action: actionTag };

export const scriptLanguage = StreamLanguage.define<ScriptState>({
  name: "tree-script",
  startState: startScript,
  token: (stream, state) => scriptToken(stream, state),
  indent: (state, textAfter, context) => scriptIndent(state, textAfter, context.unit),
  languageData,
  tokenTable,
});

interface TomlState {
  /** The open multi-line string delimiter, if any. */
  delimiter: string | null;
  /** The script inside a `script = '''...'''` string. */
  script: ScriptState | null;
  /** The last key read, so a multi-line string after `script =` is read as script. */
  key: string;
}

const TOML_HEADER = /^\s*\[\[?[^\]]*\]\]?/;
const TOML_PART = String.raw`(?:[A-Za-z0-9_-]+|"(?:[^"\\]|\\.)*"|'[^']*')`;
const TOML_KEY = new RegExp(String.raw`^\s*${TOML_PART}(?:\s*\.\s*${TOML_PART})*(?=\s*=)`);
const TOML_DATE = /^\d{4}-\d{2}-\d{2}(?:[Tt ]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:[Zz]|[+-]\d{2}:\d{2})?)?/;
const TOML_NUMBER = /^[+-]?(?:inf|nan|0x[\da-fA-F_]+|0o[0-7_]+|0b[01_]+|\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?)(?![\w-])/;

function lastKeyPart(key: string): string {
  const part = key.trim().split(".").at(-1)?.trim() ?? "";
  return part.replace(/^["']|["']$/g, "");
}

function tomlToken(stream: StringStream, state: TomlState): string | null {
  if (state.delimiter !== null) {
    if (stream.match(state.delimiter)) {
      state.delimiter = null;
      state.script = null;
      return "string";
    }
    if (state.script) return scriptToken(stream, state.script, state.delimiter);
    const end = stream.string.indexOf(state.delimiter, stream.pos);
    if (end === -1) stream.skipToEnd();
    else stream.pos = end;
    return "string";
  }
  if (stream.sol()) {
    if (stream.match(TOML_HEADER)) return "heading";
    const key = stream.match(TOML_KEY);
    if (key && typeof key !== "boolean") {
      state.key = lastKeyPart(key[0]);
      return "propertyName";
    }
  }
  if (stream.eatSpace()) return null;
  if (stream.peek() === "#") {
    stream.skipToEnd();
    return "lineComment";
  }
  const open = stream.match(/^("""|''')/);
  if (open && typeof open !== "boolean") {
    state.delimiter = open[0];
    state.script = state.key === "script" ? startScript() : null;
    return "string";
  }
  if (stream.match(/^"(?:[^"\\]|\\.)*"?/) || stream.match(/^'[^']*'?/)) return "string";
  if (stream.match(TOML_DATE) || stream.match(TOML_NUMBER)) return "number";
  if (stream.match(/^(?:true|false)(?![\w-])/)) return "bool";
  const word = stream.match(/^[A-Za-z0-9_-]+/);
  if (word && typeof word !== "boolean") {
    // Inside an inline table, a word before `=` is a key.
    if (/^\s*=/.test(stream.string.slice(stream.pos))) {
      state.key = word[0];
      return "propertyName";
    }
    return null;
  }
  if (stream.match(/^[=[\]{},.]/)) return "punctuation";
  stream.next();
  return null;
}

export const tomlLanguage = StreamLanguage.define<TomlState>({
  name: "toml",
  startState: () => ({ delimiter: null, script: null, key: "" }),
  copyState: (state) => ({ ...state, script: state.script ? { ...state.script } : null }),
  token: tomlToken,
  indent: (state, textAfter, context) => (state.script ? scriptIndent(state.script, textAfter, context.unit) : null),
  languageData,
  tokenTable,
});

/** Skip one comment or quoted string at `at`; returns the index after it, or `at` when there is none. */
function skipOpaque(text: string, at: number): number {
  const char = text[at];
  if (char === "#") {
    const end = text.indexOf("\n", at);
    return end === -1 ? text.length : end;
  }
  if (char === '"' || char === "'") {
    // A TOML multi-line delimiter is transparent: the script inside it folds too.
    if (text.startsWith(char.repeat(3), at)) return at + 3;
    let i = at + 1;
    while (i < text.length && text[i] !== char && text[i] !== "\n") i += char === '"' && text[i] === "\\" ? 2 : 1;
    return Math.min(i + 1, text.length);
  }
  return at;
}

/** The `{ ... }` block opened on the line `[from, to)` and closed on a later line, as a fold range. */
export function braceBlock(text: string, from: number, to: number): { from: number; to: number } | null {
  // The outermost `{` opened on this line that is still open, and the depth below it.
  let open: number | null = null;
  let depth = 0;
  let i = from;
  while (i < text.length) {
    if (i >= to && open === null) return null;
    const skipped = skipOpaque(text, i);
    if (skipped !== i) {
      i = skipped;
      continue;
    }
    const char = text[i];
    if (char === "{") {
      if (depth === 0) open = i;
      depth += 1;
    } else if (char === "}" && depth > 0) {
      depth -= 1;
      if (depth === 0) {
        if (i >= to) return { from: open! + 1, to: i };
        open = null;
      }
    }
    i += 1;
  }
  return null;
}

const braceFolding = foldService.of((state: EditorState, lineStart: number, lineEnd: number) =>
  braceBlock(state.doc.toString(), lineStart, lineEnd),
);

export function scriptSupport(): Extension {
  return [scriptLanguage, braceFolding, indentUnit.of("  ")];
}

export function tomlSupport(): Extension {
  return [tomlLanguage, braceFolding, indentUnit.of("  ")];
}

const SCRIPT_OPEN = /(?:^|\n)[ \t]*script[ \t]*=[ \t]*('''|""")/g;

/** Where the tree script inside a TOML text starts and ends, if `pos` lies in it. */
export function scriptRegion(text: string, pos: number): { from: number; to: number } | null {
  for (const match of text.matchAll(SCRIPT_OPEN)) {
    const delimiter = match[1]!;
    let from = match.index + match[0].length;
    if (text[from] === "\n") from += 1;
    else if (text.startsWith("\r\n", from)) from += 2;
    const close = text.indexOf(delimiter, from);
    const to = close === -1 ? text.length : close;
    if (pos >= from && pos <= to) return { from, to };
  }
  return null;
}
