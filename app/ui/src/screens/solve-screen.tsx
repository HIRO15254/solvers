import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  Area,
  AreaChart,
  CartesianGrid,
  ReferenceLine,
  XAxis,
  YAxis,
} from "recharts"
import {
  IconActivity,
  IconAlertTriangle,
  IconArrowRight,
  IconCheck,
  IconClock,
  IconCloudDownload,
  IconCpu,
  IconInfoCircle,
  IconLoader2,
  IconRefresh,
  IconServer,
  IconSquare,
} from "@tabler/icons-react"

import {
  ActionBreakdown,
  StrategyBucketList,
  StrategyMatrix,
} from "@/components/strategy-matrix"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart"
import { Progress } from "@/components/ui/progress"
import { Separator } from "@/components/ui/separator"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import type {
  ConnectionProfileViewModel,
  StrategyEntry,
} from "@/lib/solve-contract"
import type {
  DesktopSolveGateway,
  LocalStrategySnapshot,
  NativeJobSnapshot,
  NativeProgress,
  NativeProgressEvent,
} from "@/lib/native-gateway"
import { errorMessage } from "@/lib/native-gateway"
import { strategyActionColors } from "@/lib/strategy-colors"
import {
  formatBytes,
  formatElapsed,
  formatInteger,
  formatRate,
} from "@/lib/format"
import {
  isTerminal,
  statusBadgeClass,
  statusBadgeVariant,
  terminalStates,
} from "@/lib/job-status"

const chartConfig = {
  deviation: {
    label: "Measured deviation",
    color: "#2563eb",
  },
} satisfies ChartConfig

const ROOT_NODE_ID = "00000000000000000000000000000000"

type SolveScreenProps = {
  profile: ConnectionProfileViewModel
  gateway: DesktopSolveGateway
  jobId: string
  displayName: string
  onViewResults: () => void
}

function sweepPercent(progress: NativeProgress | null) {
  if (!progress) {
    return 0
  }
  try {
    const current = BigInt(progress.sweeps)
    const maximum = BigInt(progress.maxSweeps)
    if (maximum <= 0n) {
      return 0
    }
    return Math.min(100, Number((current * 10_000n) / maximum) / 100)
  } catch {
    return 0
  }
}

function measuredDeviation(progress: NativeProgress | null) {
  const values =
    progress?.seats.flatMap((seat) =>
      seat.deviationGain ? [Number(seat.deviationGain.ci95[1])] : []
    ) ?? []
  const finite = values.filter(Number.isFinite)
  return finite.length ? Math.max(...finite) : null
}

function isActive(state: NativeJobSnapshot["state"]) {
  return !isTerminal(state)
}

export function SolveScreen({
  profile,
  gateway,
  jobId,
  displayName,
  onViewResults,
}: SolveScreenProps) {
  const [job, setJob] = useState<NativeJobSnapshot | null>(null)
  const [events, setEvents] = useState<NativeProgressEvent[]>([])
  const [strategy, setStrategy] = useState<LocalStrategySnapshot | null>(null)
  const [selectedEntry, setSelectedEntry] = useState<StrategyEntry | null>(null)
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const [cancelling, setCancelling] = useState(false)
  const [pollError, setPollError] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const [strategyMessage, setStrategyMessage] = useState<string | null>(null)
  const [chartScale, setChartScale] = useState<"log" | "linear">("log")
  const afterSequence = useRef<string | undefined>(undefined)
  const requestedNodeId = useRef(ROOT_NODE_ID)

  const acceptStrategy = useCallback((snapshot: LocalStrategySnapshot) => {
    setStrategy(snapshot)
    setStrategyMessage(null)
    setSelectedEntry((current) => {
      const entries = snapshot.view.entries
      return (
        entries.find((entry) => entry.id === current?.id) ??
        entries.find((entry) => entry.status === "visited") ??
        entries[0] ??
        null
      )
    })
  }, [])

  const refresh = useCallback(
    async (quiet = false) => {
      if (!gateway.availability.available || profile.kind === "remote") {
        setLoading(false)
        return true
      }
      if (!quiet) {
        setRefreshing(true)
      }
      try {
        const [nextJob, page] = await Promise.all([
          gateway.getJob(jobId),
          gateway.getEvents(jobId, afterSequence.current),
        ])
        setJob(nextJob)
        if (page.events.length) {
          setEvents((current) => {
            const bySequence = new Map(
              current.map((event) => [event.sequence, event])
            )
            page.events.forEach((event) =>
              bySequence.set(event.sequence, event)
            )
            return [...bySequence.values()].sort((left, right) => {
              try {
                const a = BigInt(left.sequence)
                const b = BigInt(right.sequence)
                return a < b ? -1 : a > b ? 1 : 0
              } catch {
                return left.sequence.localeCompare(right.sequence)
              }
            })
          })
        }
        if (page.lastSequence !== null) {
          afterSequence.current = page.lastSequence
        }
        try {
          const snapshot = await gateway.getStrategy(
            jobId,
            requestedNodeId.current
          )
          acceptStrategy(snapshot)
        } catch (error) {
          setStrategyMessage(errorMessage(error))
        }
        setPollError(null)
        return page.terminal || terminalStates.has(nextJob.state)
      } catch (error) {
        setPollError(errorMessage(error))
        return false
      } finally {
        setLoading(false)
        setRefreshing(false)
      }
    },
    [acceptStrategy, gateway, jobId, profile.kind]
  )

  const navigateToNode = useCallback(
    async (nodeId: string) => {
      requestedNodeId.current = nodeId
      setStrategy(null)
      setSelectedEntry(null)
      setStrategyMessage("選択したPreflop Nodeのsnapshotを待っています。")
      try {
        acceptStrategy(await gateway.getStrategy(jobId, nodeId))
      } catch (error) {
        setStrategyMessage(errorMessage(error))
      }
    },
    [acceptStrategy, gateway, jobId]
  )

  useEffect(() => {
    let disposed = false
    let timer: number | undefined

    const poll = async () => {
      if (disposed) {
        return
      }
      const terminal = await refresh(true)
      if (!disposed && !terminal) {
        timer = window.setTimeout(poll, 2_000)
      }
    }

    void poll()
    return () => {
      disposed = true
      if (timer !== undefined) {
        window.clearTimeout(timer)
      }
    }
  }, [refresh])

  const eventProgress =
    [...events].reverse().find((event) => event.progress)?.progress ?? null
  const progress = job?.progress ?? eventProgress
  const percent = sweepPercent(progress)
  const deviation = measuredDeviation(progress)
  const terminal = job ? terminalStates.has(job.state) : false
  const actionColors = useMemo(
    () => strategyActionColors(strategy?.actions ?? []),
    [strategy?.actions]
  )
  const chartData = useMemo(() => {
    const points = events.flatMap((event) => {
      const value = measuredDeviation(event.progress)
      return value === null
        ? []
        : [
            {
              sweeps: event.progress?.sweeps ?? "—",
              deviation: value,
            },
          ]
    })
    if (progress && measuredDeviation(progress) !== null) {
      points.push({
        sweeps: progress.sweeps,
        deviation: measuredDeviation(progress) as number,
      })
    }
    return [...new Map(points.map((point) => [point.sweeps, point])).values()]
  }, [events, progress])
  const target = Number(progress?.stopTarget)
  const canUseLog =
    chartData.length > 0 && chartData.every((p) => p.deviation > 0)
  const effectiveScale = chartScale === "log" && canUseLog ? "log" : "linear"
  const remainingSecs = (() => {
    if (!progress || progress.elapsedSecs === null) {
      return null
    }
    const elapsed = Number(progress.elapsedSecs)
    const sweeps = Number(progress.sweeps)
    const maxSweeps = Number(progress.maxSweeps)
    if (
      !Number.isFinite(elapsed) ||
      elapsed <= 0 ||
      !Number.isFinite(sweeps) ||
      !Number.isFinite(maxSweeps) ||
      sweeps <= 0 ||
      maxSweeps <= sweeps
    ) {
      return null
    }
    return Math.floor((elapsed * (maxSweeps - sweeps)) / sweeps)
  })()

  const cancel = async () => {
    setCancelling(true)
    setActionError(null)
    try {
      setJob(await gateway.cancelJob(jobId))
    } catch (error) {
      setActionError(errorMessage(error))
    } finally {
      setCancelling(false)
    }
  }

  if (!gateway.availability.available || profile.kind === "remote") {
    return (
      <div className="screen-stack">
        <div className="screen-heading">
          <div>
            <div className="eyebrow">SOLVE · {jobId}</div>
            <h1>Solve中</h1>
          </div>
        </div>
        <Alert>
          <IconInfoCircle />
          <AlertTitle>
            {profile.kind === "remote"
              ? "Remote Solveは未実装です"
              : "Browser preview"}
          </AlertTitle>
          <AlertDescription>
            {profile.kind === "remote"
              ? "RemoteはUIと契約のみで、jobへの接続は行いません。"
              : gateway.availability.reason}
          </AlertDescription>
        </Alert>
      </div>
    )
  }

  return (
    <div className="screen-stack">
      <div className="screen-heading">
        <div>
          <div className="flex items-center gap-2">
            {job && isActive(job.state) ? (
              <span className="live-pulse" />
            ) : null}
            <span className="eyebrow">RUN #{jobId}</span>
          </div>
          <h1>{loading && !job ? "Solveを読み込み中…" : displayName}</h1>
          <p className="flex flex-wrap items-center gap-x-2">
            <span>{profile.name}</span>
            <span aria-hidden="true">·</span>
            <span>External Sampling MCCFR</span>
            {job?.startedAt ? (
              <>
                <span aria-hidden="true">·</span>
                <span>{new Date(job.startedAt).toLocaleString("ja-JP")}</span>
              </>
            ) : null}
          </p>
        </div>
        <div className="flex items-center gap-2">
          {job ? (
            <Badge
              variant={statusBadgeVariant(job.state)}
              className={statusBadgeClass(job.state)}
            >
              {job.state}
            </Badge>
          ) : null}
          <Button
            variant="outline"
            size="sm"
            disabled={
              !job ||
              !["queued", "validating", "running"].includes(job.state) ||
              cancelling
            }
            onClick={cancel}
          >
            {cancelling ? (
              <IconLoader2 className="animate-spin" />
            ) : (
              <IconSquare />
            )}
            停止
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="最新状態へ更新"
            disabled={refreshing}
            onClick={() => void refresh()}
          >
            <IconRefresh className={refreshing ? "animate-spin" : undefined} />
          </Button>
        </div>
      </div>

      {pollError ? (
        <Alert variant="destructive">
          <IconAlertTriangle />
          <AlertTitle>jobの状態を取得できません</AlertTitle>
          <AlertDescription>{pollError}</AlertDescription>
        </Alert>
      ) : null}

      {job?.error ? (
        <Alert variant="destructive">
          <IconAlertTriangle />
          <AlertTitle>Solveに失敗しました</AlertTitle>
          <AlertDescription>{job.error.message}</AlertDescription>
        </Alert>
      ) : null}

      {actionError ? (
        <Alert variant="destructive">
          <IconAlertTriangle />
          <AlertTitle>停止操作を完了できませんでした</AlertTitle>
          <AlertDescription>{actionError}</AlertDescription>
        </Alert>
      ) : null}

      <Card size="sm" className="overflow-visible">
        <CardContent className="solve-status-strip">
          <div className="solve-primary-metric">
            <span>Measured deviation · 95% CI upper</span>
            <strong>
              {deviation === null ? "—" : deviation.toFixed(4)}
              {deviation !== null && progress ? (
                <em>{progress.stopTargetUnit}</em>
              ) : null}
            </strong>
            <small>
              <IconActivity />
              {progress ? (
                <>
                  target {progress.stopTarget} {progress.stopTargetUnit}
                  {deviation !== null && Number(progress.stopTarget) > 0
                    ? ` · target比 ${(deviation / Number(progress.stopTarget)).toFixed(1)}×`
                    : null}
                </>
              ) : (
                "—"
              )}
            </small>
          </div>

          <div className="metric-tile">
            <span>Sweeps</span>
            <strong>
              {progress ? formatInteger(progress.sweeps) : "—"} /{" "}
              {progress ? formatInteger(progress.maxSweeps) : "—"}
            </strong>
            <Progress
              className="metric-tile-progress"
              value={percent}
              aria-label="sweep上限の消化率"
            />
            <small>
              {percent.toFixed(1)}% · 上限消化率（収束確率ではない）
            </small>
          </div>

          <div className="metric-tile">
            <span>Elapsed</span>
            <strong>{formatElapsed(progress?.elapsedSecs ?? null)}</strong>
            <small>
              <IconClock />
              {job && isActive(job.state) && remainingSecs !== null
                ? `残り ${formatElapsed(String(remainingSecs))}（sweep budget基準）`
                : "wall clock"}
            </small>
          </div>

          <div className="metric-tile">
            <span>Resources</span>
            <strong>{formatBytes(progress?.memoryBytes ?? null)}</strong>
            <small>
              <IconCpu />{" "}
              {progress?.handUpdatesPerSecond !== null &&
              progress?.handUpdatesPerSecond !== undefined
                ? `${formatRate(progress.handUpdatesPerSecond)} updates/s`
                : "—"}
            </small>
            {progress?.infosets !== null && progress?.infosets !== undefined ? (
              <small>{formatInteger(progress.infosets)} infosets</small>
            ) : null}
          </div>
        </CardContent>
      </Card>

      <div className="solve-dashboard">
        <Card size="sm">
          <CardHeader className="border-b">
            <CardTitle>品質推移</CardTitle>
            <CardDescription>
              seatごとのdeviation gain 95% CI upper boundの最大値
              {progress ? `（${progress.stopTargetUnit}）` : ""}
            </CardDescription>
            <CardAction>
              <ToggleGroup
                size="sm"
                value={[effectiveScale]}
                onValueChange={(next) => {
                  const value = next[0]
                  if (!value) {
                    return
                  }
                  setChartScale(value as "log" | "linear")
                }}
              >
                <ToggleGroupItem value="log" disabled={!canUseLog}>
                  Log
                </ToggleGroupItem>
                <ToggleGroupItem value="linear">Linear</ToggleGroupItem>
              </ToggleGroup>
            </CardAction>
          </CardHeader>
          <CardContent>
            {chartData.length ? (
              <ChartContainer
                config={chartConfig}
                className="h-[132px] w-full"
                initialDimension={{ width: 680, height: 132 }}
              >
                <AreaChart
                  data={chartData}
                  margin={{ left: -12, right: 12, top: 8, bottom: -2 }}
                  accessibilityLayer
                >
                  <CartesianGrid vertical={false} strokeDasharray="3 3" />
                  <XAxis
                    dataKey="sweeps"
                    axisLine={false}
                    tickLine={false}
                    minTickGap={48}
                    tickFormatter={(value: string) => formatInteger(value)}
                  />
                  <YAxis
                    axisLine={false}
                    tickLine={false}
                    scale={effectiveScale}
                    domain={
                      effectiveScale === "log" ? ["auto", "auto"] : [0, "auto"]
                    }
                    tickFormatter={(value: number) =>
                      effectiveScale === "log"
                        ? Number(value.toPrecision(2)).toString()
                        : value.toFixed(3)
                    }
                  />
                  <ChartTooltip
                    cursor={false}
                    content={<ChartTooltipContent />}
                  />
                  {Number.isFinite(target) &&
                  (effectiveScale === "linear" || target > 0) ? (
                    <ReferenceLine
                      y={target}
                      stroke="#f59e0b"
                      strokeDasharray="4 3"
                      label={{ value: "target", position: "insideTopRight" }}
                    />
                  ) : null}
                  <Area
                    type="monotone"
                    dataKey="deviation"
                    stroke="var(--color-deviation)"
                    strokeWidth={2}
                    fill="var(--color-deviation)"
                    fillOpacity={0.12}
                    dot={{ r: 2.5, fill: "var(--color-deviation)" }}
                    activeDot={{ r: 4 }}
                  />
                </AreaChart>
              </ChartContainer>
            ) : (
              <div className="flex h-[132px] items-center justify-center rounded-lg border border-dashed text-xs text-muted-foreground">
                最初の評価boundaryを待っています。
              </div>
            )}
          </CardContent>
        </Card>

        <Card size="sm">
          <CardHeader className="border-b">
            <CardTitle>実行状況</CardTitle>
            <CardDescription>progress.jsonlの最新event</CardDescription>
            <CardAction>
              <Badge variant="outline">{events.length} events</Badge>
            </CardAction>
          </CardHeader>
          <CardContent className="space-y-2">
            <div className="flex items-center justify-between rounded-md border bg-muted/25 px-2 py-1.5">
              <div className="flex items-center gap-2.5">
                <span className="rounded-md border bg-background p-1.5">
                  <IconCloudDownload className="size-4" />
                </span>
                <div>
                  <p className="text-xs font-medium">Checkpoint</p>
                  <p className="text-[10px] text-muted-foreground">
                    {progress?.checkpoint.available
                      ? "利用可能"
                      : "まだ生成されていません"}
                  </p>
                </div>
              </div>
              <Badge variant="secondary">
                {progress?.checkpoint.available ? "saved" : "waiting"}
              </Badge>
            </div>

            <div className="event-list">
              {events
                .slice(-4)
                .reverse()
                .map((event) => (
                  <div className="event-row" key={event.sequence}>
                    <span className="event-marker" />
                    <time>#{event.sequence}</time>
                    <div>
                      <strong>{event.kind}</strong>
                      <small>
                        {event.progress
                          ? `${formatInteger(event.progress.sweeps)} sweeps`
                          : event.state}
                      </small>
                    </div>
                  </div>
                ))}
            </div>
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader className="border-b">
          <CardTitle className="flex items-center gap-2">
            Preflop Tree
            {strategy ? (
              <Badge
                variant={strategy.status === "stale" ? "secondary" : "default"}
              >
                {strategy.status.toUpperCase()}
              </Badge>
            ) : null}
          </CardTitle>
          <CardDescription>
            {strategy
              ? `Linear average · revision ${strategy.revision} · as of ${formatInteger(strategy.asOfSweeps)} sweeps · ${strategy.node.street}`
              : (strategyMessage ?? "Root Nodeのlive snapshotを取得しています")}
          </CardDescription>
          {strategy ? (
            <CardAction>
              <Badge variant="outline">
                coverage {(Number(strategy.coverage) * 100).toFixed(1)}%
              </Badge>
            </CardAction>
          ) : null}
        </CardHeader>
        <CardContent className="p-0">
          {strategy ? (
            <>
              <div className="flex flex-wrap items-center justify-between gap-3 border-b px-4 py-2">
                <div className="node-breadcrumb" aria-label="現在のnode">
                  <button
                    className="font-semibold hover:underline disabled:no-underline"
                    disabled={strategy.node.nodeId === ROOT_NODE_ID}
                    onClick={() => void navigateToNode(ROOT_NODE_ID)}
                    type="button"
                  >
                    Root
                  </button>
                  {strategy.node.breadcrumb.map((item) => (
                    <button
                      className="hover:underline"
                      key={item.nodeId}
                      onClick={() => void navigateToNode(item.nodeId)}
                      type="button"
                    >
                      / S{item.actorSeat} {item.actionLabel}
                    </button>
                  ))}
                </div>
                <span className="text-xs text-muted-foreground">
                  Actor S{strategy.node.actorSeat}
                  {strategy.node.potMilliBb === null
                    ? ""
                    : ` · Pot ${(Number(strategy.node.potMilliBb) / 1_000).toFixed(3)} BB`}
                </span>
              </div>
              <div className="strategy-workbench">
                <div className="min-w-0 p-4">
                  <div className="mb-3 flex flex-wrap items-center gap-3">
                    {strategy.actions.map((action, index) => (
                      <Button
                        className="h-7 gap-1.5 px-2 text-xs"
                        disabled={action.childNodeId === null}
                        key={action.id}
                        onClick={() =>
                          action.childNodeId
                            ? void navigateToNode(action.childNodeId)
                            : undefined
                        }
                        size="sm"
                        title={
                          action.destination === "postflop"
                            ? "Postflop NodeはSolve中の閲覧対象外です"
                            : action.destination === "terminal"
                              ? "Terminal action"
                              : "次のPreflop Nodeを開く"
                        }
                        variant="outline"
                      >
                        <i
                          className="size-2 rounded-full"
                          style={{
                            backgroundColor: actionColors[index],
                          }}
                        />
                        {action.label}
                        {action.destination === "postflop"
                          ? " · Postflop"
                          : action.destination === "terminal"
                            ? " · Terminal"
                            : ""}
                      </Button>
                    ))}
                  </div>
                  {strategy.view.kind === "preflop-hand-classes" ? (
                    <StrategyMatrix
                      entries={strategy.view.entries}
                      actions={strategy.actions}
                      selectedId={selectedEntry?.id ?? null}
                      onSelect={setSelectedEntry}
                    />
                  ) : (
                    <StrategyBucketList
                      entries={strategy.view.entries}
                      actions={strategy.actions}
                      selectedId={selectedEntry?.id ?? null}
                      onSelect={setSelectedEntry}
                    />
                  )}
                </div>
                <aside className="strategy-detail">
                  <ActionBreakdown
                    entry={selectedEntry}
                    actions={strategy.actions}
                    showEv={false}
                  />
                  <Separator />
                  <div className="space-y-2">
                    {[
                      `Node ID · ${strategy.node.nodeId}`,
                      `Snapshot · ${strategy.generatedAt}`,
                      "Unvisited values remain null",
                    ].map((item) => (
                      <p
                        className="flex items-center gap-2 text-xs text-muted-foreground"
                        key={item}
                      >
                        <IconCheck className="size-3.5 text-emerald-600" />
                        {item}
                      </p>
                    ))}
                  </div>
                </aside>
              </div>
            </>
          ) : (
            <div className="flex min-h-56 items-center justify-center p-6 text-sm text-muted-foreground">
              {loading ? (
                <IconLoader2 className="mr-2 animate-spin" />
              ) : (
                <IconServer className="mr-2" />
              )}
              {strategyMessage ?? "strategy snapshotはまだありません。"}
            </div>
          )}
        </CardContent>
      </Card>

      <div className="flex justify-end">
        <Button variant="outline" onClick={onViewResults} disabled={!terminal}>
          結果を表示
          <IconArrowRight />
        </Button>
      </div>
    </div>
  )
}
