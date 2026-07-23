import { useState } from "react"
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
  IconDotsVertical,
  IconPlayerPause,
  IconRefresh,
  IconRoute,
  IconServer,
  IconSquare,
} from "@tabler/icons-react"

import { ActionBreakdown, StrategyMatrix } from "@/components/strategy-matrix"
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Separator } from "@/components/ui/separator"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  convergenceData,
  demoProgress,
  demoSession,
  demoSnapshot,
  nodeRows,
  solveEvents,
  strategyCells,
} from "@/data/demo"
import type {
  ConnectionProfileViewModel,
  StrategyCellViewModel,
} from "@/lib/solve-contract"

const chartConfig = {
  deviation: {
    label: "Measured deviation",
    color: "#18181b",
  },
  target: {
    label: "Target",
    color: "#16a34a",
  },
} satisfies ChartConfig

type SolveScreenProps = {
  profile: ConnectionProfileViewModel
  onViewResults: () => void
}

function formatInteger(value: number) {
  return new Intl.NumberFormat("ja-JP").format(value)
}

export function SolveScreen({ profile, onViewResults }: SolveScreenProps) {
  const [selectedCell, setSelectedCell] = useState<StrategyCellViewModel>(
    strategyCells.find((cell) => cell.hand === "AKs") ?? strategyCells[1]
  )
  const progress =
    (demoProgress.sweep / Math.max(1, demoProgress.maxSweeps)) * 100
  const isRemote = profile.kind === "remote"

  return (
    <div className="screen-stack">
      <div className="screen-heading">
        <div>
          <div className="flex items-center gap-2">
            <span className="live-pulse" />
            <span className="eyebrow">
              SOLVING DEMO · RUN #{demoSession.id}
            </span>
          </div>
          <h1>{demoSession.name}</h1>
          <p className="flex flex-wrap items-center gap-x-2">
            <span>{profile.name}</span>
            <span aria-hidden="true">·</span>
            <span>External Sampling MCCFR</span>
            <span aria-hidden="true">·</span>
            <span>開始 09:47</span>
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled
            title="Solve adapter接続後に利用できます"
          >
            <IconPlayerPause />
            一時停止
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled
            title="Solve adapter接続後に利用できます"
          >
            <IconSquare />
            停止して保存
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="その他の操作（未接続）"
            disabled
          >
            <IconDotsVertical />
          </Button>
        </div>
      </div>

      <Alert className="border-amber-300 bg-amber-50 text-amber-950">
        <IconAlertTriangle />
        <AlertTitle>操作可能なSolve中画面のfixtureです</AlertTitle>
        <AlertDescription className="text-amber-800">
          進捗・イベント・戦略はすべて画面確認用です。
          {isRemote
            ? `${profile.endpoint} とは通信していません。`
            : "ローカルSolveエンジンはまだ呼び出していません。"}
        </AlertDescription>
      </Alert>

      <Card className="overflow-visible">
        <CardContent className="grid gap-5 md:grid-cols-[minmax(0,1.6fr)_repeat(3,minmax(120px,0.55fr))]">
          <div className="space-y-3">
            <div className="flex items-end justify-between gap-4">
              <div>
                <p className="text-xs text-muted-foreground">
                  Sweep limit consumption
                </p>
                <p className="text-3xl font-semibold tracking-tight">
                  {progress.toFixed(1)}
                  <span className="ml-1 text-base text-muted-foreground">
                    %
                  </span>
                </p>
              </div>
              <p className="text-right font-mono text-xs text-muted-foreground">
                {formatInteger(demoProgress.sweep)}
                <br />
                <span>/ {formatInteger(demoProgress.maxSweeps)} sweeps</span>
              </p>
            </div>
            <Progress value={progress} aria-label="Solve進捗" />
          </div>

          <div className="metric-tile">
            <span>Elapsed</span>
            <strong>2h 43m</strong>
            <small>
              <IconClock /> 参考残り 1h 18m
            </small>
          </div>
          <div className="metric-tile">
            <span>Deviation</span>
            <strong>0.074</strong>
            <small className="text-emerald-700">
              <IconActivity /> target 0.050
            </small>
          </div>
          <div className="metric-tile">
            <span>Resources</span>
            <strong>{isRemote ? "未確認" : "9.2 GiB"}</strong>
            <small>
              {isRemote ? <IconServer /> : <IconCpu />}{" "}
              {isRemote ? "handshake後に表示" : "12 threads · fixture"}
            </small>
          </div>
        </CardContent>
        <p className="border-t px-5 py-2 text-[11px] text-muted-foreground">
          進捗率と参考残り時間はsweep上限とfixture
          throughputからの算出であり、収束確率・収束予測ではありません。
        </p>
      </Card>

      <div className="solve-dashboard">
        <Card>
          <CardHeader className="border-b">
            <CardTitle>収束</CardTitle>
            <CardDescription>
              Found deviation gainのone-sided 95% CI upper bound
            </CardDescription>
            <CardAction>
              <Select defaultValue="full">
                <SelectTrigger aria-label="収束グラフの表示期間">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="full">全期間</SelectItem>
                  <SelectItem value="hour">直近1時間</SelectItem>
                  <SelectItem value="checks">直近20評価</SelectItem>
                </SelectContent>
              </Select>
            </CardAction>
          </CardHeader>
          <CardContent>
            <ChartContainer
              config={chartConfig}
              className="h-[220px] w-full"
              initialDimension={{ width: 680, height: 220 }}
            >
              <AreaChart
                data={convergenceData}
                margin={{ left: -12, right: 12, top: 10, bottom: 0 }}
                accessibilityLayer
              >
                <defs>
                  <linearGradient
                    id="deviation-fill"
                    x1="0"
                    y1="0"
                    x2="0"
                    y2="1"
                  >
                    <stop offset="5%" stopColor="#18181b" stopOpacity={0.22} />
                    <stop offset="95%" stopColor="#18181b" stopOpacity={0} />
                  </linearGradient>
                </defs>
                <CartesianGrid vertical={false} strokeDasharray="3 3" />
                <XAxis
                  dataKey="sweep"
                  axisLine={false}
                  tickLine={false}
                  tickMargin={8}
                  tickFormatter={(value: number) => `${value}M`}
                />
                <YAxis
                  domain={[0, 0.45]}
                  axisLine={false}
                  tickLine={false}
                  tickMargin={8}
                  tickFormatter={(value: number) => value.toFixed(2)}
                />
                <ChartTooltip
                  cursor={false}
                  content={
                    <ChartTooltipContent
                      labelFormatter={(_, payload) =>
                        `${payload[0]?.payload.sweep ?? 0}M sweeps`
                      }
                    />
                  }
                />
                <ReferenceLine
                  y={0.05}
                  stroke="var(--color-target)"
                  strokeDasharray="5 4"
                />
                <Area
                  type="monotone"
                  dataKey="deviation"
                  stroke="var(--color-deviation)"
                  strokeWidth={2}
                  fill="url(#deviation-fill)"
                  dot={false}
                  activeDot={{ r: 4 }}
                />
              </AreaChart>
            </ChartContainer>
            <div className="mt-2 flex items-center justify-between text-xs text-muted-foreground">
              <span>評価間隔 10,000 sweeps</span>
              <span>target以下を3回連続で終了</span>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="border-b">
            <CardTitle>実行状況</CardTitle>
            <CardDescription>イベントとcheckpoint · fixture</CardDescription>
            <CardAction>
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label="更新（未接続）"
                disabled
              >
                <IconRefresh />
              </Button>
            </CardAction>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center justify-between rounded-lg border bg-muted/25 p-3">
              <div className="flex items-center gap-2.5">
                <span className="rounded-md border bg-background p-2">
                  <IconCloudDownload className="size-4" />
                </span>
                <div>
                  <p className="font-medium">Checkpoint</p>
                  <p className="text-xs text-muted-foreground">
                    3分前 · 1.8 GB
                  </p>
                </div>
              </div>
              <Badge variant="secondary">saved</Badge>
            </div>

            <div className="event-list">
              {solveEvents.map((event) => (
                <div className="event-row" key={`${event.time}-${event.title}`}>
                  <span className="event-marker" />
                  <time>{event.time}</time>
                  <div>
                    <strong>{event.title}</strong>
                    <small>{event.detail}</small>
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
            現在の戦略
            <Badge className="gap-1 bg-amber-600 text-white hover:bg-amber-600">
              <span className="size-1.5 rounded-full bg-white" />
              DEMO · LIVE AVERAGE
            </Badge>
          </CardTitle>
          <CardDescription>
            atomic snapshot fixture · revision {demoSnapshot.revision} · as of{" "}
            {formatInteger(demoSnapshot.asOfSweeps)} sweeps ·{" "}
            {demoSnapshot.street} · 12:31:42
          </CardDescription>
          <CardAction className="flex flex-wrap items-center justify-end gap-2">
            <Select defaultValue={String(demoSnapshot.node.actor)}>
              <SelectTrigger aria-label="戦略を表示するactor">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="3">Seat 3 · UTG</SelectItem>
                <SelectItem value="4">Seat 4 · HJ</SelectItem>
                <SelectItem value="5">Seat 5 · CO</SelectItem>
                <SelectItem value="0">Seat 0 · BTN</SelectItem>
              </SelectContent>
            </Select>
            <Button
              variant="outline"
              size="sm"
              disabled
              title="Node選択adapterは未接続です"
            >
              <IconRoute />
              Node
            </Button>
          </CardAction>
        </CardHeader>
        <CardContent className="p-0">
          <Tabs defaultValue="strategy">
            <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-2">
              <div className="node-breadcrumb" aria-label="現在のnode">
                <button
                  type="button"
                  disabled
                  title="Node選択adapterは未接続です"
                >
                  Root
                </button>
                <span>/</span>
                <button
                  type="button"
                  disabled
                  title="Node選択adapterは未接続です"
                >
                  UTG 2.5
                </button>
                <span>/</span>
                <strong>HJ decision</strong>
              </div>
              <TabsList variant="line">
                <TabsTrigger value="strategy">戦略</TabsTrigger>
                <TabsTrigger value="tree">Node tree</TabsTrigger>
              </TabsList>
            </div>

            <TabsContent value="strategy" className="strategy-workbench">
              <div className="min-w-0 p-4">
                <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
                  <div className="action-legend">
                    <span>
                      <i className="bg-violet-600" /> Raise 2.5
                    </span>
                    <span>
                      <i className="bg-green-600" /> Call
                    </span>
                    <span>
                      <i className="bg-zinc-300" /> Fold
                    </span>
                  </div>
                  <span className="text-xs text-muted-foreground">
                    クリックして詳細を表示
                  </span>
                </div>
                <StrategyMatrix
                  cells={strategyCells}
                  selectedHand={selectedCell.hand}
                  onSelect={setSelectedCell}
                />
              </div>
              <aside className="strategy-detail">
                <ActionBreakdown cell={selectedCell} />
                <Separator />
                <div className="space-y-2">
                  <p className="text-xs font-medium">Snapshot integrity</p>
                  {[
                    "Complete sweep only",
                    `Node ID · ${demoSnapshot.node.id}`,
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
            </TabsContent>

            <TabsContent value="tree" className="p-4">
              <div className="divide-y rounded-lg border">
                {nodeRows.map((node) => (
                  <button
                    type="button"
                    className="flex w-full items-center gap-3 px-3 py-2.5 text-left hover:bg-muted/50"
                    key={node.path}
                    disabled
                    title="Node選択adapterは未接続です"
                  >
                    <span className="size-1.5 rounded-full bg-zinc-400" />
                    <span className="flex-1">{node.path}</span>
                    <span className="text-xs text-muted-foreground">
                      {node.actor} · {node.pot}
                    </span>
                    {node.active ? <Badge>current</Badge> : null}
                  </button>
                ))}
              </div>
            </TabsContent>
          </Tabs>
        </CardContent>
      </Card>

      <div className="flex justify-end">
        <Button variant="outline" onClick={onViewResults}>
          結果画面のfixtureを見る
          <IconArrowRight />
        </Button>
      </div>
    </div>
  )
}
