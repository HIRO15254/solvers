import { useState } from "react"
import {
  IconArrowRight,
  IconBolt,
  IconCheck,
  IconChevronRight,
  IconCpu,
  IconFileDescription,
  IconInfoCircle,
  IconStack2,
} from "@tabler/icons-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Separator } from "@/components/ui/separator"
import { Switch } from "@/components/ui/switch"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { demoDraft } from "@/data/demo"
import type { ConnectionProfileViewModel } from "@/lib/solve-contract"
import { cn } from "@/lib/utils"

type SetupScreenProps = {
  profile: ConnectionProfileViewModel
  onStart: () => void
  onOpenConnections: () => void
}

const seats = [
  { seat: 0, position: "BTN", className: "seat-0" },
  { seat: 1, position: "SB", className: "seat-1" },
  { seat: 2, position: "BB", className: "seat-2" },
  { seat: 3, position: "UTG", className: "seat-3" },
  { seat: 4, position: "HJ", className: "seat-4" },
  { seat: 5, position: "CO", className: "seat-5" },
]

type SeatDraft = {
  stack: string
  blind: string
  range: string
}

const initialSeatDrafts: SeatDraft[] = seats.map((seat) => ({
  stack: "100.000",
  blind: seat.seat === 1 ? "0.500" : seat.seat === 2 ? "1.000" : "0.000",
  range: seat.seat === 3 ? "22+,A2s+,K9s+,QTs+,JTs,ATo+,KQo" : "random",
}))

export function SetupScreen({
  profile,
  onStart,
  onOpenConnections,
}: SetupScreenProps) {
  const [activeSeat, setActiveSeat] = useState(3)
  const [seatDrafts, setSeatDrafts] = useState<SeatDraft[]>(initialSeatDrafts)
  const [rakeEnabled, setRakeEnabled] = useState(false)
  const isRemote = profile.kind === "remote"
  const activeSeatDraft = seatDrafts[activeSeat] ?? initialSeatDrafts[0]

  const updateActiveSeat = (field: keyof SeatDraft, value: string) => {
    setSeatDrafts((current) =>
      current.map((seat, index) =>
        index === activeSeat ? { ...seat, [field]: value } : seat
      )
    )
  }

  return (
    <div className="screen-stack">
      <div className="screen-heading">
        <div>
          <div className="eyebrow">NEW SOLVE</div>
          <h1>Solve設定を作成</h1>
          <p>
            テーブル、ツリー、精度と実行先をひとつの再現可能な設定へまとめます。
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled
            title="GUI fixtureではファイルを読み込みません"
          >
            <IconFileDescription />
            TOMLを読み込む
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled
            title="GUI fixtureではファイルを保存しません"
          >
            下書きを保存
          </Button>
        </div>
      </div>

      <Alert>
        <IconInfoCircle />
        <AlertTitle>GUI fixture</AlertTitle>
        <AlertDescription>
          入力・進捗・結果は画面確認用のサンプルです。設定検証、Solve実行、ファイル入出力は行いません。
        </AlertDescription>
      </Alert>

      {isRemote ? (
        <Alert className="border-amber-300 bg-amber-50 text-amber-950">
          <IconInfoCircle />
          <AlertTitle>リモートSolveは契約プレビューです</AlertTitle>
          <AlertDescription className="text-amber-800">
            この画面では設定と実行フローを確認できますが、今回の実装はネットワークへジョブを送信しません。
          </AlertDescription>
        </Alert>
      ) : null}

      <div className="setup-layout">
        <div className="min-w-0 space-y-4">
          <Card>
            <CardHeader className="border-b">
              <CardTitle>開始点</CardTitle>
              <CardDescription>
                実戦的な初期値から始め、必要な差分だけを編集します。
              </CardDescription>
              <CardAction>
                <Badge variant="secondary">Multiway v1</Badge>
              </CardAction>
            </CardHeader>
            <CardContent className="grid gap-4 pt-0 md:grid-cols-3">
              <div className="field-stack md:col-span-2">
                <Label htmlFor="solve-name">Solve名</Label>
                <Input id="solve-name" defaultValue={demoDraft.name} />
              </div>
              <div className="field-stack">
                <Label>プリセット</Label>
                <Select defaultValue="cash-6max">
                  <SelectTrigger className="w-full" aria-label="プリセット">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="cash-6max">
                      6-max Cash · 100BB
                    </SelectItem>
                    <SelectItem value="mtt-9max">9-max MTT · ICM</SelectItem>
                    <SelectItem value="push-fold">9-max Push / Fold</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="border-b">
              <CardTitle>テーブルとレンジ</CardTitle>
              <CardDescription>
                seat IDはbuttonから時計回り。クリックして個別設定を編集します。
              </CardDescription>
              <CardAction>
                <Select defaultValue="6">
                  <SelectTrigger aria-label="Seat数">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="2">2 seats</SelectItem>
                    <SelectItem value="6">6 seats</SelectItem>
                    <SelectItem value="9">9 seats</SelectItem>
                  </SelectContent>
                </Select>
              </CardAction>
            </CardHeader>
            <CardContent className="table-editor">
              <div className="seat-map" role="group" aria-label="6 seat table">
                <div className="poker-table">
                  <span>6-MAX</span>
                  <small>0.5 / 1 BB</small>
                </div>
                {seats.map((seat) => (
                  <button
                    type="button"
                    className={cn(
                      "seat-chip",
                      seat.className,
                      activeSeat === seat.seat && "seat-chip--active"
                    )}
                    key={seat.seat}
                    onClick={() => setActiveSeat(seat.seat)}
                    aria-pressed={activeSeat === seat.seat}
                  >
                    <span>
                      S{seat.seat} · {seat.position}
                    </span>
                    <strong>{seatDrafts[seat.seat]?.stack ?? "—"} BB</strong>
                    <small>{seatDrafts[seat.seat]?.range ?? "—"}</small>
                  </button>
                ))}
              </div>

              <div className="seat-detail">
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <p className="font-medium">
                      Seat {activeSeat} · {seats[activeSeat]?.position}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      defaultから上書き
                    </p>
                  </div>
                  <Badge variant="outline">
                    {activeSeat === 0 ? "Button" : "Player"}
                  </Badge>
                </div>
                <Separator />
                <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-1">
                  <div className="field-stack">
                    <Label htmlFor="stack">Stack (BB)</Label>
                    <Input
                      id="stack"
                      type="number"
                      value={activeSeatDraft.stack}
                      onChange={(event) =>
                        updateActiveSeat("stack", event.target.value)
                      }
                    />
                  </div>
                  <div className="field-stack">
                    <Label htmlFor="blind">Live blind (BB)</Label>
                    <Input
                      id="blind"
                      type="number"
                      value={activeSeatDraft.blind}
                      onChange={(event) =>
                        updateActiveSeat("blind", event.target.value)
                      }
                    />
                  </div>
                </div>
                <div className="field-stack">
                  <div className="flex items-center justify-between">
                    <Label htmlFor="range">Range</Label>
                    <Button
                      variant="link"
                      size="xs"
                      disabled
                      title="レンジグリッドはGUI fixtureでは利用できません"
                    >
                      グリッドで編集
                    </Button>
                  </div>
                  <Input
                    id="range"
                    value={activeSeatDraft.range}
                    onChange={(event) =>
                      updateActiveSeat("range", event.target.value)
                    }
                  />
                  <p className="field-help">
                    1,326 comboへ正規化して検証します。
                  </p>
                </div>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="border-b">
              <CardTitle>ゲームとアルゴリズム</CardTitle>
              <CardDescription>
                主要項目を先に表示し、詳細設定は同じ正規化済みTOMLへ出力します。
              </CardDescription>
            </CardHeader>
            <CardContent>
              <Tabs defaultValue="tree">
                <TabsList
                  variant="line"
                  className="w-full max-w-full justify-start overflow-x-auto"
                >
                  <TabsTrigger value="tree">Betting tree</TabsTrigger>
                  <TabsTrigger value="economics">Economics</TabsTrigger>
                  <TabsTrigger value="solver">Solver</TabsTrigger>
                  <TabsTrigger value="runtime">Runtime</TabsTrigger>
                </TabsList>

                <TabsContent
                  value="tree"
                  className="grid gap-4 pt-3 md:grid-cols-2"
                >
                  <div className="field-stack">
                    <Label>Tree frontend</Label>
                    <Select defaultValue="standard">
                      <SelectTrigger
                        className="w-full"
                        aria-label="Tree frontend"
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="standard">
                          Standard · typed rules
                        </SelectItem>
                        <SelectItem value="script">Script · .mwtree</SelectItem>
                      </SelectContent>
                    </Select>
                    <p className="field-help">
                      Open 2.5×、reraise 3×、legal all-inを含む標準ツリー。
                    </p>
                  </div>
                  <div className="rounded-lg border bg-muted/25 p-3">
                    <div className="flex items-center justify-between">
                      <span className="font-medium">Tree preview</span>
                      <Badge variant="secondary">1,284 nodes</Badge>
                    </div>
                    <div className="mt-3 space-y-2 text-xs text-muted-foreground">
                      <p className="flex justify-between">
                        <span>Preflop aggressive cap</span>
                        <strong className="text-foreground">4</strong>
                      </p>
                      <p className="flex justify-between">
                        <span>Postflop bet / raise</span>
                        <strong className="text-foreground">50% / 75%</strong>
                      </p>
                      <p className="flex justify-between">
                        <span>Donk bet</span>
                        <strong className="text-foreground">Allowed</strong>
                      </p>
                    </div>
                  </div>
                </TabsContent>

                <TabsContent
                  value="economics"
                  className="grid gap-4 pt-3 md:grid-cols-2"
                >
                  <div className="field-stack">
                    <Label>Economics</Label>
                    <Select defaultValue="cash">
                      <SelectTrigger className="w-full" aria-label="Economics">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="cash">Cash · chipEV</SelectItem>
                        <SelectItem value="icm">Tournament · ICM</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="flex items-center justify-between rounded-lg border p-3">
                    <div>
                      <p className="font-medium">Rake</p>
                      <p className="text-xs text-muted-foreground">
                        offの場合はno-rake
                      </p>
                    </div>
                    <Switch
                      checked={rakeEnabled}
                      onCheckedChange={setRakeEnabled}
                      aria-label="Rakeを有効化"
                    />
                  </div>
                </TabsContent>

                <TabsContent
                  value="solver"
                  className="grid gap-4 pt-3 md:grid-cols-3"
                >
                  <div className="field-stack">
                    <Label>Algorithm</Label>
                    <Input value="External Sampling MCCFR" readOnly />
                  </div>
                  <div className="field-stack">
                    <Label htmlFor="rollouts">Rollouts / state</Label>
                    <Input id="rollouts" type="number" defaultValue="512" />
                    <p className="field-help">検証済みの標準値</p>
                  </div>
                  <div className="field-stack">
                    <Label>Recall</Label>
                    <Select defaultValue="current">
                      <SelectTrigger className="w-full" aria-label="Recall">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="current">Current street</SelectItem>
                        <SelectItem value="history">Bucket history</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </TabsContent>

                <TabsContent
                  value="runtime"
                  className="grid gap-4 pt-3 md:grid-cols-3"
                >
                  <div className="field-stack">
                    <Label htmlFor="sweeps">Max sweeps</Label>
                    <Input id="sweeps" defaultValue="5,000,000" />
                  </div>
                  <div className="field-stack">
                    <Label htmlFor="target">Stop target</Label>
                    <Input id="target" defaultValue="0.05" />
                    <p className="field-help">BB / hand</p>
                  </div>
                  <div className="field-stack">
                    <Label>Resources</Label>
                    <Select defaultValue="auto">
                      <SelectTrigger className="w-full" aria-label="Resources">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="auto">Auto · recommended</SelectItem>
                        <SelectItem value="manual">Manual</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </TabsContent>
              </Tabs>
            </CardContent>
          </Card>
        </div>

        <aside className="setup-summary">
          <Card className="sticky-card">
            <CardHeader className="border-b">
              <CardTitle>実行サマリー</CardTitle>
              <CardDescription>
                {isRemote
                  ? "Capability未確認 · 送信しません"
                  : "Preflightの表示サンプル"}
              </CardDescription>
              <CardAction>
                <Badge variant="outline">
                  {isRemote ? "未確認" : "FIXTURE"}
                </Badge>
              </CardAction>
            </CardHeader>
            <CardContent className="space-y-4">
              <button
                type="button"
                className="machine-summary"
                onClick={onOpenConnections}
              >
                <span className="rounded-md border bg-background p-2">
                  {isRemote ? (
                    <IconCpu className="size-4" />
                  ) : (
                    <IconBolt className="size-4" />
                  )}
                </span>
                <span className="min-w-0 flex-1 text-left">
                  <small>SOLVE先</small>
                  <strong className="truncate">{profile.name}</strong>
                  <em>
                    {isRemote ? "Remote · contract only" : "Local · fixture"}
                  </em>
                </span>
                <IconChevronRight className="size-4 text-muted-foreground" />
              </button>

              <div className="summary-metrics">
                <div>
                  <span>Threads</span>
                  <strong>{isRemote ? "未確認" : "12 / sample"}</strong>
                </div>
                <div>
                  <span>Peak memory</span>
                  <strong>{isRemote ? "未確認" : "9.6 GiB · sample"}</strong>
                </div>
                <div>
                  <span>Abstraction</span>
                  <strong>64 / 64 / 64</strong>
                </div>
                <div>
                  <span>Estimate</span>
                  <strong>{isRemote ? "未算出" : "~ 4h 03m · sample"}</strong>
                </div>
              </div>

              <Separator />

              <div className="space-y-2.5">
                <p className="text-xs font-medium">
                  {isRemote ? "Remote preflight" : "Validation sample"}
                </p>
                {(isRemote
                  ? [
                      "Capability handshake · 未実行",
                      "Config schema · 未確認",
                      "Resource limits · 未確認",
                      "Remote run · 未作成",
                    ]
                  : [
                      "Config schema v1 · sample",
                      "Ranges · 7,956 combos · sample",
                      "Tree rules · sample",
                      "Memory budget · sample",
                    ]
                ).map((item) => (
                  <p
                    className="flex items-center gap-2 text-xs text-muted-foreground"
                    key={item}
                  >
                    <span
                      className={cn(
                        "rounded-full p-0.5",
                        isRemote
                          ? "bg-muted text-muted-foreground"
                          : "bg-emerald-100 text-emerald-700"
                      )}
                    >
                      {isRemote ? (
                        <IconInfoCircle className="size-3" />
                      ) : (
                        <IconCheck className="size-3" />
                      )}
                    </span>
                    {item}
                  </p>
                ))}
              </div>

              <Alert>
                <IconInfoCircle />
                <AlertTitle>品質の境界</AlertTitle>
                <AlertDescription>
                  3人以上はregret-minimized
                  profileであり、認証済みNash/GTO解ではありません。
                </AlertDescription>
              </Alert>
            </CardContent>
            <CardFooter className="flex-col gap-2 border-t">
              <Button className="w-full" size="lg" onClick={onStart}>
                {isRemote ? "リモート実行デモを表示" : "ローカル実行デモを表示"}
                <IconArrowRight />
              </Button>
              <p className="text-center text-[11px] text-muted-foreground">
                {isRemote
                  ? "remote managed run: 未作成"
                  : "fixture run: ファイルを作成しません"}
              </p>
            </CardFooter>
          </Card>

          <div className="flex items-start gap-2 rounded-lg border border-dashed p-3 text-xs text-muted-foreground">
            <IconStack2 className="mt-0.5 size-4 shrink-0" />
            <p>
              実装時は有効値を含むTOMLとfingerprintを成果物へ記録します。fixtureでは保存しません。
            </p>
          </div>
        </aside>
      </div>
    </div>
  )
}
