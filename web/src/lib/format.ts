import type { LineStep } from "../api/types";

/** Rounded BB display; never use this spelling for a line token. */
export function bb(value: number): string {
  return String(Number(value.toFixed(6)));
}

/** Canonical line amount, matching ChipScale::amount_text. */
export function lineAmount(value: number, bbChips: number): string {
  const chips = Math.round(value * bbChips);
  let denominator = bbChips;
  let twos = 0;
  let fives = 0;
  while (denominator % 2 === 0) { denominator /= 2; twos++; }
  while (denominator % 5 === 0) { denominator /= 5; fives++; }
  if (denominator !== 1) return `${chips}c`;
  const digits = Math.max(twos, fives);
  const scaled = BigInt(chips) * 10n ** BigInt(digits) / BigInt(bbChips);
  if (digits === 0) return String(scaled);
  const text = scaled.toString().padStart(digits + 1, "0");
  const whole = text.slice(0, -digits);
  const fraction = text.slice(-digits).replace(/0+$/, "");
  return fraction ? `${whole}.${fraction}` : whole;
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
