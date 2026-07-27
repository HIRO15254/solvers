import { useEffect, useMemo, useState } from "react"
import {
  IconChartBar,
  IconCheck,
  IconDownload,
  IconInfoCircle,
  IconLoader2,
  IconPlayerPlay,
} from "@tabler/icons-react"

import { ResumeDialog, type ResumeSubmission } from "@/components/resume-dialog"
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
import type {
  ConnectionProfileViewModel,
  StrategyEntry,
} from "@/lib/solve-contract"
import type {
  ArtifactKind,
  DesktopSolveGateway,
  LocalStrategySnapshot,
  NativeJobResult,
  NativeJobSnapshot,
  NativeSeatMetrics,
} from "@/lib/native-gateway"
import { errorMessage } from "@/lib/native-gateway"
import { strategyActionColors } from "@/lib/strategy-colors"
import { cn } from "@/lib/utils"
import {
  formatBytes,
  formatDecimal,
  formatElapsed,
  formatInteger,
  formatScientific,
} from "@/lib/format"
import {
  statusBadgeClass,
  statusBadgeVariant,
  terminalStates,
} from "@/lib/job-status"

type ResultsScreenProps = {
  profile: ConnectionProfileViewModel
  gateway: DesktopSolveGateway
  jobId: string
  displayName: string
  onJobResumed: (job: NativeJobSnapshot, name: string) => void
}

const artifactLabels: Record<ArtifactKind, string> = {
  run: "run.json",
  progress: "progress.jsonl",
  solution: "solution.mwsol",
  checkpoint: "checkpoint.mwckpt",
}

export function ResultsScreen({
  profile,
  gateway,
  jobId,
  displayName,
  onJobResumed,
}: ResultsScreenProps) {
  const [job, setJob] = useState<NativeJobSnapshot | null>(null)
  const [result, setResult] = useState<NativeJobResult | null>(null)
  const [strategy, setStrategy] = useState<LocalStrategySnapshot | null>(null)
  const [selectedEntry, setSelectedEntry] = useState<StrategyEntry | null>(null)
  const [loading, setLoading] = useState(true)
  const [busyAction, setBusyAction] = useState<string | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [resultError, setResultError] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const [strategyMessage, setStrategyMessage] = useState<string | null>(null)
  const [receipt, setReceipt] = useState<string | null>(null)
  const [resumeOpen, setResumeOpen] = useState(false)

  useEffect(() => {
    let disposed = false
    let timer: number | undefined
    if (!gateway.availability.available || profile.kind === "remote") {
      return
    }

    const load = async (reset = false) => {
      let pollAgain = false
      if (reset) {
        setLoading(true)
        setJob(null)
        setResult(null)
        setStrategy(null)
        setSelectedEntry(null)
        setLoadError(null)
        setResultError(null)
        setStrategyMessage(null)
      }
      try {
        const nextJob = await gateway.getJob(jobId)
        if (disposed) {
          return
        }
        setJob(nextJob)
        setLoadError(null)
        pollAgain = !terminalStates.has(nextJob.state)

        if (terminalStates.has(nextJob.state) && nextJob.state !== "failed") {
          try {
            const nextResult = await gateway.getResult(jobId)
            if (!disposed) {
              setResult(nextResult)
              setResultError(null)
            }
          } catch (error) {
            if (!disposed) {
              setResultError(errorMessage(error))
            }
          }
        }

        try {
          const nextStrategy = await gateway.getStrategy(jobId)
          if (!disposed) {
            setStrategy(nextStrategy)
            setStrategyMessage(null)
            setSelectedEntry((current) => {
              const entries = nextStrategy.view.entries
              return (
                entries.find((entry) => entry.id === current?.id) ??
                entries.find((entry) => entry.status === "visited") ??
                entries[0] ??
                null
              )
            })
          }
        } catch (error) {
          if (!disposed) {
            setStrategyMessage(errorMessage(error))
          }
        }
      } catch (error) {
        if (!disposed) {
          setLoadError(errorMessage(error))
          pollAgain = true
        }
      } finally {
        if (!disposed) {
          setLoading(false)
          if (pollAgain) {
            timer = window.setTimeout(() => void load(), 2_000)
          }
        }
      }
    }

    void load(true)
    return () => {
      disposed = true
      if (timer !== undefined) {
        window.clearTimeout(timer)
      }
    }
  }, [gateway, jobId, profile.kind])

  const seats = useMemo<NativeSeatMetrics[]>(
    () => job?.progress?.seats ?? [],
    [job?.progress?.seats]
  )
  const sweeps = job?.progress?.sweeps ?? strategy?.currentSweeps ?? "unknown"
  const deviationValues = seats.flatMap((seat) =>
    seat.deviationGain ? [Number(seat.deviationGain.ci95[1])] : []
  )
  const finiteDeviationValues = deviationValues.filter(Number.isFinite)
  const deviation = finiteDeviationValues.length
    ? Math.max(...finiteDeviationValues)
    : null
  const actionColors = useMemo(
    () => strategyActionColors(strategy?.actions ?? []),
    [strategy?.actions]
  )

  const runOperation = async (
    label: string,
    operation: () => Promise<void>
  ) => {
    setBusyAction(label)
    setActionError(null)
    setReceipt(null)
    try {
      await operation()
    } catch (error) {
      setActionError(errorMessage(error))
    } finally {
      setBusyAction(null)
    }
  }

  const exportArtifact = (artifact: ArtifactKind) =>
    runOperation(`export-${artifact}`, async () => {
      const next = await gateway.exportArtifact(jobId, artifact)
      if (next) {
        setReceipt(
          `${next.fileName} · ${formatBytes(next.byteLength)} · SHA-256 ${next.sha256.slice(0, 12)}…`
        )
      }
    })

  const saveEffectiveConfig = () =>
    runOperation("save-config", async () => {
      if (!result) {
        return
      }
      const next = await gateway.saveConfig(
        result.effectiveConfigToml,
        `${displayName || "solve"}-effective.toml`
      )
      if (next) {
        setReceipt(`${next.fileName} · ${formatBytes(next.byteLength)}`)
      }
    })

  const resume = (submission: ResumeSubmission) =>
    runOperation("resume", async () => {
      const resumed = await gateway.resumeJob(
        jobId,
        submission.name,
        submission.overrides
      )
      setResumeOpen(false)
      onJobResumed(resumed, submission.name)
    })

  if (!gateway.availability.available || profile.kind === "remote") {
    return (
      <div className="screen-stack">
        <div className="screen-heading">
          <div>
            <div className="eyebrow">RESULT · {jobId}</div>
            <h1>結果</h1>
          </div>
        </div>
        <Alert>
          <IconInfoCircle />
          <AlertTitle>
            {profile.kind === "remote"
              ? "Remote Resultsは未実装です"
              : "Browser preview"}
          </AlertTitle>
          <AlertDescription>
            {profile.kind === "remote"
              ? "RemoteはUIと契約のみで、成果物を取得しません。"
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
          <div className="flex flex-wrap items-center gap-2">
            {job ? (
              <Badge
                variant={statusBadgeVariant(job.state)}
                className={statusBadgeClass(job.state)}
              >
                {job.state}
              </Badge>
            ) : null}
            <span className="eyebrow">RUN #{jobId}</span>
          </div>
          <h1>{loading && !job ? "結果を読み込み中…" : displayName}</h1>
          <p>
            {job?.finishedAt
              ? new Date(job.finishedAt).toLocaleString("ja-JP")
              : "finished time unavailable"}{" "}
            · {profile.name}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={!job?.resumeAvailable || busyAction !== null}
            onClick={() => setResumeOpen(true)}
          >
            {busyAction === "resume" ? (
              <IconLoader2 className="animate-spin" />
            ) : (
              <IconPlayerPlay />
            )}
            checkpointから再開
          </Button>
          <Button
            size="sm"
            disabled={!job?.artifacts.solution.available || busyAction !== null}
            onClick={() => exportArtifact("solution")}
          >
            <IconDownload />
            solutionを保存
          </Button>
        </div>
      </div>

      {loadError ? (
        <Alert variant="destructive">
          <IconInfoCircle />
          <AlertTitle>jobの状態を取得できません</AlertTitle>
          <AlertDescription>{loadError}</AlertDescription>
        </Alert>
      ) : null}

      {job?.error ? (
        <Alert variant="destructive">
          <IconInfoCircle />
          <AlertTitle>Solveに失敗しました</AlertTitle>
          <AlertDescription>{job.error.message}</AlertDescription>
        </Alert>
      ) : null}

      {job && !terminalStates.has(job.state) ? (
        <Alert>
          <IconInfoCircle />
          <AlertTitle>Solveはまだ実行中です</AlertTitle>
          <AlertDescription>
            terminal状態になった後に結果artifactを読み込みます。
          </AlertDescription>
        </Alert>
      ) : null}

      {resultError ? (
        <Alert variant="destructive">
          <IconInfoCircle />
          <AlertTitle>結果artifactを読み込めません</AlertTitle>
          <AlertDescription>{resultError}</AlertDescription>
        </Alert>
      ) : null}

      {actionError ? (
        <Alert variant="destructive">
          <IconInfoCircle />
          <AlertTitle>操作を完了できませんでした</AlertTitle>
          <AlertDescription>{actionError}</AlertDescription>
        </Alert>
      ) : null}

      {receipt ? (
        <Alert>
          <IconCheck />
          <AlertTitle>保存しました</AlertTitle>
          <AlertDescription>{receipt}</AlertDescription>
        </Alert>
      ) : null}

      {loading ? (
        <Card>
          <CardContent className="flex min-h-52 items-center justify-center text-sm text-muted-foreground">
            <IconLoader2 className="mr-2 animate-spin" />
            resultとartifactを検証中…
          </CardContent>
        </Card>
      ) : (
        <>
          <div className="result-summary-grid">
            <Card>
              <CardContent className="metric-card">
                <span>Stop status</span>
                <strong>{result?.terminalStatus ?? job?.state ?? "—"}</strong>
                <small>{formatInteger(sweeps)} sweeps</small>
              </CardContent>
            </Card>
            <Card>
              <CardContent className="metric-card">
                <span>Deviation upper</span>
                <strong>
                  {deviation === null ? "—" : deviation.toFixed(4)}
                </strong>
                <small>
                  max seat 95% CI
                  {job?.progress
                    ? ` · target ${job.progress.stopTarget} ${job.progress.stopTargetUnit}`
                    : ""}
                </small>
              </CardContent>
            </Card>
            <Card>
              <CardContent className="metric-card">
                <span>Elapsed</span>
                <strong>
                  {formatElapsed(job?.progress?.elapsedSecs ?? null)}
                </strong>
                <small>cumulative solve elapsed</small>
              </CardContent>
            </Card>
            <Card>
              <CardContent className="metric-card">
                <span>Strategy coverage</span>
                <strong>
                  {strategy
                    ? `${(Number(strategy.coverage) * 100).toFixed(1)}%`
                    : "—"}
                </strong>
                <small>visited entries / domain entries</small>
              </CardContent>
            </Card>
          </div>

          <Alert>
            <IconInfoCircle />
            <AlertTitle>結果の保証境界</AlertTitle>
            <AlertDescription>
              {result?.guaranteeBoundary ??
                "Multiway profileはregret-minimized approximationです。"}
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
              <TabsTrigger value="artifacts">Artifacts</TabsTrigger>
            </TabsList>

            <TabsContent value="strategy">
              <Card>
                <CardHeader className="border-b">
                  <CardTitle className="flex items-center gap-2">
                    Linear average strategy
                    {strategy ? (
                      <Badge
                        variant={
                          strategy.status === "stale" ? "secondary" : "default"
                        }
                      >
                        {strategy.status}
                      </Badge>
                    ) : null}
                  </CardTitle>
                  <CardDescription>
                    {strategy
                      ? `Root strategy · ${strategy.node.nodeId} · actor S${strategy.node.actorSeat} · ${strategy.node.street} · revision ${strategy.revision}`
                      : (strategyMessage ?? "formal solutionはありません")}
                  </CardDescription>
                </CardHeader>
                {strategy ? (
                  <CardContent className="p-0">
                    <div className="strategy-workbench">
                      <div className="min-w-0 p-4">
                        <div className="mb-3 flex flex-wrap items-center gap-3">
                          {strategy.actions.map((action, index) => (
                            <span
                              className="flex items-center gap-1.5 text-xs"
                              key={action.id}
                            >
                              <i
                                className="size-2 rounded-full"
                                style={{
                                  backgroundColor: actionColors[index],
                                }}
                              />
                              {action.label}
                            </span>
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
                        />
                        <Separator />
                        <p className="text-xs text-muted-foreground">
                          as of {formatInteger(strategy.asOfSweeps)} sweeps ·{" "}
                          {strategy.generatedAt}
                        </p>
                      </aside>
                    </div>
                  </CardContent>
                ) : (
                  <CardContent className="flex min-h-48 items-center justify-center text-sm text-muted-foreground">
                    cancelled / resource-limit / failedにはformal
                    solutionがない場合があります。
                  </CardContent>
                )}
              </Card>
            </TabsContent>

            <TabsContent value="seats">
              <Card>
                <CardHeader className="border-b">
                  <CardTitle>Seat economics</CardTitle>
                  <CardDescription>
                    profile EVと95% CI。値がないseatを0として補完しません。
                  </CardDescription>
                </CardHeader>
                <CardContent className="p-0">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Seat</TableHead>
                        <TableHead className="text-right">
                          {result ? `EV（${result.units.utility}）` : "EV"}
                        </TableHead>
                        <TableHead className="text-right">95% CI</TableHead>
                        <TableHead className="text-right">
                          Positive regret
                        </TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {seats.map((seat) => (
                        <TableRow key={seat.seat}>
                          <TableCell className="font-medium">
                            Seat {seat.seat}
                          </TableCell>
                          <TableCell
                            className={cn(
                              "text-right font-mono",
                              seat.profileEv &&
                                (Number(seat.profileEv.mean) >= 0
                                  ? "text-emerald-700"
                                  : "text-red-600")
                            )}
                          >
                            {seat.profileEv
                              ? formatDecimal(seat.profileEv.mean)
                              : "unavailable"}
                          </TableCell>
                          <TableCell className="text-right font-mono">
                            {seat.profileEv
                              ? `[${formatDecimal(seat.profileEv.ci95[0])}, ${formatDecimal(seat.profileEv.ci95[1])}]`
                              : "—"}
                          </TableCell>
                          <TableCell className="text-right font-mono">
                            {seat.averagePositiveRegret !== null
                              ? formatScientific(seat.averagePositiveRegret)
                              : "—"}
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
                {seats.map((seat) => (
                  <Card key={seat.seat}>
                    <CardHeader className="border-b">
                      <CardTitle>Seat {seat.seat}</CardTitle>
                      <CardDescription>quality diagnostics</CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-3">
                      <p className="flex justify-between gap-3">
                        <span>Average positive regret</span>
                        <strong className="font-mono">
                          {seat.averagePositiveRegret !== null
                            ? formatScientific(seat.averagePositiveRegret, 4)
                            : "—"}
                        </strong>
                      </p>
                      <p className="flex justify-between gap-3">
                        <span>Strategy drift L1</span>
                        <strong className="font-mono">
                          {seat.strategyDriftL1 !== null
                            ? formatScientific(seat.strategyDriftL1, 4)
                            : "—"}
                        </strong>
                      </p>
                      <p className="flex justify-between gap-3">
                        <span>Deviation upper</span>
                        <strong className="font-mono">
                          {seat.deviationGain
                            ? formatDecimal(seat.deviationGain.ci95[1])
                            : "unavailable"}
                        </strong>
                      </p>
                    </CardContent>
                  </Card>
                ))}
              </div>
            </TabsContent>

            <TabsContent value="config">
              <Card>
                <CardHeader className="border-b">
                  <CardTitle>Effective config</CardTitle>
                  <CardDescription>
                    defaultsと導出値を展開したreparse可能なTOML
                  </CardDescription>
                  <CardAction>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={!result || busyAction !== null}
                      onClick={saveEffectiveConfig}
                    >
                      <IconDownload />
                      TOMLを保存
                    </Button>
                  </CardAction>
                </CardHeader>
                <CardContent>
                  <pre className="config-preview">
                    {result?.effectiveConfigToml ?? "unavailable"}
                  </pre>
                </CardContent>
              </Card>
            </TabsContent>

            <TabsContent value="artifacts">
              <Card>
                <CardHeader className="border-b">
                  <CardTitle>Generated files</CardTitle>
                  <CardDescription>
                    native save dialogで保存先を選択します。
                  </CardDescription>
                </CardHeader>
                <CardContent className="grid gap-3 md:grid-cols-2">
                  {job
                    ? (Object.keys(artifactLabels) as ArtifactKind[]).map(
                        (artifact) => {
                          const descriptor = job.artifacts[artifact]
                          return (
                            <div
                              className="flex items-center justify-between gap-3 rounded-lg border p-3"
                              key={artifact}
                            >
                              <div className="min-w-0">
                                <p className="font-medium">
                                  {artifactLabels[artifact]}
                                </p>
                                <p className="text-xs text-muted-foreground">
                                  {descriptor.available
                                    ? `${formatBytes(descriptor.byteLength)}${descriptor.sha256 ? ` · SHA-256 ${descriptor.sha256.slice(0, 12)}…` : ""}`
                                    : "not available"}
                                </p>
                              </div>
                              <Button
                                variant="outline"
                                size="sm"
                                disabled={
                                  !descriptor.available || busyAction !== null
                                }
                                onClick={() => exportArtifact(artifact)}
                              >
                                {busyAction === `export-${artifact}` ? (
                                  <IconLoader2 className="animate-spin" />
                                ) : (
                                  <IconDownload />
                                )}
                                保存
                              </Button>
                            </div>
                          )
                        }
                      )
                    : null}
                </CardContent>
              </Card>
            </TabsContent>
          </Tabs>

          <div className="flex items-center gap-2 rounded-lg border bg-muted/30 p-3">
            <span className="rounded-md border bg-background p-2">
              <IconChartBar className="size-4" />
            </span>
            <div>
              <p className="text-xs font-medium">Artifact integrity</p>
              <p className="text-xs text-muted-foreground">
                file formatとfingerprintはRust readerが検証してから表示します。
              </p>
            </div>
          </div>
        </>
      )}
      {resumeOpen && job ? (
        <ResumeDialog
          key={`${job.id}-${job.progress?.sweeps ?? "0"}`}
          sourceLabel={`job ${job.id}`}
          completedSweeps={
            job.progress?.sweeps ?? strategy?.currentSweeps ?? "0"
          }
          suggestedName={`Resumed ${displayName}`}
          busy={busyAction === "resume"}
          onCancel={() => setResumeOpen(false)}
          onConfirm={resume}
        />
      ) : null}
    </div>
  )
}
