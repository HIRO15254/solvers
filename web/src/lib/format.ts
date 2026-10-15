/** BB amounts at the input's 0.001 BB grid, without trailing zeros. */
export function bb(value: number): string {
  return String(Number(value.toFixed(3)));
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
