import type { StrategyEntry, TypedAction } from "@/lib/solve-contract"
import { strategyActionColors } from "@/lib/strategy-colors"
import { cn } from "@/lib/utils"

const ranks = [
  "A",
  "K",
  "Q",
  "J",
  "T",
  "9",
  "8",
  "7",
  "6",
  "5",
  "4",
  "3",
  "2",
] as const

function handLabel(row: number, column: number) {
  if (row === column) {
    return `${ranks[row]}${ranks[column]}`
  }
  if (row < column) {
    return `${ranks[row]}${ranks[column]}s`
  }
  return `${ranks[column]}${ranks[row]}o`
}

const semanticShort: Record<TypedAction["semantic"], string> = {
  fold: "F",
  check: "X",
  call: "C",
  "bet-to": "B",
  "raise-to": "R",
}

function probabilityPercent(entry: StrategyEntry, actionIndex: number) {
  if (entry.status === "unvisited" || entry.probabilityU16 === null) {
    return 0
  }
  return ((entry.probabilityU16[actionIndex] ?? 0) / 65_535) * 100
}

function dominantAction(
  entry: StrategyEntry | undefined,
  actions: TypedAction[]
): { short: string; percent: number } | null {
  if (!entry || entry.status === "unvisited" || !entry.probabilityU16?.length) {
    return null
  }
  let bestIndex = -1
  let bestPercent = -Infinity
  actions.forEach((_, index) => {
    const percent = probabilityPercent(entry, index)
    if (percent > bestPercent) {
      bestPercent = percent
      bestIndex = index
    }
  })
  if (bestIndex === -1) {
    return null
  }
  const action = actions[bestIndex]
  return { short: semanticShort[action.semantic], percent: bestPercent }
}

function isOutOfRange(entry: StrategyEntry | undefined) {
  if (!entry) {
    return true
  }
  const weight = Number(entry.weight)
  return Number.isFinite(weight) && weight <= 0
}

function cellBackground(
  entry: StrategyEntry | undefined,
  actionColors: string[]
) {
  if (isOutOfRange(entry)) {
    return "#18181b"
  }
  if (!entry || entry.status === "unvisited" || entry.probabilityU16 === null) {
    return "repeating-linear-gradient(135deg, #52525b 0 5px, #3f3f46 5px 10px)"
  }

  let cursor = 0
  const stops = actionColors.map((color, index) => {
    const start = cursor
    cursor += probabilityPercent(entry, index)
    return `${color} ${start}% ${Math.min(100, cursor)}%`
  })
  return stops.length
    ? `linear-gradient(90deg, ${stops.join(", ")})`
    : "#18181b"
}

type StrategyMatrixProps = {
  entries: StrategyEntry[]
  actions: TypedAction[]
  selectedId: string | null
  onSelect: (entry: StrategyEntry) => void
  compact?: boolean
}

export function StrategyMatrix({
  entries,
  actions,
  selectedId,
  onSelect,
  compact = false,
}: StrategyMatrixProps) {
  const entriesByLabel = new Map(entries.map((entry) => [entry.label, entry]))
  const actionColors = strategyActionColors(actions)

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
              const label = handLabel(row, column)
              const entry = entriesByLabel.get(label)
              const isSelected = entry ? selectedId === entry.id : false
              const dominant = dominantAction(entry, actions)
              const accessibleStrategy = !entry
                ? `${label}: snapshotにデータがありません`
                : entry.status === "unvisited" || entry.probabilityU16 === null
                  ? `${entry.label}: unvisited`
                  : `${entry.label}: ${actions
                      .map(
                        (action, index) =>
                          `${action.label} ${probabilityPercent(entry, index).toFixed(1)}%`
                      )
                      .join(", ")}`

              return (
                <button
                  type="button"
                  key={label}
                  className={cn(
                    "strategy-cell",
                    isSelected && "strategy-cell--selected",
                    (!entry || entry.status === "unvisited") &&
                      "strategy-cell--unvisited text-white",
                    entry?.status === "visited" && "text-white"
                  )}
                  style={{
                    background: cellBackground(entry, actionColors),
                  }}
                  aria-label={accessibleStrategy}
                  title={accessibleStrategy}
                  aria-pressed={isSelected}
                  onClick={() => entry && onSelect(entry)}
                  disabled={!entry}
                >
                  <span>{label}</span>
                  {!compact && dominant ? (
                    <small>
                      {dominant.short} {Math.round(dominant.percent)}
                    </small>
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

type StrategyBucketListProps = {
  entries: StrategyEntry[]
  actions: TypedAction[]
  selectedId: string | null
  onSelect: (entry: StrategyEntry) => void
}

export function StrategyBucketList({
  entries,
  actions,
  selectedId,
  onSelect,
}: StrategyBucketListProps) {
  const actionColors = strategyActionColors(actions)
  return (
    <div className="max-h-[520px] divide-y overflow-auto rounded-lg border">
      {entries.map((entry) => {
        const accessibleStrategy =
          entry.status === "unvisited" || entry.probabilityU16 === null
            ? `${entry.label}: unvisited`
            : `${entry.label}: ${actions
                .map(
                  (action, index) =>
                    `${action.label} ${probabilityPercent(entry, index).toFixed(1)}%`
                )
                .join(", ")}`
        return (
          <button
            type="button"
            className={cn(
              "grid w-full gap-3 px-3 py-2.5 text-left hover:bg-muted/50 md:grid-cols-[minmax(140px,0.6fr)_minmax(260px,1.4fr)]",
              selectedId === entry.id && "bg-muted"
            )}
            key={entry.id}
            title={accessibleStrategy}
            onClick={() => onSelect(entry)}
            aria-pressed={selectedId === entry.id}
          >
            <span>
              <strong className="block">{entry.label}</strong>
              <small className="text-muted-foreground">
                {entry.bucketPath?.join(" / ") ?? entry.id} · {entry.status}
              </small>
            </span>
            {entry.status === "unvisited" || !entry.probabilityU16 ? (
              <span className="text-xs text-muted-foreground">unvisited</span>
            ) : (
              <span className="flex min-w-0 overflow-hidden rounded-full">
                {actions.map((action, index) => (
                  <span
                    key={action.id}
                    className="h-5 min-w-0"
                    title={`${action.label}: ${probabilityPercent(entry, index).toFixed(1)}%`}
                    style={{
                      width: `${probabilityPercent(entry, index)}%`,
                      backgroundColor: actionColors[index],
                    }}
                  />
                ))}
              </span>
            )}
          </button>
        )
      })}
    </div>
  )
}

type ActionBreakdownProps = {
  entry: StrategyEntry | null
  actions: TypedAction[]
  showEv?: boolean
}

export function ActionBreakdown({
  entry,
  actions,
  showEv = true,
}: ActionBreakdownProps) {
  const actionColors = strategyActionColors(actions)
  if (!entry) {
    return (
      <p className="text-xs text-muted-foreground">
        戦略entryを選択してください。
      </p>
    )
  }

  if (entry.status === "unvisited" || entry.probabilityU16 === null) {
    return (
      <div className="space-y-4">
        <div>
          <p className="text-2xl font-semibold tracking-tight">{entry.label}</p>
          <p className="text-xs text-muted-foreground">
            {entry.comboCount ?? "—"} combos · unvisited
          </p>
        </div>
        <p className="rounded-md border border-dashed p-2 text-xs text-muted-foreground">
          このinfosetは未訪問です。
          {showEv
            ? "strategyとEVを0%やuniformで補完しません。"
            : "strategyを0%やuniformで補完しません。"}
        </p>
      </div>
    )
  }

  return (
    <div className="space-y-4">
      <div className="flex items-end justify-between gap-4">
        <div>
          <p className="text-2xl font-semibold tracking-tight">{entry.label}</p>
          <p className="text-xs text-muted-foreground">
            {entry.comboCount ?? "—"} combos · weight {entry.weight}
          </p>
        </div>
        {showEv ? (
          <div className="text-right">
            <p className="text-xs text-muted-foreground">EV</p>
            <p className="font-mono text-sm font-semibold">
              {entry.ev
                ? `${entry.ev.value} ${entry.ev.unit}`
                : "not available"}
            </p>
          </div>
        ) : null}
      </div>

      <div className="space-y-3">
        {actions.map((action, index) => {
          const value = probabilityPercent(entry, index)
          return (
            <div className="space-y-1.5" key={action.id}>
              <div className="flex items-center justify-between gap-3 text-xs">
                <span>{action.label}</span>
                <span className="font-mono font-medium">
                  {value.toFixed(1)}%
                </span>
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full"
                  style={{
                    width: `${value}%`,
                    backgroundColor: actionColors[index],
                  }}
                />
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}
