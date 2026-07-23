import { useState } from "react"
import {
  IconArrowDown,
  IconArrowsDiff,
  IconChartBar,
  IconChevronRight,
  IconDownload,
  IconFileAnalytics,
  IconInfoCircle,
  IconSearch,
  IconShare,
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
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Separator } from "@/components/ui/separator"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { demoSession, nodeRows, seatRows, strategyCells } from "@/data/demo"
import type {
  ConnectionProfileViewModel,
  StrategyCellViewModel,
} from "@/lib/solve-contract"
import { cn } from "@/lib/utils"

type ResultsScreenProps = {
  profile: ConnectionProfileViewModel
  runId: string
}

export function ResultsScreen({ profile, runId }: ResultsScreenProps) {
  const [selectedCell, setSelectedCell] = useState<StrategyCellViewModel>(
    strategyCells[0]
  )
  const [activeNode, setActiveNode] = useState("Root")

  return (
    <div className="screen-stack">
      <div className="screen-heading">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant="secondary">SWEEP LIMIT</Badge>
            <span className="eyebrow">RUN #{runId}</span>
          </div>
          <h1>{demoSession.name}</h1>
          <p>
            {demoSession.finishedAt} · sweep-limit · 3h 58m · {profile.name}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled
            title="GUI fixtureでは比較データを読み込みません"
          >
            <IconArrowsDiff />
            比較
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled
            title="GUI fixtureでは共有しません"
          >
            <IconShare />
            共有
          </Button>
          <Button
            size="sm"
            disabled
            title="GUI fixtureではファイルを書き出しません"
          >
            <IconDownload />
            エクスポート
          </Button>
        </div>
      </div>

      <Alert>
        <IconInfoCircle />
        <AlertTitle>GUI fixture</AlertTitle>
        <AlertDescription>
          {profile.kind === "remote"
            ? `${profile.name}とは通信していません。表示値はリモート結果画面を確認するためのサンプルです。`
            : "表示値は結果閲覧画面を確認するためのサンプルです。Solve成果物の読み込みやファイル操作は行いません。"}
        </AlertDescription>
      </Alert>

      <div className="result-summary-grid">
        <Card>
          <CardContent className="metric-card">
            <span>Stop status</span>
            <strong>Sweep limit</strong>
            <small>5,000,000 sweeps</small>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="metric-card">
            <span>Measured deviation</span>
            <strong>0.061</strong>
            <small>target 0.050 BB / hand</small>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="metric-card">
            <span>Final evaluation</span>
            <strong>8,192</strong>
            <small>samples · 95% one-sided CI</small>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="metric-card">
            <span>Strategy coverage</span>
            <strong>96.8%</strong>
            <small>visited infoset weight</small>
          </CardContent>
        </Card>
      </div>

      <Alert>
        <IconInfoCircle />
        <AlertTitle>結果の読み方</AlertTitle>
        <AlertDescription>
          これはExternal Sampling MCCFRのaverage
          profileです。3人以上では認証済みNash/GTO解ではなく、表示値は探索できたdeviationに対する統計評価です。
        </AlertDescription>
      </Alert>

      <Tabs defaultValue="strategy" className="gap-4">
        <TabsList
          variant="line"
          className="w-full max-w-full justify-start overflow-x-auto border-b"
        >
          <TabsTrigger value="strategy">Strategy explorer</TabsTrigger>
          <TabsTrigger value="seats">Seat EV</TabsTrigger>
          <TabsTrigger value="quality">Quality</TabsTrigger>
          <TabsTrigger value="config">Effective config</TabsTrigger>
        </TabsList>

        <TabsContent value="strategy">
          <div className="results-explorer">
            <Card className="results-tree">
              <CardHeader className="border-b">
                <CardTitle>Public tree</CardTitle>
                <CardDescription>1,284 decision nodes</CardDescription>
              </CardHeader>
              <CardContent className="space-y-3 p-3">
                <div className="relative">
                  <IconSearch className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    className="pl-8"
                    placeholder="Actionを検索"
                    aria-label="Actionを検索"
                  />
                </div>
                <div className="tree-list">
                  {nodeRows.map((node, index) => (
                    <button
                      type="button"
                      key={node.path}
                      className={cn(
                        "tree-node",
                        activeNode === node.path && "tree-node--active"
                      )}
                      style={{ paddingLeft: `${10 + index * 10}px` }}
                      onClick={() => setActiveNode(node.path)}
                      aria-pressed={activeNode === node.path}
                    >
                      {index > 0 ? (
                        <span className="tree-branch" aria-hidden="true" />
                      ) : null}
                      <span className="min-w-0 flex-1">
                        <strong>{node.path}</strong>
                        <small>
                          {node.actor} · {node.pot}
                        </small>
                      </span>
                      <IconChevronRight className="size-3.5 shrink-0" />
                    </button>
                  ))}
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  className="w-full"
                  disabled
                  title="GUI fixtureでは追加nodeを読み込みません"
                >
                  さらに表示
                  <IconArrowDown />
                </Button>
              </CardContent>
            </Card>

            <Card className="min-w-0">
              <CardHeader className="border-b">
                <CardTitle>{activeNode}</CardTitle>
                <CardDescription>
                  Pot 1.5 BB · Actor Seat 3 (UTG) · 169 hands
                </CardDescription>
                <CardAction className="flex items-center gap-2">
                  <Select defaultValue="3">
                    <SelectTrigger aria-label="Actor seat">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="3">Seat 3 · UTG</SelectItem>
                      <SelectItem value="4">Seat 4 · HJ</SelectItem>
                      <SelectItem value="5">Seat 5 · CO</SelectItem>
                    </SelectContent>
                  </Select>
                  <Select defaultValue="strategy">
                    <SelectTrigger aria-label="表示指標">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="strategy">Strategy</SelectItem>
                      <SelectItem value="range">Reach range</SelectItem>
                      <SelectItem value="ev">EV</SelectItem>
                    </SelectContent>
                  </Select>
                </CardAction>
              </CardHeader>
              <CardContent className="p-4">
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
                    Linear average · u16
                  </span>
                </div>
                <StrategyMatrix
                  cells={strategyCells}
                  selectedHand={selectedCell.hand}
                  onSelect={setSelectedCell}
                />
              </CardContent>
            </Card>

            <Card className="results-detail">
              <CardHeader className="border-b">
                <CardTitle>Hand details</CardTitle>
                <CardDescription>選択したhand class</CardDescription>
              </CardHeader>
              <CardContent className="space-y-5">
                <ActionBreakdown cell={selectedCell} />
                <Separator />
                <div className="space-y-3">
                  <p className="text-xs font-medium">Range contribution</p>
                  <div className="grid grid-cols-2 gap-2">
                    <div className="rounded-md bg-muted/60 p-2">
                      <span className="text-[10px] text-muted-foreground uppercase">
                        Weight
                      </span>
                      <p className="font-mono text-sm font-semibold">1.000</p>
                    </div>
                    <div className="rounded-md bg-muted/60 p-2">
                      <span className="text-[10px] text-muted-foreground uppercase">
                        Reach
                      </span>
                      <p className="font-mono text-sm font-semibold">0.842</p>
                    </div>
                  </div>
                </div>
              </CardContent>
            </Card>
          </div>
        </TabsContent>

        <TabsContent value="seats">
          <Card>
            <CardHeader className="border-b">
              <CardTitle>Seat economics</CardTitle>
              <CardDescription>
                実カードsettlement · main / side pot · no rake
              </CardDescription>
              <CardAction>
                <Button
                  variant="outline"
                  size="sm"
                  disabled
                  title="GUI fixtureではCSVを書き出しません"
                >
                  <IconFileAnalytics />
                  CSV
                </Button>
              </CardAction>
            </CardHeader>
            <CardContent className="p-0">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Seat</TableHead>
                    <TableHead>Position</TableHead>
                    <TableHead>Starting stack</TableHead>
                    <TableHead className="text-right">EV (BB / hand)</TableHead>
                    <TableHead className="text-right">95% CI</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {seatRows.map((row) => (
                    <TableRow key={row.seat}>
                      <TableCell className="font-medium">
                        Seat {row.seat}
                      </TableCell>
                      <TableCell>{row.position}</TableCell>
                      <TableCell>{row.stack.toFixed(3)} BB</TableCell>
                      <TableCell
                        className={cn(
                          "text-right font-mono font-medium",
                          row.ev >= 0 ? "text-emerald-700" : "text-red-600"
                        )}
                      >
                        {row.ev >= 0 ? "+" : ""}
                        {row.ev.toFixed(2)}
                      </TableCell>
                      <TableCell className="text-right font-mono">
                        {row.ci}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="quality">
          <div className="grid gap-4 lg:grid-cols-2">
            <Card>
              <CardHeader className="border-b">
                <CardTitle>Stopping evaluation</CardTitle>
                <CardDescription>Final three confirmations</CardDescription>
              </CardHeader>
              <CardContent className="space-y-3">
                {[
                  ["Evaluation #498", "0.068", "not met"],
                  ["Evaluation #499", "0.064", "not met"],
                  ["Evaluation #500", "0.061", "not met"],
                ].map(([label, value, status]) => (
                  <div
                    className="flex items-center justify-between rounded-lg border p-3"
                    key={label}
                  >
                    <div>
                      <p className="font-medium">{label}</p>
                      <p className="text-xs text-muted-foreground">
                        8,192 samples · {status}
                      </p>
                    </div>
                    <span className="font-mono text-base font-semibold">
                      {value}
                    </span>
                  </div>
                ))}
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="border-b">
                <CardTitle>Artifact integrity preview</CardTitle>
                <CardDescription>
                  solution.mwsol v4 · 未検証の表示サンプル
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-3">
                {[
                  "Effective config embedded",
                  "Game / algorithm fingerprints match",
                  "Probability distributions sum to 65,535",
                  "Unvisited infosets remain explicit",
                  "Atomic artifact write verified",
                ].map((item) => (
                  <p className="flex items-center gap-2" key={item}>
                    <IconInfoCircle className="size-4 text-muted-foreground" />
                    {item}
                  </p>
                ))}
              </CardContent>
            </Card>
          </div>
        </TabsContent>

        <TabsContent value="config">
          <Card>
            <CardHeader className="border-b">
              <CardTitle>Effective config</CardTitle>
              <CardDescription>
                defaultsと導出値を展開した再parse可能な設定
              </CardDescription>
              <CardAction>
                <Button
                  variant="outline"
                  size="sm"
                  disabled
                  title="GUI fixtureではTOMLを書き出しません"
                >
                  <IconDownload />
                  TOML
                </Button>
              </CardAction>
            </CardHeader>
            <CardContent>
              <pre className="config-preview">
                {`schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 6
button = 0
standard_blinds = true

[game.defaults]
stack_bb = 100.000
range = "random"

[game.tree]
kind = "standard"

[solver]
kind = "range-vector"

[run]
max_sweeps = 5000000`}
              </pre>
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>

      <div className="flex items-center justify-between rounded-lg border bg-muted/30 p-3">
        <div className="flex items-center gap-2">
          <span className="rounded-md border bg-background p-2">
            <IconChartBar className="size-4" />
          </span>
          <div>
            <p className="text-xs font-medium">次の分析</p>
            <p className="text-xs text-muted-foreground">
              同一game fingerprintのsolutionと戦略・EV・品質を比較できます。
            </p>
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled
          title="GUI fixtureでは比較対象を読み込みません"
        >
          比較対象を開く
        </Button>
      </div>
    </div>
  )
}
