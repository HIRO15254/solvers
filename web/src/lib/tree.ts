// What the pre-solve tree's rows mean for display: each action's size as the
// script's size literals read it (pot percentage, multiple of the wager
// faced, all-in) and where it leads. Rows come from `POST /v1/tree`; their
// histories spell the moves (`x`, `c`, `f`, `r<to>`, `[card]`).

import type { PreviewTreeRow, Seat, StartState } from "../api/types";

export type ActionKind = "fold" | "check" | "call" | "bet" | "raise";

export interface ActionInfo {
  kind: ActionKind;
  /** Bet amount or raise-to target on the street, in BB; for a call, the chips it adds. */
  amount: number | null;
  /** The wager over the one faced, as a share of the pot after calling (a bet: of the pot), in percent. */
  potPercent: number | null;
  /** A raise-to target as a multiple of the wager faced. */
  times: number | null;
  /** The actor puts in every chip left. */
  allIn: boolean;
}

/** Where an action leads. */
export type Outcome = "decision" | "deal" | "showdown" | "fold";

const TOKEN = /x|c|f|[br](\d+(?:\.\d+)?c?)|\[[^\]]*\]/g;
const DECIMAL_TOKEN = /x|c|f|[br](\d+(?:\.\d+)?)|\[[^\]]*\]/g;

function amount(text: string, bbChips: number): number {
  return text.endsWith("c") ? Number(text.slice(0, -1)) / bbChips : Number(text);
}

/** Street wagers and chips put in on earlier streets at the node a history reaches; the tree starts a street. */
export function wagersAt(history: string, bbChips: number): { wager: Record<Seat, number>; earlier: Record<Seat, number> } {
  const wager = { oop: 0, ip: 0 };
  const earlier = { oop: 0, ip: 0 };
  let actor: Seat = "oop";
  const other = (seat: Seat): Seat => (seat === "oop" ? "ip" : "oop");
  let denominator = bbChips;
  while (denominator % 2 === 0) denominator /= 2;
  while (denominator % 5 === 0) denominator /= 5;
  for (const [token, to] of history.matchAll(denominator === 1 ? DECIMAL_TOKEN : TOKEN)) {
    if (token.startsWith("[")) {
      earlier.oop += wager.oop;
      earlier.ip += wager.ip;
      wager.oop = 0;
      wager.ip = 0;
      actor = "oop";
      continue;
    }
    if (token === "c") wager[actor] = Math.max(wager.oop, wager.ip);
    else if (to !== undefined) wager[actor] = amount(to, bbChips);
    actor = other(actor);
  }
  return { wager, earlier };
}

/** The stacks behind at the tree's root, by role, or null when the start is unknown. */
export function rootStacks(start: StartState | null | undefined): Record<Seat, number> | null {
  const stack = (seat: number) => start?.seats.find((s) => s.seat === seat)?.remaining_stack;
  const oop = start && stack(start.oop.seat);
  const ip = start && stack(start.ip.seat);
  return oop != null && ip != null ? { oop, ip } : null;
}

/** The action at `index` of an action row, sized against the node's pot and wagers. */
export function describeAction(row: PreviewTreeRow, index: number, stacks: Record<Seat, number> | null, bbChips: number): ActionInfo {
  const label = row.actions[index] ?? "";
  const actor = row.actor ?? "oop";
  const { wager, earlier } = wagersAt(row.history, bbChips);
  const faced = Math.max(wager.oop, wager.ip);
  const own = wager[actor];
  const behind = stacks ? stacks[actor] - earlier[actor] : null;
  const chips = (value: number) => Math.round(value * bbChips);
  const allInAt = (to: number) => behind !== null && chips(to) === chips(behind);
  const wagered = /^(bet|raise to) (\d+(?:\.\d+)?c?)$/.exec(label);
  if (wagered) {
    const to = amount(wagered[2]!, bbChips);
    const pot = (row.pot ?? 0) + faced - own;
    return {
      kind: wagered[1] === "bet" ? "bet" : "raise",
      amount: to,
      potPercent: pot > 0 ? ((to - faced) / pot) * 100 : null,
      times: faced > 0 ? to / faced : null,
      allIn: allInAt(to),
    };
  }
  if (label === "call") {
    const allIn = behind !== null && chips(faced) >= chips(behind);
    return { kind: "call", amount: faced - own, potPercent: null, times: null, allIn };
  }
  return { kind: label === "fold" ? "fold" : "check", amount: null, potPercent: null, times: null, allIn: false };
}

/** Where the action at `index` of an action row leads. */
export function outcome(row: PreviewTreeRow, index: number): Outcome {
  const kind = row.child_kinds[index];
  if (kind === "action") return "decision";
  if (kind === "chance") return "deal";
  return row.actions[index] === "fold" ? "fold" : "showdown";
}

/** A percentage as the size literals write it: whole above 10, one decimal below. */
export function sizePercent(value: number): string {
  return `${value >= 10 ? Math.round(value) : Number(value.toFixed(1))}%`;
}

/**
 * Rows of several `restOfStreet` selections, split back into one list per
 * selection: each list starts at its selected node, in request order.
 */
export function splitStreets(rows: PreviewTreeRow[], roots: string[]): PreviewTreeRow[][] {
  const out: PreviewTreeRow[][] = [];
  let next = 0;
  for (const row of rows) {
    if (next < roots.length && row.history === roots[next]) {
      out.push([]);
      next += 1;
    }
    out.at(-1)?.push(row);
  }
  return out;
}
