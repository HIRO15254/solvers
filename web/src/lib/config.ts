// The Setup form edits a `solvers.nlh/v1` document as a TOML value tree.
// Keys the form has no field for are kept as they are, so opening a config
// and solving it never drops part of it; defaults and validation belong to
// `solvers validate` through the daemon (docs/app-architecture.md R4).

import { parse, stringify, type TomlPrimitive, type TomlTable } from "smol-toml";

export type Doc = TomlTable;
export type Path = readonly string[];

export const SCHEMA = "solvers.nlh/v1";

export function readDoc(text: string): Doc {
  return parse(text);
}

/** A string TOML can hold as a multi-line literal: no `'''`, no trailing `'`, no control character but tab and LF. */
const MULTILINE_LITERAL = /^(?![\s\S]*''')(?![\s\S]*'$)[^\x00-\x08\x0b-\x1f\x7f]*$/;

/**
 * TOML text for `doc`. smol-toml writes every string on one quoted line; a
 * string with line breaks (a tree script) goes out as a placeholder instead
 * and comes back as a `'''` literal, the way the examples write scripts.
 */
export function writeDoc(doc: Doc): string {
  const { schema, ...rest } = doc;
  const literals: string[] = [];
  let mark = "SOLVERSLITERAL";
  while (JSON.stringify(doc).includes(mark)) mark += "X";
  const swap = (value: unknown): unknown => {
    if (typeof value === "string") {
      if (!value.includes("\n") || !MULTILINE_LITERAL.test(value)) return value;
      literals.push(value);
      return `${mark}${literals.length - 1}N`;
    }
    if (Array.isArray(value)) return value.map(swap);
    if (typeof value !== "object" || value === null || value instanceof Date) return value;
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, swap(item)]));
  };
  const text = stringify(swap({ schema: schema ?? SCHEMA, ...rest }) as Doc);
  return literals.reduce((out, literal, i) => out.replace(`"${mark}${i}N"`, () => `'''\n${literal}'''`), text);
}

export function getAt(doc: Doc, path: Path): unknown {
  let node: unknown = doc;
  for (const key of path) {
    if (typeof node !== "object" || node === null || Array.isArray(node)) return undefined;
    node = (node as TomlTable)[key];
  }
  return node;
}

/**
 * Returns a copy of `doc` with `path` set to `value`. `undefined` removes the
 * key, and tables left empty by the removal go with it, so clearing a field
 * falls back to the default instead of writing an empty table.
 */
export function setAt(doc: Doc, path: Path, value: TomlPrimitive | undefined): Doc {
  const [key, ...rest] = path;
  if (key === undefined) return doc;
  const copy: Doc = { ...doc };
  if (rest.length === 0) {
    if (value === undefined) delete copy[key];
    else copy[key] = value;
    return copy;
  }
  const child = copy[key];
  const table = typeof child === "object" && child !== null && !Array.isArray(child) ? (child as Doc) : {};
  const next = setAt(table, rest, value);
  if (Object.keys(next).length === 0) delete copy[key];
  else copy[key] = next;
  return copy;
}

/**
 * Reads a form field: empty is "not written", a decimal number is a number,
 * anything else stays text for the validator to judge.
 */
export function fieldValue(text: string): string | number | undefined {
  const trimmed = text.trim();
  if (trimmed === "") return undefined;
  return /^-?\d+(\.\d+)?$/.test(trimmed) ? Number(trimmed) : trimmed;
}

/** Reads a comma-separated list field (`100, 60, 0`). */
export function listValue(text: string): (string | number)[] | undefined {
  const items = text
    .split(",")
    .map((item) => fieldValue(item))
    .filter((item) => item !== undefined);
  return items.length === 0 ? undefined : items;
}

export function fieldText(value: unknown): string {
  if (value === undefined || value === null) return "";
  if (Array.isArray(value)) return value.map(fieldText).join(", ");
  return String(value);
}

/** Example P1 configs shipped with the repository, offered as templates. */
const EXAMPLES = import.meta.glob("../../../examples/hu-postflop/*.toml", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

export interface Template {
  name: string;
  text: string;
}

// `river_script` loads an external tree file, which a self-contained daemon
// config cannot carry (R10), so it is not offered.
export const TEMPLATES: Template[] = Object.entries(EXAMPLES)
  .map(([path, text]) => ({ name: path.replace(/^.*\/(.*)\.toml$/, "$1"), text }))
  .filter((template) => !/^\s*source\s*=/m.test(template.text))
  .sort((a, b) => a.name.localeCompare(b.name));
