import { ranks } from "@/data/demo"
import type { StrategyCellViewModel } from "@/lib/solve-contract"
import { cn } from "@/lib/utils"

type StrategyMatrixProps = {
  cells: StrategyCellViewModel[]
  selectedHand: string
  onSelect: (cell: StrategyCellViewModel) => void
  compact?: boolean
}

function cellBackground(cell: StrategyCellViewModel) {
  if (cell.status === "unvisited") {
    return "var(--muted)"
  }

  const raiseEnd = probabilityPercent(cell, "raise-2500")
  const callEnd = raiseEnd + probabilityPercent(cell, "call")
  return `linear-gradient(135deg, #7c3aed 0% ${raiseEnd}%, #16a34a ${raiseEnd}% ${callEnd}%, #d4d4d8 ${callEnd}% 100%)`
}

function probabilityPercent(cell: StrategyCellViewModel, actionId: string) {
  if (cell.status === "unvisited") {
    return 0
  }
  const value =
    cell.probabilities.find((item) => item.actionId === actionId)
      ?.probabilityU16 ?? 0
  return (value / 65_535) * 100
}

export function StrategyMatrix({
  cells,
  selectedHand,
  onSelect,
  compact = false,
}: StrategyMatrixProps) {
  return (
    <div className="strategy-scroll" role="region" aria-label="13 × 13 戦略表">
      <div
        className={cn("strategy-matrix", compact && "strategy-matrix--compact")}
      >
        <span aria-hidden="true" />
        {ranks.map((rank) => (
          <span className="strategy-axis" key={`column-${rank}`}>
            {rank}
          </span>
        ))}
        {ranks.map((rank, row) => (
          <div className="contents" key={`row-${rank}`}>
            <span className="strategy-axis">{rank}</span>
            {ranks.map((_, column) => {
              const cell = cells[row * ranks.length + column]
              const isSelected = selectedHand === cell.hand
              const raise = probabilityPercent(cell, "raise-2500")
              const call = probabilityPercent(cell, "call")
              const darkText = cell.status === "unvisited" || raise + call < 28
              const accessibleStrategy =
                cell.status === "unvisited"
                  ? `${cell.hand}: 未訪問。戦略とEVはありません`
                  : `${cell.hand}: raise ${raise.toFixed(1)}%, call ${call.toFixed(1)}%, fold ${probabilityPercent(cell, "fold").toFixed(1)}%`

              return (
                <button
                  type="button"
                  key={cell.hand}
                  className={cn(
                    "strategy-cell",
                    isSelected && "strategy-cell--selected",
                    darkText ? "text-zinc-950" : "text-white",
                    cell.status === "unvisited" && "strategy-cell--unvisited"
                  )}
                  style={{ background: cellBackground(cell) }}
                  aria-label={accessibleStrategy}
                  aria-pressed={isSelected}
                  onClick={() => onSelect(cell)}
                >
                  <span>{cell.hand}</span>
                  {!compact && cell.status === "visited" ? (
                    <small>{Math.round(raise)}R</small>
                  ) : null}
                </button>
              )
            })}
          </div>
        ))}
      </div>
    </div>
  )
}

type ActionBreakdownProps = {
  cell: StrategyCellViewModel
}

export function ActionBreakdown({ cell }: ActionBreakdownProps) {
  if (cell.status === "unvisited") {
    return (
      <div className="space-y-4">
        <div>
          <p className="text-2xl font-semibold tracking-tight">{cell.hand}</p>
          <p className="text-xs text-muted-foreground">
            {cell.combos} combos · 未訪問
          </p>
        </div>
        <p className="rounded-md border border-dashed p-2 text-xs text-muted-foreground">
          このinfosetは未訪問です。戦略・EVは存在せず、uniformや0%として補完しません。
        </p>
      </div>
    )
  }

  const actions = [
    {
      label: "Raise to 2.500 BB",
      value: probabilityPercent(cell, "raise-2500"),
      color: "bg-violet-600",
    },
    {
      label: "Call",
      value: probabilityPercent(cell, "call"),
      color: "bg-green-600",
    },
    {
      label: "Fold",
      value: probabilityPercent(cell, "fold"),
      color: "bg-zinc-300",
    },
  ]

  return (
    <div className="space-y-4">
      <div className="flex items-end justify-between gap-4">
        <div>
          <p className="text-2xl font-semibold tracking-tight">{cell.hand}</p>
          <p className="text-xs text-muted-foreground">
            {cell.combos} combos · average strategy
          </p>
        </div>
        <div className="text-right">
          <p className="text-xs text-muted-foreground">EV</p>
          <p className="font-mono text-sm font-semibold">
            {cell.evMilliBb >= 0 ? "+" : ""}
            {(cell.evMilliBb / 1_000).toFixed(3)} BB
          </p>
        </div>
      </div>

      <div className="space-y-3">
        {actions.map((action) => (
          <div className="space-y-1.5" key={action.label}>
            <div className="flex items-center justify-between text-xs">
              <span>{action.label}</span>
              <span className="font-mono font-medium">
                {action.value.toFixed(1)}%
              </span>
            </div>
            <div className="h-1.5 overflow-hidden rounded-full bg-muted">
              <div
                className={cn("h-full rounded-full", action.color)}
                style={{ width: `${action.value}%` }}
              />
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
