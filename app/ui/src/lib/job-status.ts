import type { JobState } from "@/lib/solve-contract"

export const terminalStates = new Set<JobState>([
  "target-reached",
  "sweep-limit",
  "time-limit",
  "cancelled",
  "resource-limit",
  "failed",
])

export function isTerminal(state: JobState): boolean {
  return terminalStates.has(state)
}

export function statusBadgeVariant(
  state: JobState
): "default" | "secondary" | "destructive" | "outline" {
  if (state === "target-reached") {
    return "default"
  }
  if (state === "failed") {
    return "destructive"
  }
  if (state === "cancelled" || state === "resource-limit") {
    return "outline"
  }
  return "secondary"
}

export function statusBadgeClass(state: JobState): string | undefined {
  if (state === "target-reached") {
    return "border-transparent bg-emerald-600 text-white"
  }
  if (state === "cancelled" || state === "resource-limit") {
    return "border-amber-400 bg-amber-50 text-amber-900 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-200"
  }
  return undefined
}
