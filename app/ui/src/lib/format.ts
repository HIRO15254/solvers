export function formatInteger(raw: string): string {
  try {
    return BigInt(raw).toLocaleString("ja-JP")
  } catch {
    return raw
  }
}

export function formatBytes(raw: string | null, fallback = "—"): string {
  if (raw === null) {
    return fallback
  }
  try {
    const bytes = BigInt(raw)
    const units: Array<[bigint, string]> = [
      [1_099_511_627_776n, "TiB"],
      [1_073_741_824n, "GiB"],
      [1_048_576n, "MiB"],
      [1_024n, "KiB"],
    ]
    for (const [unit, label] of units) {
      if (bytes >= unit) {
        const whole = bytes / unit
        const tenth = ((bytes % unit) * 10n) / unit
        return `${whole.toLocaleString("ja-JP")}.${tenth} ${label}`
      }
    }
    return `${bytes.toLocaleString("ja-JP")} bytes`
  } catch {
    return raw
  }
}

export function formatElapsed(value: string | null): string {
  if (value === null) {
    return "—"
  }
  const parsed = Number(value)
  if (!Number.isFinite(parsed)) {
    return value
  }
  const seconds = Math.max(0, Math.floor(parsed))
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const rest = seconds % 60
  return hours > 0 ? `${hours}h ${minutes}m` : `${minutes}m ${rest}s`
}

export function formatRate(value: string | null): string {
  if (value === null) {
    return "—"
  }
  const parsed = Number(value)
  return Number.isFinite(parsed)
    ? parsed.toLocaleString("ja-JP", { maximumFractionDigits: 0 })
    : value
}

export function formatDecimal(value: string, digits = 6): string {
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed.toFixed(digits) : value
}

export function formatScientific(value: string, digits = 3): string {
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed.toExponential(digits) : value
}
