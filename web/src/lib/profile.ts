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

export function relockPayload(locks: PendingLock[], replace: boolean, freezeUpstream: boolean): RelockRequest {
  return {
    profileToml: stringify({ profile: { lock: locks } }),
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
