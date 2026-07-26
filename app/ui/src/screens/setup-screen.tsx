import { useMemo, useRef, useState } from "react"
import {
  IconArrowRight,
  IconBolt,
  IconCheck,
  IconChevronRight,
  IconCpu,
  IconDownload,
  IconFileDescription,
  IconFolderOpen,
  IconInfoCircle,
  IconLoader2,
  IconStack2,
} from "@tabler/icons-react"

import { BettingTreeEditor } from "@/components/betting-tree-editor"
import { EconomicsEditor } from "@/components/economics-editor"
import { ResumeDialog, type ResumeSubmission } from "@/components/resume-dialog"
import { SetupPresetPicker } from "@/components/setup-preset-picker"
import { TableRangeEditor } from "@/components/table-range-editor"
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
import { Progress } from "@/components/ui/progress"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Textarea } from "@/components/ui/textarea"
import type {
  ConnectionProfileViewModel,
  ValidationResult,
} from "@/lib/solve-contract"
import type {
  CheckpointSource,
  DesktopSolveGateway,
  NativeJobSnapshot,
} from "@/lib/native-gateway"
import { errorMessage } from "@/lib/native-gateway"
import { formatBytes, formatInteger } from "@/lib/format"
import {
  applyTreePreset,
  createDefaultFormDraft,
  createRecommendedSetupPreset,
  DraftTokenError,
  renderFormDraftToml,
  splitNumericTokenList,
  type FormSolveDraft,
  type RecommendedSetupPresetId,
  type TreePresetId,
} from "@/lib/setup-config"

type SetupScreenProps = {
  profile: ConnectionProfileViewModel
  gateway: DesktopSolveGateway
  onJobStarted: (job: NativeJobSnapshot, name: string) => void
  onCheckpointResumed: (job: NativeJobSnapshot, name: string) => void
  onResultOpened: (job: NativeJobSnapshot, name: string) => void
  onOpenConnections: () => void
}

type ConfigSource =
  | { kind: "form" }
  | {
      kind: "toml"
      sourceId: string
      fileName: string
      configToml: string
    }

function memoryPercent(estimate: string, budget: string) {
  try {
    const estimateBytes = BigInt(estimate)
    const budgetBytes = BigInt(budget)
    if (budgetBytes <= 0n) {
      return null
    }
    const perMille = (estimateBytes * 1_000n) / budgetBytes
    return Number(perMille > 1_000n ? 1_000n : perMille) / 10
  } catch {
    return null
  }
}

export function SetupScreen({
  profile,
  gateway,
  onJobStarted,
  onCheckpointResumed,
  onResultOpened,
  onOpenConnections,
}: SetupScreenProps) {
  const [draft, setDraft] = useState<FormSolveDraft>(createDefaultFormDraft)
  const [source, setSource] = useState<ConfigSource>({ kind: "form" })
  const [setupPreset, setSetupPreset] = useState<
    RecommendedSetupPresetId | "custom"
  >("custom")
  const [treePreset, setTreePreset] = useState<TreePresetId | "custom">(
    "compact-checkdown"
  )
  const [editorTab, setEditorTab] = useState("table")
  const [validation, setValidation] = useState<ValidationResult | null>(null)
  const [busyAction, setBusyAction] = useState<string | null>(null)
  const [operationError, setOperationError] = useState<string | null>(null)
  const [checkpointSource, setCheckpointSource] =
    useState<CheckpointSource | null>(null)
  const draftRevision = useRef(0)
  const commandBarRef = useRef<HTMLDivElement>(null)
  const isRemote = profile.kind === "remote"
  const nativeReady = gateway.availability.available
  const configLocked = busyAction === "validate" || busyAction === "start"
  const treePreflight = validation?.preflight?.tree
  const memoryPreflight = validation?.preflight?.memory
  const economicsPreflight = validation?.preflight?.economics
  const icmFieldPlayers =
    draft.seatCount +
    splitNumericTokenList(draft.economics.tournamentIcm.outsideFieldBb).length
  const solverStatePercent =
    memoryPreflight?.solverStateBytes && memoryPreflight.budgetBytes
      ? memoryPercent(
          memoryPreflight.solverStateBytes,
          memoryPreflight.budgetBytes
        )
      : null

  const generatedToml = useMemo(() => {
    if (source.kind === "toml") {
      return { value: source.configToml, error: null }
    }
    try {
      return { value: renderFormDraftToml(draft), error: null }
    } catch (error) {
      return {
        value: null,
        error:
          error instanceof DraftTokenError
            ? `${error.path}: ${error.message}`
            : errorMessage(error),
      }
    }
  }, [draft, source])
  const validationError = validation?.errors[0]
  const commandError =
    operationError ??
    generatedToml.error ??
    (validationError
      ? `${validationError.path}: ${validationError.message}`
      : null)

  const invalidate = () => {
    draftRevision.current += 1
    setValidation(null)
    setOperationError(null)
  }

  const updateDraft = (next: FormSolveDraft) => {
    setDraft(next)
    setSetupPreset("custom")
    invalidate()
  }

  const handleSetupPreset = (presetId: RecommendedSetupPresetId) => {
    const next = createRecommendedSetupPreset(presetId)
    setSource({ kind: "form" })
    setDraft(next)
    setSetupPreset(presetId)
    setTreePreset(
      presetId === "cash-6max-100bb-k256"
        ? "cash-canonical"
        : "tournament-canonical"
    )
    setEditorTab("table")
    invalidate()
  }

  const handleTreePreset = (presetId: TreePresetId) => {
    setSource({ kind: "form" })
    setDraft((current) => applyTreePreset(current, presetId))
    setSetupPreset("custom")
    setTreePreset(presetId)
    setEditorTab("tree")
    invalidate()
  }

  const updateTreeDraft = (next: FormSolveDraft) => {
    setTreePreset("custom")
    updateDraft(next)
  }

  const currentToml = () => {
    if (!generatedToml.value) {
      throw new Error(generatedToml.error ?? "設定をTOMLへ変換できません。")
    }
    return generatedToml.value
  }

  const currentSourceId = () =>
    source.kind === "toml"
      ? source.sourceId
      : draft.treeKind === "script"
        ? (draft.treeScriptSourceId ?? undefined)
        : undefined

  const runOperation = async (
    label: string,
    operation: () => Promise<void>
  ) => {
    setBusyAction(label)
    setOperationError(null)
    try {
      await operation()
    } catch (error) {
      setOperationError(errorMessage(error))
    } finally {
      setBusyAction(null)
    }
  }

  const handleLoadConfig = () =>
    runOperation("load", async () => {
      const loaded = await gateway.loadConfig()
      if (!loaded) {
        return
      }
      setSource({
        kind: "toml",
        sourceId: loaded.sourceId,
        fileName: loaded.fileName,
        configToml: loaded.configToml,
      })
      setSetupPreset("custom")
      setTreePreset("custom")
      setDraft((current) => ({
        ...current,
        name: loaded.fileName.replace(/\.toml$/i, ""),
      }))
      invalidate()
    })

  const handleSaveConfig = () =>
    runOperation("save", async () => {
      await gateway.saveConfig(currentToml(), `${draft.name || "solve"}.toml`)
    })

  const handlePickTreeScript = () =>
    runOperation("tree-script", async () => {
      const selected = await gateway.pickTreeScript()
      if (!selected) {
        return
      }
      setDraft((current) => ({
        ...current,
        treeKind: "script",
        treeScriptSource: selected.fileName,
        treeScriptSourceId: selected.sourceId,
      }))
      setSetupPreset("custom")
      setTreePreset("custom")
      setEditorTab("tree")
      invalidate()
    })

  const handleValidate = () =>
    runOperation("validate", async () => {
      const configToml = currentToml()
      const sourceId = currentSourceId()
      const revision = draftRevision.current
      const result = await gateway.validateConfig(configToml, sourceId)
      if (revision === draftRevision.current) {
        setValidation(result)
        if (!result.valid) {
          requestAnimationFrame(() => commandBarRef.current?.focus())
        }
      }
    })

  const handleStart = () =>
    runOperation("start", async () => {
      const name = draft.name.trim() || "Untitled solve"
      const revision = draftRevision.current
      let current = validation
      if (
        !current?.valid ||
        !current.effectiveConfigToml ||
        !current.configFingerprint
      ) {
        const result = await gateway.validateConfig(
          currentToml(),
          currentSourceId()
        )
        if (revision !== draftRevision.current) {
          return
        }
        setValidation(result)
        current = result
        if (!result.valid) {
          requestAnimationFrame(() => commandBarRef.current?.focus())
          return
        }
      }
      if (
        !current.valid ||
        !current.effectiveConfigToml ||
        !current.configFingerprint
      ) {
        return
      }
      const job = await gateway.startJob(
        name,
        current.effectiveConfigToml,
        current.configFingerprint
      )
      if (revision === draftRevision.current) {
        onJobStarted(job, name || job.id)
      }
    })

  const handleOpenResult = (kind: "run" | "solution") =>
    runOperation(kind, async () => {
      const job =
        kind === "run" ? await gateway.openRun() : await gateway.openSolution()
      if (job) {
        onResultOpened(job, job.id)
      }
    })

  const handlePickCheckpoint = () =>
    runOperation("checkpoint-pick", async () => {
      const selected = await gateway.pickCheckpoint()
      if (selected) {
        setCheckpointSource(selected)
      }
    })

  const handleResumeCheckpoint = (submission: ResumeSubmission) =>
    runOperation("checkpoint-resume", async () => {
      if (!checkpointSource) {
        return
      }
      const job = await gateway.resumeCheckpoint(
        checkpointSource.sourceId,
        submission.name,
        submission.overrides
      )
      setCheckpointSource(null)
      onCheckpointResumed(job, submission.name)
    })

  return (
    <div className="screen-stack">
      <div className="screen-heading">
        <div>
          <div className="eyebrow">NEW SOLVE</div>
          <h1>Solve設定を作成</h1>
          <p>
            v1設定をRust
            normalizerで検証し、同じ実効TOMLからローカルSolveを開始します。
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={!nativeReady || busyAction !== null}
            onClick={handleLoadConfig}
          >
            <IconFileDescription />
            TOMLを読み込む
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={
              !nativeReady || busyAction !== null || !generatedToml.value
            }
            onClick={handleSaveConfig}
          >
            <IconDownload />
            下書きを保存
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={!nativeReady || busyAction !== null}
            onClick={() => handleOpenResult("solution")}
          >
            <IconFolderOpen />
            .mwsolを開く
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={!nativeReady || busyAction !== null}
            onClick={handlePickCheckpoint}
          >
            <IconStack2 />
            .mwckptから再開
          </Button>
        </div>
      </div>

      {!nativeReady ? (
        <Alert>
          <IconInfoCircle />
          <AlertTitle>Browser preview</AlertTitle>
          <AlertDescription>
            {gateway.availability.reason}
            設定内容は確認できますが、検証・Solve・ファイル操作は無効です。
          </AlertDescription>
        </Alert>
      ) : null}

      {isRemote ? (
        <Alert className="border-amber-300 bg-amber-50 text-amber-950">
          <IconInfoCircle />
          <AlertTitle>Remoteは仕様・UIのみです</AlertTitle>
          <AlertDescription className="text-amber-800">
            接続先プロファイルは確認できますが、認証・送信・remote
            job作成はこの実装には含まれません。
          </AlertDescription>
        </Alert>
      ) : null}

      {operationError || generatedToml.error ? (
        <Alert variant="destructive">
          <IconInfoCircle />
          <AlertTitle>操作を完了できませんでした</AlertTitle>
          <AlertDescription>
            {operationError ?? generatedToml.error}
          </AlertDescription>
        </Alert>
      ) : null}

      <div
        ref={commandBarRef}
        className="setup-command-bar"
        role="region"
        aria-label="Solve設定の検証と開始"
        aria-live="polite"
        aria-atomic="true"
        aria-busy={busyAction === "validate" || busyAction === "start"}
        tabIndex={-1}
      >
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Badge
              variant={
                generatedToml.error
                  ? "destructive"
                  : validation?.valid
                    ? "default"
                    : validation
                      ? "destructive"
                      : "outline"
              }
            >
              {generatedToml.error
                ? "入力エラー"
                : validation?.valid
                  ? "BUILD OK"
                  : validation
                    ? "要修正"
                    : "未検証"}
            </Badge>
            <strong className="truncate">
              {commandError
                ? "設定を修正してください"
                : validation?.valid
                  ? "ツリーとresourceを確認済み"
                  : "Solve開始で構築・検証から実行します"}
            </strong>
          </div>
          <p className={commandError ? "command-error" : undefined}>
            {commandError ??
              "Solve開始は未検証の設定を自動で構築・検証してから実行します。"}
          </p>
        </div>
        <Button
          variant="outline"
          disabled={
            !nativeReady ||
            isRemote ||
            busyAction !== null ||
            generatedToml.error !== null
          }
          onClick={handleValidate}
        >
          {busyAction === "validate" ? (
            <IconLoader2 className="animate-spin" />
          ) : (
            <IconCheck />
          )}
          ツリー構築・検証
        </Button>
        <Button
          disabled={
            !nativeReady ||
            isRemote ||
            busyAction !== null ||
            generatedToml.error !== null
          }
          onClick={handleStart}
        >
          {busyAction === "start" ? (
            <IconLoader2 className="animate-spin" />
          ) : (
            <IconArrowRight />
          )}
          {validation?.valid ? "Solve開始" : "検証してSolve開始"}
        </Button>
      </div>

      <div className="setup-layout">
        <fieldset
          className="min-w-0 space-y-4 border-0 p-0 disabled:opacity-75"
          disabled={configLocked}
        >
          <Card>
            <CardHeader className="border-b">
              <h2 className="font-heading text-sm font-medium">開始点</h2>
              <CardDescription>
                フォームまたは読み込んだTOMLのどちらか一方を編集します。
              </CardDescription>
              <CardAction>
                <Badge variant="secondary">
                  {source.kind === "form" ? "Multiway v1" : source.fileName}
                </Badge>
              </CardAction>
            </CardHeader>
            <CardContent className="grid gap-4 md:grid-cols-3">
              <div className="field-stack md:col-span-2">
                <Label htmlFor="solve-name">Solve名</Label>
                <Input
                  id="solve-name"
                  value={draft.name}
                  onChange={(event) =>
                    setDraft({ ...draft, name: event.target.value })
                  }
                />
              </div>
              <div className="field-stack">
                <Label>設定ソース</Label>
                <Select
                  value={source.kind}
                  onValueChange={(value) => {
                    if (value === "form") {
                      setSource({ kind: "form" })
                      invalidate()
                    }
                  }}
                >
                  <SelectTrigger className="w-full" aria-label="設定ソース">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="form">フォームエディター</SelectItem>
                    {source.kind === "toml" ? (
                      <SelectItem value="toml">Imported TOML</SelectItem>
                    ) : null}
                  </SelectContent>
                </Select>
              </div>
            </CardContent>
          </Card>

          <SetupPresetPicker
            setupPreset={setupPreset}
            treePreset={treePreset}
            onSetupPresetChange={handleSetupPreset}
            onTreePresetChange={handleTreePreset}
          />

          {source.kind === "toml" ? (
            <Card>
              <CardHeader className="border-b">
                <h2 className="font-heading text-sm font-medium">
                  Imported TOML
                </h2>
                <CardDescription>
                  相対.mwtreeは検証時にopaque
                  sourceIdを使ってmaterializeします。client
                  pathはReactへ渡されません。
                </CardDescription>
              </CardHeader>
              <CardContent>
                <Textarea
                  className="min-h-96 font-mono text-xs"
                  value={source.configToml}
                  onChange={(event) => {
                    setSource({ ...source, configToml: event.target.value })
                    invalidate()
                  }}
                  spellCheck={false}
                  aria-label="Multiway v1 TOML"
                />
              </CardContent>
            </Card>
          ) : (
            <Tabs
              value={editorTab}
              onValueChange={setEditorTab}
              className="setup-editor-workbench"
            >
              <TabsList className="setup-editor-tabs">
                <TabsTrigger value="table">1 · テーブル</TabsTrigger>
                <TabsTrigger value="economics">2 · ICM / Rake</TabsTrigger>
                <TabsTrigger value="tree">3 · Tree</TabsTrigger>
                <TabsTrigger value="solver">4 · Solver / 実行</TabsTrigger>
              </TabsList>

              <TabsContent value="table" className="mt-0">
                <TableRangeEditor draft={draft} onChange={updateDraft} />
              </TabsContent>

              <TabsContent value="economics" className="mt-0">
                <EconomicsEditor draft={draft} onChange={updateDraft} />
              </TabsContent>

              <TabsContent value="tree" className="mt-0">
                <BettingTreeEditor
                  treeKind={draft.treeKind}
                  scriptSource={draft.treeScriptSource}
                  scriptParams={draft.treeScriptParams}
                  allowLimp={draft.treeAllowLimp}
                  aggressionCapsEnabled={draft.treeAggressionCapsEnabled}
                  aggressionCaps={{
                    preflop: draft.treeAggressionCapPreflop,
                    flop: draft.treeAggressionCapFlop,
                    turn: draft.treeAggressionCapTurn,
                    river: draft.treeAggressionCapRiver,
                  }}
                  reraiseJamEnabled={draft.treeReraiseJamEnabled}
                  reraiseJamNumerator={draft.treeReraiseJamNumerator}
                  reraiseJamDenominator={draft.treeReraiseJamDenominator}
                  rules={draft.treeRules}
                  onChange={(treeRules) =>
                    updateTreeDraft({ ...draft, treeRules })
                  }
                  onTreeConfigChange={(update) =>
                    updateTreeDraft({ ...draft, ...update })
                  }
                  onPickScript={() => void handlePickTreeScript()}
                  disabled={configLocked}
                />
              </TabsContent>

              <TabsContent value="solver" className="mt-0">
                <Card size="sm">
                  <CardHeader className="border-b">
                    <h2 className="font-heading text-sm font-medium">
                      Solver・実行設定
                    </h2>
                    <CardDescription>
                      精度・abstractionと、実行上限・resourceを分けて設定します。
                    </CardDescription>
                  </CardHeader>
                  <CardContent>
                    <Tabs defaultValue="solver">
                      <TabsList
                        variant="line"
                        className="w-full max-w-full justify-start overflow-x-auto"
                      >
                        <TabsTrigger value="solver">Solver・精度</TabsTrigger>
                        <TabsTrigger value="runtime">
                          実行・リソース
                        </TabsTrigger>
                      </TabsList>

                      <TabsContent value="solver" className="space-y-4 pt-3">
                        <div className="grid gap-3 rounded-lg border p-3 md:grid-cols-3">
                          <div className="rounded-md border bg-muted/25 p-3 md:col-span-3">
                            <div className="flex flex-wrap items-center justify-between gap-2">
                              <div>
                                <Label>Production abstraction</Label>
                                <p className="field-help">
                                  EHS² percentile・current-street
                                  recallでsweep前に全policy arenaを確保します。
                                </p>
                              </div>
                              <div className="flex gap-2">
                                <Badge variant="secondary">EHS²</Badge>
                                <Badge variant="outline">Current street</Badge>
                              </div>
                            </div>
                          </div>
                          {(["flop", "turn", "river"] as const).map(
                            (street) => {
                              const field = `${street}Buckets` as const
                              return (
                                <div className="field-stack" key={street}>
                                  <Label htmlFor={`${street}-buckets`}>
                                    {street[0].toUpperCase() + street.slice(1)}{" "}
                                    buckets
                                  </Label>
                                  <Input
                                    id={`${street}-buckets`}
                                    inputMode="numeric"
                                    value={draft[field]}
                                    onChange={(event) =>
                                      updateDraft({
                                        ...draft,
                                        [field]: event.target.value,
                                      })
                                    }
                                  />
                                </div>
                              )
                            }
                          )}
                        </div>

                        <div className="grid gap-3 rounded-lg border p-3 md:grid-cols-4">
                          <div className="field-stack">
                            <Label>Solver kind</Label>
                            <Select
                              value={draft.solverKind}
                              onValueChange={(solverKind) =>
                                updateDraft({
                                  ...draft,
                                  solverKind:
                                    solverKind as FormSolveDraft["solverKind"],
                                  pruningKind:
                                    solverKind === "single-hand"
                                      ? "none"
                                      : draft.pruningKind,
                                })
                              }
                            >
                              <SelectTrigger className="w-full">
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                <SelectItem value="range-vector">
                                  Range vector
                                </SelectItem>
                                <SelectItem value="single-hand">
                                  Single hand
                                </SelectItem>
                              </SelectContent>
                            </Select>
                          </div>
                          <div className="field-stack">
                            <Label htmlFor="solver-seed">Solver seed</Label>
                            <Input
                              id="solver-seed"
                              inputMode="numeric"
                              value={draft.solverSeed}
                              onChange={(event) =>
                                updateDraft({
                                  ...draft,
                                  solverSeed: event.target.value,
                                })
                              }
                            />
                          </div>
                          <div className="field-stack">
                            <Label htmlFor="opponent-exploration">
                              Opponent exploration
                            </Label>
                            <Input
                              id="opponent-exploration"
                              inputMode="decimal"
                              value={draft.opponentExploration}
                              onChange={(event) =>
                                updateDraft({
                                  ...draft,
                                  opponentExploration: event.target.value,
                                })
                              }
                            />
                          </div>
                          <div className="field-stack">
                            <Label htmlFor="batch-sweeps">Batch sweeps</Label>
                            <Input
                              id="batch-sweeps"
                              inputMode="numeric"
                              value={draft.batchSweeps}
                              onChange={(event) =>
                                updateDraft({
                                  ...draft,
                                  batchSweeps: event.target.value,
                                })
                              }
                            />
                          </div>
                          <div className="field-stack">
                            <Label>Discount</Label>
                            <Select
                              value={draft.discountKind}
                              onValueChange={(discountKind) =>
                                updateDraft({
                                  ...draft,
                                  discountKind:
                                    discountKind as FormSolveDraft["discountKind"],
                                })
                              }
                            >
                              <SelectTrigger className="w-full">
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                <SelectItem value="periodic">
                                  Periodic
                                </SelectItem>
                                <SelectItem value="none">None</SelectItem>
                              </SelectContent>
                            </Select>
                          </div>
                          {draft.discountKind === "periodic" ? (
                            <>
                              <div className="field-stack">
                                <Label htmlFor="discount-every">
                                  Discount every
                                </Label>
                                <Input
                                  id="discount-every"
                                  inputMode="numeric"
                                  value={draft.discountEverySweeps}
                                  onChange={(event) =>
                                    updateDraft({
                                      ...draft,
                                      discountEverySweeps: event.target.value,
                                    })
                                  }
                                />
                              </div>
                              <div className="field-stack">
                                <Label htmlFor="discount-until">
                                  Discount until
                                </Label>
                                <Input
                                  id="discount-until"
                                  inputMode="numeric"
                                  value={draft.discountUntilSweeps}
                                  onChange={(event) =>
                                    updateDraft({
                                      ...draft,
                                      discountUntilSweeps: event.target.value,
                                    })
                                  }
                                />
                              </div>
                            </>
                          ) : null}
                          <div className="field-stack">
                            <Label>Pruning</Label>
                            <Select
                              value={draft.pruningKind}
                              disabled={draft.solverKind === "single-hand"}
                              onValueChange={(pruningKind) =>
                                updateDraft({
                                  ...draft,
                                  pruningKind:
                                    pruningKind as FormSolveDraft["pruningKind"],
                                })
                              }
                            >
                              <SelectTrigger className="w-full">
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                <SelectItem value="regret-based">
                                  Regret based
                                </SelectItem>
                                <SelectItem value="none">None</SelectItem>
                              </SelectContent>
                            </Select>
                          </div>
                        </div>
                      </TabsContent>

                      <TabsContent
                        value="runtime"
                        className="grid gap-4 pt-3 md:grid-cols-3"
                      >
                        <div className="field-stack">
                          <Label htmlFor="sweeps">Max sweeps</Label>
                          <Input
                            id="sweeps"
                            inputMode="numeric"
                            value={draft.maxSweeps}
                            onChange={(event) =>
                              updateDraft({
                                ...draft,
                                maxSweeps: event.target.value,
                              })
                            }
                          />
                        </div>
                        <div className="field-stack">
                          <Label htmlFor="max-time">Max time（optional）</Label>
                          <Input
                            id="max-time"
                            value={draft.maxTime}
                            placeholder="12h"
                            onChange={(event) =>
                              updateDraft({
                                ...draft,
                                maxTime: event.target.value,
                              })
                            }
                          />
                        </div>
                        <div className="field-stack">
                          <Label htmlFor="target">Stop target</Label>
                          <Input
                            id="target"
                            value={draft.stopTarget}
                            onChange={(event) =>
                              updateDraft({
                                ...draft,
                                stopTarget: event.target.value,
                              })
                            }
                          />
                          <p className="field-help">
                            {draft.economics.kind === "tournament-icm"
                              ? '"default" = total prize poolの0.0001。明示値はprize pool比率。'
                              : '"default" = 0.05 BB / hand。明示値はBB / hand。'}
                          </p>
                        </div>
                        <div className="field-stack">
                          <Label htmlFor="checkpoint">
                            Checkpoint interval
                          </Label>
                          <Input
                            id="checkpoint"
                            value={draft.checkpointInterval}
                            onChange={(event) =>
                              updateDraft({
                                ...draft,
                                checkpointInterval: event.target.value,
                              })
                            }
                          />
                        </div>
                        <div className="field-stack">
                          <Label htmlFor="threads">Threads</Label>
                          <Input
                            id="threads"
                            value={draft.threads}
                            onChange={(event) =>
                              updateDraft({
                                ...draft,
                                threads: event.target.value,
                              })
                            }
                          />
                        </div>
                        <div className="field-stack">
                          <Label htmlFor="memory">Memory</Label>
                          <Input
                            id="memory"
                            value={draft.memory}
                            onChange={(event) =>
                              updateDraft({
                                ...draft,
                                memory: event.target.value,
                              })
                            }
                          />
                          <p className="field-help">
                            auto = 6 GiB。productionでは明示値も6 GiB以下。
                          </p>
                        </div>
                        <div className="field-stack">
                          <Label htmlFor="stop-check-every">
                            Evaluate every sweeps
                          </Label>
                          <Input
                            id="stop-check-every"
                            inputMode="numeric"
                            value={draft.stopCheckEverySweeps}
                            onChange={(event) =>
                              updateDraft({
                                ...draft,
                                stopCheckEverySweeps: event.target.value,
                              })
                            }
                          />
                        </div>
                        <div className="field-stack">
                          <Label htmlFor="stop-confirmations">
                            Confirmations
                          </Label>
                          <Input
                            id="stop-confirmations"
                            inputMode="numeric"
                            value={draft.stopConfirmations}
                            onChange={(event) =>
                              updateDraft({
                                ...draft,
                                stopConfirmations: event.target.value,
                              })
                            }
                          />
                        </div>
                        <div className="field-stack">
                          <Label htmlFor="evaluation-samples">
                            Evaluation samples
                          </Label>
                          <Input
                            id="evaluation-samples"
                            inputMode="numeric"
                            value={draft.evaluationSamples}
                            onChange={(event) =>
                              updateDraft({
                                ...draft,
                                evaluationSamples: event.target.value,
                              })
                            }
                          />
                        </div>
                        <div className="field-stack">
                          <Label htmlFor="deviator-traversals">
                            Deviator traversals
                          </Label>
                          <Input
                            id="deviator-traversals"
                            inputMode="numeric"
                            value={draft.deviatorTraversals}
                            onChange={(event) =>
                              updateDraft({
                                ...draft,
                                deviatorTraversals: event.target.value,
                              })
                            }
                          />
                        </div>
                        <div className="field-stack">
                          <Label>Probability encoding</Label>
                          <Select
                            value={draft.probabilityEncoding}
                            onValueChange={(probabilityEncoding) =>
                              updateDraft({
                                ...draft,
                                probabilityEncoding:
                                  probabilityEncoding as FormSolveDraft["probabilityEncoding"],
                              })
                            }
                          >
                            <SelectTrigger className="w-full">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="u16">
                                U16 production
                              </SelectItem>
                              <SelectItem value="f32">F32 research</SelectItem>
                            </SelectContent>
                          </Select>
                        </div>
                      </TabsContent>
                    </Tabs>
                  </CardContent>
                </Card>
              </TabsContent>
            </Tabs>
          )}
        </fieldset>

        <aside className="setup-summary">
          <Card className="sticky-card">
            <CardHeader className="border-b">
              <h2 className="font-heading text-sm font-medium">実行サマリー</h2>
              <CardDescription>
                {validation
                  ? validation.valid
                    ? "Rust validation済み"
                    : "設定を修正してください"
                  : "検証すると実効設定とresource設定を表示します"}
              </CardDescription>
              <CardAction>
                <Badge
                  variant={
                    validation?.valid
                      ? "default"
                      : validation
                        ? "destructive"
                        : "outline"
                  }
                >
                  {validation?.valid
                    ? "VALID"
                    : validation
                      ? "INVALID"
                      : "未検証"}
                </Badge>
              </CardAction>
            </CardHeader>
            <CardContent className="space-y-4">
              <button
                type="button"
                className="machine-summary"
                disabled={busyAction !== null}
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
                  <em>{isRemote ? "Remote · 未実装" : "Local · in-process"}</em>
                </span>
                <IconChevronRight className="size-4 text-muted-foreground" />
              </button>

              <div className="summary-metrics">
                <div>
                  <span>Economics</span>
                  <strong>
                    {economicsPreflight
                      ? economicsPreflight.kind === "cash"
                        ? "ChipEV"
                        : `ICM · ${economicsPreflight.fieldPlayers}p`
                      : source.kind === "toml"
                        ? "未検証"
                        : draft.economics.kind === "cash"
                          ? "ChipEV"
                          : `ICM · ${icmFieldPlayers}p`}
                  </strong>
                </div>
                <div>
                  <span>Threads</span>
                  <strong>
                    {validation?.preflight?.threads
                      ? source.kind === "form" && draft.threads === "auto"
                        ? `auto → ${validation.preflight.threads}`
                        : validation.preflight.threads
                      : "—"}
                  </strong>
                </div>
                <div>
                  <span>Recall</span>
                  <strong>{treePreflight?.recallMode ?? "—"}</strong>
                </div>
                <div>
                  <span>Decision nodes</span>
                  <strong>
                    {treePreflight
                      ? formatInteger(treePreflight.decisionNodes)
                      : "—"}
                  </strong>
                </div>
                <div>
                  <span>Solver state</span>
                  <strong>
                    {memoryPreflight?.solverStateBytes
                      ? formatBytes(memoryPreflight.solverStateBytes)
                      : memoryPreflight?.estimateKind === "prefix-lower-bound"
                        ? "prefix下限"
                        : "—"}
                  </strong>
                </div>
                <div>
                  <span>メモリ上限</span>
                  <strong>
                    {memoryPreflight
                      ? formatBytes(memoryPreflight.budgetBytes)
                      : "—"}
                  </strong>
                </div>
                <div>
                  <span>Abstraction</span>
                  <strong>{validation?.preflight?.abstraction ?? "—"}</strong>
                </div>
                <div>
                  <span>Fingerprint</span>
                  <strong className="truncate">
                    {validation?.configFingerprint?.slice(0, 12) ?? "—"}
                  </strong>
                </div>
              </div>

              {treePreflight && memoryPreflight ? (
                <>
                  <div className="space-y-3 rounded-lg border p-3">
                    <div className="flex items-center justify-between gap-2">
                      <div>
                        <p className="text-sm font-medium">
                          ベッティングツリー検証済み
                        </p>
                        <p className="text-xs text-muted-foreground">
                          実Solverと同じ公開action treeを全探索
                        </p>
                      </div>
                      <Badge variant="secondary">
                        {formatInteger(treePreflight.decisionNodes)} nodes
                      </Badge>
                    </div>
                    <div className="grid grid-cols-2 gap-2 text-xs">
                      <div>
                        <span className="text-muted-foreground">
                          Terminal edges
                        </span>
                        <strong className="block font-mono">
                          {treePreflight.terminalEdges
                            ? formatInteger(treePreflight.terminalEdges)
                            : "prefix検証中断"}
                        </strong>
                      </div>
                      <div>
                        <span className="text-muted-foreground">
                          Policy columns
                        </span>
                        <strong className="block font-mono">
                          {treePreflight.policyColumns
                            ? formatInteger(treePreflight.policyColumns)
                            : "prefix検証中断"}
                        </strong>
                      </div>
                      <div className="col-span-2">
                        <span className="text-muted-foreground">
                          Policy slots
                        </span>
                        <strong className="block font-mono">
                          {treePreflight.policySlots
                            ? formatInteger(treePreflight.policySlots)
                            : "prefix検証中断"}
                        </strong>
                      </div>
                    </div>
                  </div>

                  <div className="space-y-3 rounded-lg border p-3">
                    <div className="flex items-center justify-between gap-2">
                      <div>
                        <p className="text-sm font-medium">
                          Solver stateメモリ
                        </p>
                        <p className="text-xs text-muted-foreground">
                          {memoryPreflight.budgetMode === "auto"
                            ? "auto · production固定6 GiB"
                            : "明示指定した上限"}
                        </p>
                      </div>
                      <Badge
                        variant={
                          memoryPreflight.fitsBudget === false
                            ? "destructive"
                            : memoryPreflight.fitsBudget === true
                              ? "default"
                              : "secondary"
                        }
                      >
                        {memoryPreflight.fitsBudget === false
                          ? "上限超過"
                          : memoryPreflight.fitsBudget === true
                            ? "範囲内"
                            : "動的"}
                      </Badge>
                    </div>

                    {memoryPreflight.solverStateBytes ? (
                      <>
                        <div className="flex items-baseline justify-between gap-2 text-xs">
                          <strong className="font-mono">
                            {formatBytes(memoryPreflight.solverStateBytes)}
                          </strong>
                          <span className="text-muted-foreground">
                            / {formatBytes(memoryPreflight.budgetBytes)}
                          </span>
                        </div>
                        {solverStatePercent !== null ? (
                          <Progress
                            value={solverStatePercent}
                            aria-label="Solver stateのメモリ上限使用率"
                          />
                        ) : null}
                        <p className="text-xs text-muted-foreground">
                          {memoryPreflight.headroomBytes
                            ? `余裕 ${formatBytes(memoryPreflight.headroomBytes)}`
                            : memoryPreflight.fitsBudget === false
                              ? "ツリー、bucket数、またはメモリ上限を調整してください。"
                              : "上限と同量です。"}
                        </p>
                      </>
                    ) : (
                      <p className="text-xs text-muted-foreground">
                        policy arena上限を超えたtree
                        prefixで検証を停止しました。
                      </p>
                    )}
                    <p className="border-t pt-2 text-xs text-muted-foreground">
                      この値はpolicy arenaのSolver stateです。公開ツリー、
                      ICM準備領域、abstraction cache、thread scratch、評価、
                      checkpoint stagingを含むプロセス全体のpeakではありません。
                    </p>
                  </div>

                  {economicsPreflight?.kind === "tournament-icm" ? (
                    <div className="space-y-3 rounded-lg border p-3">
                      <div className="flex items-center justify-between gap-2">
                        <div>
                          <p className="text-sm font-medium">Tournament ICM</p>
                          <p className="text-xs text-muted-foreground">
                            {formatInteger(economicsPreflight.fieldPlayers)}
                            人・有賞
                            {formatInteger(economicsPreflight.paidPlaces)}位
                          </p>
                        </div>
                        <Badge variant="secondary">
                          {economicsPreflight.mode === "exact"
                            ? "Exact"
                            : "Sampled"}
                        </Badge>
                      </div>
                      {economicsPreflight.preparedBytes ? (
                        <>
                          <div className="flex items-baseline justify-between gap-2 text-xs">
                            <span>ICM準備領域</span>
                            <strong className="font-mono">
                              {formatBytes(economicsPreflight.preparedBytes)}
                            </strong>
                          </div>
                          <div className="flex items-baseline justify-between gap-2 text-xs">
                            <span className="text-muted-foreground">
                              内部上限
                            </span>
                            <span className="font-mono text-muted-foreground">
                              {economicsPreflight.preparedLimitBytes
                                ? formatBytes(
                                    economicsPreflight.preparedLimitBytes
                                  )
                                : "—"}
                            </span>
                          </div>
                          <Badge
                            variant={
                              economicsPreflight.fitsPreparedLimit === false
                                ? "destructive"
                                : "default"
                            }
                          >
                            {economicsPreflight.fitsPreparedLimit === false
                              ? "上限超過"
                              : `${formatInteger(economicsPreflight.samples ?? "0")} samples`}
                          </Badge>
                        </>
                      ) : (
                        <p className="text-xs text-muted-foreground">
                          15人以下はsubset dynamic programmingによるexact
                          ICMです。sampled race bufferは使いません。
                        </p>
                      )}
                    </div>
                  ) : null}
                </>
              ) : null}

              {validation?.errors.length ? (
                <div className="space-y-2">
                  <p className="text-xs font-medium text-destructive">
                    Validation errors
                  </p>
                  {validation.errors.map((error, index) => (
                    <p
                      className="text-xs text-destructive"
                      key={`${error.path}-${error.code}-${index}`}
                    >
                      <strong>{error.path}</strong>: {error.message}
                    </p>
                  ))}
                </div>
              ) : validation?.valid ? (
                <div className="space-y-2">
                  {[
                    "Config schema v1",
                    "Ranges / collision-free deal",
                    ...(economicsPreflight?.kind === "tournament-icm"
                      ? [
                          economicsPreflight.mode === "exact"
                            ? "Exact ICM economics"
                            : "Sampled ICM preparation fits limit",
                        ]
                      : []),
                    "Betting tree construction",
                    memoryPreflight?.estimateKind === "prefix-lower-bound"
                      ? "Tree prefix lower bound reported"
                      : "Solver state fits memory budget",
                  ].map((item) => (
                    <p
                      className="flex items-center gap-2 text-xs text-muted-foreground"
                      key={item}
                    >
                      <span className="rounded-full bg-emerald-100 p-0.5 text-emerald-700">
                        <IconCheck className="size-3" />
                      </span>
                      {item}
                    </p>
                  ))}
                </div>
              ) : null}

              {validation?.warnings.length ? (
                <div className="space-y-2 rounded-lg border border-amber-300 bg-amber-50 p-3 text-amber-950">
                  <p className="text-xs font-medium">Validation warnings</p>
                  {validation.warnings.map((warning, index) => (
                    <p
                      className="text-xs text-amber-800"
                      key={`${warning.path}-${warning.code}-${index}`}
                    >
                      <strong>{warning.path}</strong>: {warning.message}
                    </p>
                  ))}
                </div>
              ) : null}

              <Alert>
                <IconInfoCircle />
                <AlertTitle>品質の境界</AlertTitle>
                <AlertDescription>
                  {validation?.guaranteeBoundary ??
                    "3人以上はregret-minimized profileであり、認証済みNash/GTO解ではありません。"}
                </AlertDescription>
              </Alert>
            </CardContent>
            <CardFooter className="border-t">
              <Button
                variant="ghost"
                size="sm"
                className="w-full"
                disabled={!nativeReady || busyAction !== null}
                onClick={() => handleOpenResult("run")}
              >
                <IconFolderOpen />
                生成済みrunを開く
              </Button>
            </CardFooter>
          </Card>

          <div className="flex items-start gap-2 rounded-lg border border-dashed p-3 text-xs text-muted-foreground">
            <IconStack2 className="mt-0.5 size-4 shrink-0" />
            <p>
              job作成には検証済みeffective
              TOMLとfingerprintだけを渡します。入力元pathは送信しません。
            </p>
          </div>
        </aside>
      </div>
      {checkpointSource ? (
        <ResumeDialog
          key={checkpointSource.sourceId}
          sourceLabel={checkpointSource.fileName}
          completedSweeps={checkpointSource.completedSweeps}
          suggestedName={checkpointSource.suggestedName}
          busy={busyAction === "checkpoint-resume"}
          onCancel={() => setCheckpointSource(null)}
          onConfirm={handleResumeCheckpoint}
        />
      ) : null}
    </div>
  )
}
