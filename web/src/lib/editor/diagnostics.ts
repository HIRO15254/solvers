// Map the daemon's one error (and a TOML parse error) to editor lines. The
// daemon reports `NLH003: tree.script: line N: ...` for script errors and a
// key path for the rest; it gives no columns.

import type { Diagnostic } from "@codemirror/lint";
import type { Text } from "@codemirror/state";

export interface LineDiagnostic {
  /** 1-based line in the editor's text. */
  line: number;
  /** 1-based column, when the source knows it. */
  column?: number;
  message: string;
  severity: "error" | "warning";
}

const SCRIPT_LINE = /^line (\d+): ([\s\S]*)$/;

/** The script line a `tree.script` issue names, or null for a script error without one. */
export function scriptIssueLine(message: string): { line: number; message: string } | null {
  const match = SCRIPT_LINE.exec(message);
  const line = Number(match?.[1]);
  return match && line > 0 ? { line, message: match[2]! } : null;
}

const HEADER = /^\s*\[\[?\s*([^\]]*?)\s*\]\]?\s*(?:#.*)?$/;
const KEY_PART = String.raw`(?:[A-Za-z0-9_-]+|"(?:[^"\\]|\\.)*"|'[^']*')`;
const KEY = new RegExp(String.raw`^\s*(${KEY_PART}(?:\s*\.\s*${KEY_PART})*)\s*=`);

/** A TOML dotted key as plain segments. */
function segments(key: string): string[] {
  return (key.match(new RegExp(KEY_PART, "g")) ?? []).map((part) => part.replace(/^["']|["']$/g, ""));
}

export interface TomlLines {
  /** 1-based line of each table header and key, by dotted path. */
  keys: Map<string, number>;
  /** 1-based line where the `tree.script` string's text starts. */
  scriptStart: number | null;
}

/** Where each header and key sits in a TOML text, skipping multi-line strings. */
export function tomlLines(text: string): TomlLines {
  const keys = new Map<string, number>();
  let scriptStart: number | null = null;
  let table: string[] = [];
  let delimiter: string | null = null;
  const lines = text.split("\n");
  lines.forEach((raw, index) => {
    const line = raw.replace(/\r$/, "");
    if (delimiter !== null) {
      if (line.includes(delimiter)) delimiter = null;
      return;
    }
    const header = HEADER.exec(line);
    if (header) {
      table = segments(header[1]!);
      keys.set(table.join("."), index + 1);
      return;
    }
    const key = KEY.exec(line);
    if (!key) return;
    const path = [...table, ...segments(key[1]!)].join(".");
    if (!keys.has(path)) keys.set(path, index + 1);
    const rest = line.slice(key[0].length);
    const open = /^\s*('''|""")/.exec(rest);
    if (!open) return;
    const after = rest.slice(open[0].length);
    if (path === "tree.script") scriptStart = after.trim() === "" ? index + 2 : index + 1;
    if (!after.includes(open[1]!)) delimiter = open[1]!;
  });
  return { keys, scriptStart };
}

/** The editor line of a daemon issue in a TOML text: the script line it names, else the deepest key on its path. */
export function tomlIssueLine(text: string, path: string, message: string): number | null {
  const { keys, scriptStart } = tomlLines(text);
  const scriptLine = path === "tree.script" ? scriptIssueLine(message) : null;
  if (scriptLine && scriptStart !== null) return scriptStart + scriptLine.line - 1;
  const parts = path.split(".");
  for (let length = parts.length; length > 0; length -= 1) {
    const line = keys.get(parts.slice(0, length).join("."));
    if (line !== undefined) return line;
  }
  return null;
}

/** CodeMirror diagnostics for line diagnostics, underlining from the column (or the first non-blank) to the line end. */
export function toDiagnostics(doc: Text, list: readonly LineDiagnostic[]): Diagnostic[] {
  return list.flatMap((item) => {
    if (item.line < 1 || item.line > doc.lines) return [];
    const line = doc.line(item.line);
    const indent = /^\s*/.exec(line.text)?.[0].length ?? 0;
    const from = line.from + Math.min(item.column !== undefined ? item.column - 1 : indent, line.length);
    return [{ from, to: Math.max(from, line.to), severity: item.severity, message: item.message }];
  });
}
