// What a run is, for display: read from its `run.toml` and the daemon's
// `validate` of that config. Both are fixed by the config hash, so each
// distinct config is fetched once per page load.

import { api } from "../api/client";
import type { HuResources, RunSummary, StartState } from "../api/types";
import { getAt, readDoc } from "./config";
import { parseBoard } from "./cards";
import { bb, significant } from "./format";

export interface RunMeta {
  name: string | null;
  line: string;
  board: string[];
  start: StartState | null;
  utilityUnit: string;
  /** Total prizes for a tournament, the unit `%prizes` targets refer to. */
  prizePool: number | null;
  target: string | null;
  threads: string;
  resources: HuResources | null;
  economics: string;
}

const cache = new Map<string, Promise<RunMeta>>();

export function runMeta(run: RunSummary): Promise<RunMeta> {
  let meta = cache.get(run.configHash);
  if (!meta) {
    meta = load(run);
    meta.catch(() => cache.delete(run.configHash));
    cache.set(run.configHash, meta);
  }
  return meta;
}

async function load(run: RunSummary): Promise<RunMeta> {
  const text = await api.artifactText(run.runId, "run.toml");
  const doc = readDoc(text);
  const hu = run.gameKind === "hu-postflop";
  // Resources build the P1 tree estimate (fractions of a second); the P2
  // arena count can take far longer, so it is not requested here.
  const validated = await api.validate(text, hu).catch(() => null);
  const payouts = getAt(doc, ["economics", "payouts"]);
  const kind = String(getAt(doc, ["economics", "kind"]) ?? "cash");
  const rate = getAt(doc, ["economics", "rake", "rate"]);
  const stack = getAt(doc, ["table", "stack_bb"]);
  return {
    name: (getAt(doc, ["meta", "name"]) as string | undefined) ?? null,
    line: String(getAt(doc, ["spot", "line"]) ?? ""),
    board: parseBoard(String(getAt(doc, ["spot", "board"]) ?? "")) ?? [],
    start: validated?.summary.start ?? null,
    utilityUnit: validated?.summary.utilityUnit ?? (kind === "tournament" ? "prizes" : "BB"),
    prizePool: Array.isArray(payouts) ? payouts.reduce<number>((sum, p) => sum + Number(p), 0) : null,
    target: (getAt(doc, ["solver", "stop", "target"]) as string | undefined) ?? null,
    threads: String(getAt(doc, ["run", "threads"]) ?? "auto"),
    resources: validated?.summary.resources ?? null,
    economics: [
      stack !== undefined ? `${stack}bb` : null,
      kind === "tournament" ? "ICM" : rate !== undefined ? `rake ${bb(Number(rate) * 100)}%` : "No rake",
    ]
      .filter(Boolean)
      .join(" · "),
  };
}

/**
 * `NashConv / 2` against the start pot (cash) or the prize pool
 * (tournament), the quantities stop targets are written in
 * (docs/nlh-input-v1.jp.md §10). Falls back to utility units.
 */
export function exploitability(nashConv: number, meta: RunMeta | undefined): string {
  const pct = exploitabilityPercent(nashConv, meta);
  if (pct !== null) return `${significant(pct)}% ${meta?.utilityUnit === "prizes" ? "prizes" : "pot"}`;
  return `${significant(nashConv / 2)} ${meta?.utilityUnit ?? ""}`.trim();
}

/** The same quantity as a percentage, for charts (`null` when no normalizer is known). */
export function exploitabilityPercent(nashConv: number, meta: RunMeta | undefined): number | null {
  if (meta?.utilityUnit === "prizes" && meta.prizePool) return (nashConv / 2 / meta.prizePool) * 100;
  if (meta?.start && meta.utilityUnit === "BB") return (nashConv / 2 / meta.start.pot) * 100;
  return null;
}

/** The stop target in the same percentage, when the run has one that converts. */
export function targetPercent(meta: RunMeta | undefined): number | null {
  const match = /^(\d+(?:\.\d+)?)(%pot|%prizes|bb)$/.exec(meta?.target ?? "");
  if (!match) return null;
  const value = Number(match[1]);
  if (match[2] !== "bb") return value;
  return meta?.start ? (value / meta.start.pot) * 100 : null;
}
