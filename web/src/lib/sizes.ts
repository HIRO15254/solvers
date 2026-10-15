// The Tree step's size table: bet and raise sizes by street and role, written
// to `tree.script` in the script grammar (docs/nlh-input-v1.jp.md §9). The
// layout follows the bet-size tables of PioSolver and GTO Wizard's custom
// solutions (one row per street, OOP and IP columns). Cells hold the
// grammar's size literals; the daemon's `validate` judges them. A script
// the table cannot write back exactly reads as `null` and stays a script.

export type SizeStreet = "flop" | "turn" | "river";
export type SizeRole = "oop" | "ip";
export type SizeKind = "bet" | "raise";

export const SIZE_STREETS: SizeStreet[] = ["flop", "turn", "river"];
export const SIZE_ROLES: SizeRole[] = ["oop", "ip"];
export const SIZE_KINDS: SizeKind[] = ["bet", "raise"];

export type StreetSizes = Record<SizeRole, Record<SizeKind, string[]>>;

export interface SizeTable {
  sizes: Record<SizeStreet, StreetSizes>;
  /** OOP does not lead into the last aggressor of the previous street (`donk`). */
  noDonk: boolean;
  /** A raise facing a bet and a raise is all-in only. */
  reraiseAllIn: boolean;
}

export const DEFAULT_SIZES: SizeTable = {
  sizes: {
    flop: {
      oop: { bet: ["33"], raise: ["50"] },
      ip: { bet: ["33"], raise: ["50"] },
    },
    turn: {
      oop: { bet: ["75"], raise: ["a"] },
      ip: { bet: ["75"], raise: ["a"] },
    },
    river: {
      oop: { bet: ["75", "a"], raise: ["a"] },
      ip: { bet: ["75", "a"], raise: ["a"] },
    },
  },
  noDonk: true,
  reraiseAllIn: true,
};

/** A cell's literal: the grammar's bare-token characters, so a typo cannot break the script around it. */
export const SIZE_LITERAL = /^[A-Za-z0-9_.%]+$/;

/** The literals typed in a cell, separated by commas or spaces. */
export function sizeList(text: string): string[] {
  return text.split(/[\s,]+/).filter(Boolean);
}

const HEADER = "# Written by the Setup size table";
const WHEN: Record<SizeRole, string> = {
  oop: "!in_position",
  ip: "in_position",
};
const NO_DONK = "!in_position && !donk";
const RERAISE = "when aggressions >= 2 { replace raise [a] }";
const RULE = /^when (!in_position && !donk|!in_position|in_position) \{ replace (bet|raise) \[([^\]]*)\] \}$/;
const STREET: Record<string, SizeStreet> = {
  flop: "flop",
  turn: "turn",
  river: "river",
};
const KIND: Record<string, SizeKind> = { bet: "bet", raise: "raise" };

/** Whether a street can reach a raise facing a raise: one role bets and the other raises. */
function reraises(street: StreetSizes): boolean {
  return (
    (street.ip.bet.length > 0 && street.oop.raise.length > 0) ||
    (street.oop.bet.length > 0 && street.ip.raise.length > 0)
  );
}

export function anyReraise(table: SizeTable): boolean {
  return SIZE_STREETS.some((street) => reraises(table.sizes[street]));
}

export function anyOopBet(table: SizeTable): boolean {
  return SIZE_STREETS.some((street) => table.sizes[street].oop.bet.length > 0);
}

/**
 * The script for `table`: per street, each role's bets and raises, the
 * re-raise rule where a re-raise can happen. Without donk bets the OOP bet
 * rule also requires `!donk`, so a probe after a checked-through street
 * stays and no rule is left unmatched. An empty table is the empty script.
 */
export function sizeScript(table: SizeTable): string {
  const blocks = SIZE_STREETS.flatMap((name) => {
    const street = table.sizes[name];
    const rules = SIZE_KINDS.flatMap((kind) =>
      SIZE_ROLES.filter((role) => street[role][kind].length > 0).map((role) => {
        const when = kind === "bet" && role === "oop" && table.noDonk ? NO_DONK : WHEN[role];
        return `  when ${when} { replace ${kind} [${street[role][kind].join(", ")}] }`;
      }),
    );
    if (table.reraiseAllIn && reraises(street)) rules.push(`  ${RERAISE}`);
    return rules.length === 0 ? [] : [`${name} {`, ...rules, "}"];
  });
  return blocks.length === 0 ? "" : [HEADER, ...blocks, ""].join("\n");
}

const meaningful = (script: string) =>
  script
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== "" && !line.startsWith("#"))
    .join("\n");

export interface SizeCell {
  street: SizeStreet;
  role: SizeRole;
  kind: SizeKind;
}

/**
 * The cell of a table-written `script` that wrote its line `line` (1-based,
 * as the validator counts), or `null` when no cell wrote that line.
 */
export function sizeCellAt(script: string, line: number): SizeCell | null {
  const lines = script.split(/\r?\n/);
  let street: SizeStreet | undefined;
  for (const [i, raw] of lines.slice(0, line).entries()) {
    const text = raw.trim();
    const opened = /^(\w+) \{$/.exec(text);
    if (opened) street = STREET[opened[1]!];
    if (i < line - 1) continue;
    const rule = RULE.exec(text);
    if (!rule || !street) return null;
    return {
      street,
      role: rule[1] === "in_position" ? "ip" : "oop",
      kind: KIND[rule[2]!]!,
    };
  }
  return null;
}

/**
 * The table `script` was written from, or `null` when the table cannot
 * write it back rule for rule (comments and indentation aside).
 */
export function readSizeScript(script: string): SizeTable | null {
  const sizes: Record<SizeStreet, StreetSizes> = {
    flop: { oop: { bet: [], raise: [] }, ip: { bet: [], raise: [] } },
    turn: { oop: { bet: [], raise: [] }, ip: { bet: [], raise: [] } },
    river: { oop: { bet: [], raise: [] }, ip: { bet: [], raise: [] } },
  };
  let noDonk: boolean | null = null;
  let reraise = false;
  let street: SizeStreet | null = null;
  for (const line of meaningful(script).split("\n")) {
    if (line === "") continue;
    if (street === null) {
      street = STREET[/^(\w+) \{$/.exec(line)?.[1] ?? ""] ?? null;
      if (street === null) return null;
    } else if (line === "}") {
      street = null;
    } else if (line === RERAISE) {
      reraise = true;
    } else {
      const [, when, kindName, list] = RULE.exec(line) ?? [];
      const kind = KIND[kindName ?? ""];
      if (!when || !kind || list === undefined) return null;
      const role: SizeRole = when === "in_position" ? "ip" : "oop";
      if (role === "oop" && kind === "bet") noDonk = when === NO_DONK;
      const items = list.split(",").map((size) => size.trim());
      if (!items.every((size) => SIZE_LITERAL.test(size))) return null;
      sizes[street][role][kind] = items;
    }
  }
  if (street !== null) return null;
  const read = {
    sizes,
    noDonk: noDonk ?? true,
    reraiseAllIn: reraise || !anyReraise({ sizes, noDonk: true, reraiseAllIn: false }),
  };
  return meaningful(sizeScript(read)) === meaningful(script) ? read : null;
}
