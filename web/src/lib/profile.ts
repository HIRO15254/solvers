import type { TomlPrimitive, TomlTable } from "smol-toml";
import { getAt, setAt } from "./config";
import { stringify } from "smol-toml";
import type { RelockRequest, StrategyRow } from "../api/types";
import { ALL_CLASSES, classOf } from "./range";

export interface PendingLock {
  node: string;
  hands?: string;
  category?: string;
  strategy: Record<string, number>;
  confidence?: number;
}

export interface PendingIncentive {
  node: string;
  hands?: string;
  category?: string;
  action: string;
  pot?: number;
  bb?: number;
}

export interface PendingFrequency {
  node: string;
  hands?: string;
  category?: string;
  action: string;
  target: number;
}

export type PendingEntry =
  | { kind: "lock"; value: PendingLock }
  | { kind: "incentive"; value: PendingIncentive }
  | { kind: "frequency"; value: PendingFrequency };

export function queuedProfile(entries: PendingEntry[]): { lock?: PendingLock[]; incentive?: PendingIncentive[]; frequency?: PendingFrequency[] } {
  const profile: { lock?: PendingLock[]; incentive?: PendingIncentive[]; frequency?: PendingFrequency[] } = {};
  for (const entry of entries) {
    if (entry.kind === "lock") (profile.lock ??= []).push(entry.value);
    else if (entry.kind === "incentive") (profile.incentive ??= []).push(entry.value);
    else (profile.frequency ??= []).push(entry.value);
  }
  return profile;
}
/** Validate and construct an action-based entry from the editor's current values. */
export function actionProfileEntry(kind: "incentive" | "frequency", node: string, selector: { hands?: string; category?: string }, actions: string[], action: string, amount: string, unit: string): PendingEntry {
  if (!actions.includes(action)) throw new Error("Select an available action.");
  const number = amount.trim() === "" ? NaN : Number(amount);
  if (!Number.isFinite(number)) throw new Error(kind === "frequency" ? "Enter a target frequency between 0 and 1." : "Enter a finite bonus.");
  if (kind === "frequency") {
    if (number < 0 || number > 1) throw new Error("Enter a target frequency between 0 and 1.");
    return { kind, value: { node, ...selector, action, target: number } };
  }
  return { kind, value: { node, ...selector, action, ...(unit === "pot" ? { pot: number } : { bb: number }) } };
}


export function classesToRange(classes: Iterable<string>): string {
  const selected = new Set(classes);
  return ALL_CLASSES.filter((name) => selected.has(name)).join(",");
}

/** Reach-weighted strategy for all rows or the selected hand classes. */
export function selectionStrategy(rows: StrategyRow[], actions: string[], classes?: Set<string>): number[] {
  const mass = actions.map(() => 0);
  let weight = 0;
  for (const row of rows) {
    if (classes && !classes.has(classOf(row.combo))) continue;
    weight += row.weight;
    actions.forEach((_, i) => { mass[i]! += row.weight * (row.probabilities[i] ?? 0); });
  }
  return mass.map((value) => weight > 0 ? value / weight : 0);
}

/** Round displayed probabilities in thousandths, keeping a valid total for a nonempty selection. */
export function roundedStrategy(probabilities: number[]): string[] {
  if (!probabilities.some((p) => p > 0)) return probabilities.map(() => "0");
  let remaining = 1000;
  return probabilities.map((p, i) => {
    const units = i === probabilities.length - 1 ? remaining : Math.min(remaining, Math.max(0, Math.round(p * 1000)));
    remaining -= units;
    return (units / 1000).toFixed(3);
  });
}

export function relockPayload(entries: PendingEntry[], replace: boolean, freezeUpstream: boolean): RelockRequest {
  return {
    profileToml: stringify({ profile: queuedProfile(entries) }),
    replace,
    freezeUpstream,
    clearProfile: false,
  };
}

/** Replace one profile entry without changing any other config or profile keys. */
export function changeProfileEntry(doc: TomlTable, kind: string, index: number, entry: TomlTable | null): TomlTable {
  const current = getAt(doc, ["profile", kind]);
  const entries = Array.isArray(current) ? [...current] : [];
  if (entry === null) entries.splice(index, 1);
  else if (index === entries.length) entries.push(entry);
  else entries[index] = entry;
  return setAt(doc, ["profile", kind], entries.length ? entries as TomlPrimitive : undefined);
}

/** Import a current strategy after the user has agreed to replace existing locks. */
export function replaceProfileLocks(doc: TomlTable, node: string, combos: TomlTable): TomlTable {
  return setAt(doc, ["profile", "lock"], [{ node, combos }]);
}
