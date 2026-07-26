import type { TypedAction } from "@/lib/solve-contract"

const aggressiveColors = [
  "#fb923c",
  "#ef4444",
  "#dc2626",
  "#b91c1c",
  "#991b1b",
] as const

/**
 * GTO study tools convention: folds are blue, checks/calls are green, and
 * aggressive actions use a red size ladder. The index only distinguishes
 * multiple bet/raise sizes; semantics never change color when action order does.
 */
export function strategyActionColor(action: TypedAction, index = 0) {
  switch (action.semantic) {
    case "fold":
      return "#2563eb"
    case "check":
      return "#16a34a"
    case "call":
      return "#22c55e"
    case "bet-to":
    case "raise-to":
      return aggressiveColors[index % aggressiveColors.length]
  }
}

export function strategyActionColors(actions: TypedAction[]) {
  let aggressiveIndex = 0
  return actions.map((action) => {
    const index = aggressiveIndex
    if (action.semantic === "bet-to" || action.semantic === "raise-to") {
      aggressiveIndex += 1
    }
    return strategyActionColor(action, index)
  })
}
