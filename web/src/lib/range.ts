// Range strings of `solvers.nlh/v1` (docs/nlh-input-v1.jp.md §8), for the
// editor's grid and counts. The daemon's `validate` stays the authority: a
// string this module accepts can still be rejected there, and the GUI shows
// that error rather than this module's opinion.

import { RANKS, SUITS } from "./cards";

/** Combo key: higher rank first, then suits in `SUITS` order (`AhKs`, `QsQh`). */
export type Combo = string;
/** Combo weights; a combo absent from the map has weight 0. */
export type Weights = Map<Combo, number>;

export interface ParsedRange {
  weights: Weights;
  errors: string[];
}

function comboKey(r1: number, s1: number, r2: number, s2: number): Combo {
  // Lower index = higher rank; within a pair, suit order decides.
  if (r1 > r2 || (r1 === r2 && s1 > s2)) [r1, s1, r2, s2] = [r2, s2, r1, s1];
  return `${RANKS[r1]}${SUITS[s1]}${RANKS[r2]}${SUITS[s2]}`;
}

/** Name of the 13×13 cell at row `i`, column `j` (pairs on the diagonal, suited above). */
export function className(i: number, j: number): string {
  if (i === j) return `${RANKS[i]}${RANKS[i]}`;
  return i < j ? `${RANKS[i]}${RANKS[j]}s` : `${RANKS[j]}${RANKS[i]}o`;
}

/** Class (`AKs`, `AKo`, `AA`) of a combo key or a concrete combo label. */
export function classOf(combo: string): string {
  const r1 = RANKS.indexOf(combo[0]!.toUpperCase());
  const r2 = RANKS.indexOf(combo[2]!.toUpperCase());
  const [hi, lo] = r1 <= r2 ? [r1, r2] : [r2, r1];
  if (hi === lo) return className(hi, hi);
  return combo[1] === combo[3] ? className(hi, lo) : className(lo, hi);
}

/** The concrete combos of a class, in a fixed order. */
export function classCombos(name: string): Combo[] {
  const hi = RANKS.indexOf(name[0]!);
  const lo = RANKS.indexOf(name[1]!);
  const combos: Combo[] = [];
  for (let a = 0; a < 4; a++) {
    for (let b = 0; b < 4; b++) {
      if (hi === lo ? a < b : name[2] === "s" ? a === b : a !== b) combos.push(comboKey(hi, a, lo, b));
    }
  }
  return combos;
}

/** Every class name in grid order (row-major). */
export const ALL_CLASSES: string[] = RANKS.split("").flatMap((_, i) => RANKS.split("").map((__, j) => className(i, j)));

const RANK = "[AKQJT2-9]";
const COMBO = new RegExp(`^(${RANK})([shdc])(${RANK})([shdc])$`, "i");
const SINGLE = new RegExp(`^(${RANK})(${RANK})([so]?)(\\+?)$`, "i");
const SPAN = new RegExp(`^(${RANK})(${RANK})([so]?)-(${RANK})(${RANK})([so]?)$`, "i");
const WEIGHT = /^\d+(\.\d+)?$/;

/** One class described by rank indexes (`hi <= lo`) and suitedness (`""` = both). */
interface ClassRef {
  hi: number;
  lo: number;
  suit: string;
}

function classRef(a: string, b: string, suit: string): ClassRef | string {
  const r1 = RANKS.indexOf(a.toUpperCase());
  const r2 = RANKS.indexOf(b.toUpperCase());
  const s = suit.toLowerCase();
  if (r1 === r2 && s) return "Pairs cannot use s/o";
  return { hi: Math.min(r1, r2), lo: Math.max(r1, r2), suit: s };
}

function classCombosOf(ref: ClassRef): Combo[] {
  if (ref.hi === ref.lo) return classCombos(className(ref.hi, ref.hi));
  const suited = classCombos(className(ref.hi, ref.lo));
  const offsuit = classCombos(className(ref.lo, ref.hi));
  return ref.suit === "s" ? suited : ref.suit === "o" ? offsuit : [...suited, ...offsuit];
}

/** Expands one entry (without its weight) into classes, or explains why it is invalid. */
function expand(entry: string): ClassRef[] | string {
  const single = SINGLE.exec(entry);
  if (single) {
    const ref = classRef(single[1]!, single[2]!, single[3]!);
    if (typeof ref === "string") return ref;
    if (!single[4]) return [ref];
    const refs: ClassRef[] = [];
    if (ref.hi === ref.lo) for (let r = ref.hi; r >= 0; r--) refs.push({ hi: r, lo: r, suit: "" });
    else for (let k = ref.lo; k > ref.hi; k--) refs.push({ ...ref, lo: k });
    return refs;
  }
  const span = SPAN.exec(entry);
  if (!span) return "Cannot parse entry";
  const from = classRef(span[1]!, span[2]!, span[3]!);
  const to = classRef(span[4]!, span[5]!, span[6]!);
  if (typeof from === "string") return from;
  if (typeof to === "string") return to;
  if (from.suit !== to.suit) return "Both ends of an interval must use the same s/o suffix";
  const refs: ClassRef[] = [];
  const fromPair = from.hi === from.lo;
  if (fromPair !== (to.hi === to.lo)) return "An interval cannot mix pairs and non-pairs";
  if (fromPair) {
    for (let r = Math.min(from.hi, to.hi); r <= Math.max(from.hi, to.hi); r++) refs.push({ hi: r, lo: r, suit: "" });
  } else if (from.hi === to.hi) {
    for (let k = Math.min(from.lo, to.lo); k <= Math.max(from.lo, to.lo); k++) refs.push({ ...from, lo: k });
  } else if (from.lo - from.hi === to.lo - to.hi) {
    const gap = from.lo - from.hi;
    for (let r = Math.min(from.hi, to.hi); r <= Math.max(from.hi, to.hi); r++) {
      refs.push({ hi: r, lo: r + gap, suit: from.suit });
    }
  } else {
    return "An interval must use the same first rank or the same rank gap";
  }
  return refs;
}

/** Parses a range string; later entries overwrite earlier ones. */
export function parseRange(text: string): ParsedRange {
  const weights: Weights = new Map();
  const errors: string[] = [];
  if (text.trim() === "random") {
    for (const name of ALL_CLASSES) for (const combo of classCombos(name)) weights.set(combo, 1);
    return { weights, errors };
  }
  for (const raw of text.split(",")) {
    let entry = raw.trim();
    if (!entry) continue;
    let weight = 1;
    const colon = entry.indexOf(":");
    if (colon >= 0) {
      const value = entry.slice(colon + 1).trim();
      weight = Number(value);
      if (!WEIGHT.test(value) || weight > 1) {
        errors.push(`${raw.trim()}: weight must be a decimal number in 0..1`);
        continue;
      }
      entry = entry.slice(0, colon).trim();
    }
    const combo = COMBO.exec(entry);
    if (combo) {
      const r1 = RANKS.indexOf(combo[1]!.toUpperCase());
      const r2 = RANKS.indexOf(combo[3]!.toUpperCase());
      const s1 = SUITS.indexOf(combo[2]!.toLowerCase());
      const s2 = SUITS.indexOf(combo[4]!.toLowerCase());
      if (r1 === r2 && s1 === s2) {
        errors.push(`${entry}: the same card cannot be used twice`);
        continue;
      }
      weights.set(comboKey(r1, s1, r2, s2), weight);
      continue;
    }
    const refs = expand(entry);
    if (typeof refs === "string") {
      errors.push(`${entry}: ${refs}`);
      continue;
    }
    for (const ref of refs) for (const key of classCombosOf(ref)) weights.set(key, weight);
  }
  return { weights, errors };
}

/** Whether a combo shares a card with the board. */
export function blocked(combo: Combo, board: string[]): boolean {
  return board.includes(combo.slice(0, 2)) || board.includes(combo.slice(2, 4));
}

export interface ClassCell {
  /** Mean weight over the class's combos that the board leaves live. */
  weight: number;
  /** Combos the board removes. */
  blocked: number;
  /** All combos share one weight, so the class can be written as one entry. */
  uniform: boolean;
}

export function classCells(weights: Weights, board: string[]): Map<string, ClassCell> {
  const cells = new Map<string, ClassCell>();
  for (const name of ALL_CLASSES) {
    const combos = classCombos(name);
    const live = combos.filter((combo) => !blocked(combo, board));
    const values = live.map((combo) => weights.get(combo) ?? 0);
    const all = combos.map((combo) => weights.get(combo) ?? 0);
    cells.set(name, {
      weight: values.length === 0 ? 0 : values.reduce((a, b) => a + b, 0) / values.length,
      blocked: combos.length - live.length,
      uniform: all.every((value) => value === all[0]),
    });
  }
  return cells;
}

/** Classes with a positive live combo, and the live weighted combo count. */
export function rangeCounts(weights: Weights, board: string[]): { classes: number; combos: number } {
  const classes = new Set<string>();
  let combos = 0;
  for (const [combo, weight] of weights) {
    if (weight <= 0 || blocked(combo, board)) continue;
    combos += weight;
    classes.add(classOf(combo));
  }
  return { classes: classes.size, combos };
}

function withWeight(entry: string, weight: number): string {
  return weight === 1 ? entry : `${entry}:${Number(weight.toFixed(4))}`;
}

/**
 * Pushes the entries for runs of consecutive indexes. `top` is the index the
 * `+` form reaches (the highest rank the run can extend to); `name` spells
 * the class at one index.
 */
function pushRuns(out: string[], indexes: number[], top: number, name: (index: number) => string, weight: number) {
  let start = 0;
  while (start < indexes.length) {
    let end = start;
    while (end + 1 < indexes.length && indexes[end + 1] === indexes[end]! - 1) end++;
    const low = indexes[start]!;
    const high = indexes[end]!;
    if (high === top && low !== high) out.push(withWeight(`${name(low)}+`, weight));
    else if (low !== high) out.push(withWeight(`${name(high)}-${name(low)}`, weight));
    else out.push(withWeight(name(low), weight));
    start = end + 1;
  }
}

/**
 * Writes weights back as a range string: uniform classes as `+`/`-` runs,
 * the others combo by combo. `parseRange(formatRange(w))` reproduces `w`
 * for weights with at most four decimals.
 */
export function formatRange(weights: Weights): string {
  const out: string[] = [];
  const combosOut: string[] = [];
  const uniform = new Map<string, number>();
  for (const name of ALL_CLASSES) {
    const combos = classCombos(name);
    const values = combos.map((combo) => weights.get(combo) ?? 0);
    if (values.every((value) => value === values[0])) {
      if (values[0]! > 0) uniform.set(name, values[0]!);
    } else {
      combos.forEach((combo, k) => {
        if (values[k]! > 0) combosOut.push(withWeight(combo, values[k]!));
      });
    }
  }
  const byWeight = new Map<number, string[]>();
  for (const [name, weight] of uniform) byWeight.set(weight, [...(byWeight.get(weight) ?? []), name]);
  for (const [weight, names] of [...byWeight].sort((a, b) => b[0] - a[0])) {
    const set = new Set(names);
    // Pairs, from the bottom: indexes descending so a run reads low to high.
    const pairs = [...RANKS]
      .map((_, r) => r)
      .filter((r) => set.has(className(r, r)))
      .reverse();
    pushRuns(out, pairs, 0, (r) => className(r, r), weight);
    for (const suit of ["s", "o"]) {
      for (let hi = 0; hi < RANKS.length; hi++) {
        const kickers: number[] = [];
        for (let lo = RANKS.length - 1; lo > hi; lo--) {
          const name = suit === "s" ? className(hi, lo) : className(lo, hi);
          if (set.has(name)) kickers.push(lo);
        }
        pushRuns(out, kickers, hi + 1, (lo) => `${RANKS[hi]}${RANKS[lo]}${suit}`, weight);
      }
    }
  }
  return [...out, ...combosOut].join(",");
}
