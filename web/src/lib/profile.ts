import type { TomlPrimitive, TomlTable } from "smol-toml";
import { getAt, setAt } from "./config";

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
