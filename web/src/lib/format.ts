import type { LineStep } from "../api/types";

/** Rounded BB display; never use this spelling for a line token. */
export function bb(value: number): string {
  return String(Number(value.toFixed(6)));
}

/** Exact human-readable chip amount for a table's big blind. */
export function chipDisplay(chips: bigint, bbChips: number): string {
  if (!Number.isSafeInteger(bbChips) || bbChips <= 0) return `${chips}c`;
  let denominator = bbChips;
  let twos = 0;
  let fives = 0;
  while (denominator % 2 === 0) { denominator /= 2; twos++; }
  while (denominator % 5 === 0) { denominator /= 5; fives++; }
  if (denominator !== 1) return `${chips}c`;
  const digits = Math.max(twos, fives);
  const scaled = chips * 10n ** BigInt(digits) / BigInt(bbChips);
  if (digits === 0) return String(scaled);
  const text = scaled.toString().padStart(digits + 1, "0");
  const whole = text.slice(0, -digits);
  const fraction = text.slice(-digits).replace(/0+$/, "");
  return fraction ? `${whole}.${fraction}` : whole;
}

/** Resolve a BB or chip amount to whole chips without floating-point conversion. */
export function amountChips(value: unknown, bbChips: number, rounding = "nearest"): bigint | undefined {
  if (typeof value !== "number" && typeof value !== "string") return undefined;
  const text = String(value);
  if (/^(0|[1-9]\d*)c$/.test(text)) return BigInt(text.slice(0, -1));
  const match = /^(\d+)(?:\.(\d+))?(?:bb)?$/.exec(text);
  if (!match || !Number.isSafeInteger(bbChips) || bbChips <= 0) return undefined;
  const fraction = match[2] ?? "";
  const denominator = 10n ** BigInt(fraction.length);
  const scaled = BigInt(match[1] + fraction) * BigInt(bbChips);
  let chips = scaled / denominator;
  const remainder = scaled % denominator;
  if (rounding === "up" ? remainder > 0n : rounding !== "down" && remainder * 2n >= denominator) chips++;
  return chips;
}

/** Display an amount key, preserving exact chip amounts. */
export function amountDisplay(value: unknown, bbChips: number, rounding = "nearest"): string {
  const chips = amountChips(value, bbChips, rounding);
  return chips === undefined ? (value == null ? "" : String(value)) : chipDisplay(chips, bbChips);
}

/** TOML numbers are BB; explicitly suffixed inputs remain strings. */
export function amountInput(text: string): number | string | undefined {
  const value = text.trim();
  if (/^(0|[1-9]\d*)c$/.test(value) || /^\d+(?:\.\d+)?bb$/.test(value)) return value;
  if (/^\d+(?:\.\d+)?$/.test(value)) return Number(value);
  return undefined;
}

/** Preserve a written line only when it is the line that was replayed. */
export function replayedLine(replayed: string, written: string, isRequested: boolean): string {
  return isRequested ? written : replayed;
}

/** Normalize an entered BB wager without rounding away its value. */
export function normalizedWager(value: string): string {
  return String(Number(value.trim()));
}

/** Canonical line amount, matching ChipScale::amount_text. */
export function lineAmount(value: number, bbChips: number): string {
  return chipDisplay(BigInt(Math.round(value * bbChips)), bbChips);
}

/** Keep written wagers in the replayed prefix and the newly selected action. */
export function writtenLine(line: string, previous: string, moves: string[], canonicalMoves: string[]): string {
  const parts = line.split(/(, | \/ )/);
  const old = previous.split(/, | \/ /);
  const last = moves.at(-1);
  const canonical = canonicalMoves.at(-1);
  const lastAction = parts.length - 1;
  for (let i = 0; i < old.length && i * 2 < parts.length; i++) {
    const index = i * 2;
    if (index === lastAction && canonical !== "f" && last !== canonical) break;
    const before = old[i]!.match(/^(\S+)\s+([br])(\S+)$/);
    const after = parts[index]!.match(/^(\S+)\s+([br])(\S+)$/);
    if (before && after && before[1] === after[1] && before[2] === after[2]) {
      parts[index] = old[i]!;
    } else if (old[i] !== parts[index]) {
      break;
    }
  }
  const preserved = parts.join("");
  return last && canonical && /^[br]/.test(last) && last !== canonical && preserved.endsWith(canonical)
    ? preserved.slice(0, -canonical.length) + last
    : preserved;
}

/** One replayed action as the line grammar spells it. */
export function lineToken(step: LineStep, bbChips: number): string {
  const action = step.action;
  if (action === "fold") return "f";
  if (action === "check") return "x";
  if (typeof action !== "object") return String(action);
  const [kind, detail] = Object.entries(action)[0] as [string, { to?: number; all_in?: boolean }];
  if (detail.all_in && kind !== "call") return "a";
  if (kind === "call") return "c";
  return `${kind === "bet-to" ? "b" : "r"}${lineAmount(detail.to ?? 0, bbChips)}`;
}

export function integer(value: number): string {
  return value.toLocaleString("en-US");
}

export function percent(fraction: number, digits = 1): string {
  return `${(fraction * 100).toFixed(digits)}%`;
}

/** A small positive quantity with three significant digits (`0.245`, `0.00481`). */
export function significant(value: number): string {
  if (!Number.isFinite(value)) return "—";
  if (value === 0) return "0";
  return value
    .toPrecision(3)
    .replace(/(\.\d*?)0+$/, "$1")
    .replace(/\.$/, "");
}

export function bytes(value: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${unit === 0 ? value : value.toFixed(2)} ${units[unit]}`;
}

export function seconds(value: number): string {
  if (value < 60) return `${value.toFixed(1)} s`;
  const minutes = Math.floor(value / 60);
  if (minutes < 60) return `${minutes}m ${Math.round(value % 60)}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

export function clock(unixMs: number): string {
  return new Date(unixMs).toLocaleTimeString("en-US", { hour12: false });
}
