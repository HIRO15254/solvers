import { invoke, isTauri } from "@tauri-apps/api/core"

import type {
  ArtifactDescriptor,
  Estimate,
  ExportReceipt,
  JobEvent,
  JobEventPage,
  JobResult,
  JobSnapshot,
  MultiwayProgressV3,
  ResumeJobRequest,
  StrategyEntry,
  TypedAction,
  ValidationResult,
} from "@/lib/solve-contract"

export type ArtifactKind = "run" | "progress" | "solution" | "checkpoint"

export type LoadedConfig = {
  sourceId: string
  fileName: string
  configToml: string
}

export type LoadedTreeSource = {
  sourceId: string
  fileName: string
}

export type NativeAvailability = {
  available: boolean
  reason: string | null
}

export type NativeEstimate = Estimate
export type NativeSeatMetrics = MultiwayProgressV3["seats"][number]
export type NativeProgress = MultiwayProgressV3
export type NativeArtifactDescriptor = ArtifactDescriptor
export type NativeJobSnapshot = JobSnapshot
export type NativeProgressEvent = JobEvent
export type NativeProgressEventPage = JobEventPage
export type NativeJobResult = JobResult
export type NativeFileReceipt = ExportReceipt
export type NativeResumeOverrides = ResumeJobRequest["overrides"]

export type CheckpointSource = {
  sourceId: string
  fileName: string
  sizeBytes: string
  modifiedAt: string | null
  completedSweeps: string
  configFingerprint: string
  suggestedName: string
}

type NativeTypedAction = {
  id: string
  semantic: TypedAction["semantic"]
  amountMilliBb: number | string | null
  allIn: boolean
  fullRaise: boolean | null
  label: string
}

type NativeStrategyEntry = {
  id: string
  label: string
  status: StrategyEntry["status"]
  weight: number | string
  comboCount: number | null
  bucketPath: number[] | null
  probabilityU16: number[] | null
  ev: { value: number | string; unit: string } | null
}

type NativeStrategySnapshot = {
  schemaVersion: number
  jobId: string
  revision: number | string
  status: "live-average" | "stale" | "final"
  strategyKind: "linear-average"
  generatedAt: string
  asOfSweeps: string
  currentSweeps: string
  node: {
    nodeId: string
    actorSeat: number
    street: "preflop" | "flop" | "turn" | "river"
    potMilliBb: number | string | null
    activeOpponents: number
    breadcrumb: Array<{ actorSeat: number; action: NativeTypedAction }>
  }
  actions: NativeTypedAction[]
  view: {
    kind: "preflop-hand-classes" | "postflop-buckets"
    entries: NativeStrategyEntry[]
  }
  approximate: boolean
  coverage: number | string
}

export type LocalStrategySnapshot = {
  schemaVersion: 1
  jobId: string
  revision: string
  status: "live-average" | "stale" | "final"
  strategyKind: "linear-average"
  generatedAt: string
  asOfSweeps: string
  currentSweeps: string
  node: {
    nodeId: string
    actorSeat: number
    street: "preflop" | "flop" | "turn" | "river"
    potMilliBb: string | null
    activeOpponents: number
    breadcrumb: Array<{ actorSeat: number; actionLabel: string }>
  }
  actions: TypedAction[]
  view: {
    kind: "preflop-hand-classes" | "postflop-buckets"
    entries: StrategyEntry[]
  }
  approximate: true
  coverage: string
}

export interface DesktopSolveGateway {
  readonly availability: NativeAvailability
  loadConfig(): Promise<LoadedConfig | null>
  pickTreeScript(): Promise<LoadedTreeSource | null>
  saveConfig(
    configToml: string,
    suggestedName: string
  ): Promise<NativeFileReceipt | null>
  validateConfig(
    configToml: string,
    sourceId?: string
  ): Promise<ValidationResult>
  startJob(
    name: string,
    effectiveConfigToml: string,
    configFingerprint: string
  ): Promise<NativeJobSnapshot>
  getJob(jobId: string): Promise<NativeJobSnapshot>
  getEvents(
    jobId: string,
    afterSequence?: string
  ): Promise<NativeProgressEventPage>
  getResult(jobId: string): Promise<NativeJobResult>
  getStrategy(jobId: string, nodeId?: string): Promise<LocalStrategySnapshot>
  cancelJob(jobId: string): Promise<NativeJobSnapshot>
  resumeJob(
    jobId: string,
    name: string,
    overrides: NativeResumeOverrides
  ): Promise<NativeJobSnapshot>
  pickCheckpoint(): Promise<CheckpointSource | null>
  resumeCheckpoint(
    sourceId: string,
    name: string,
    overrides: NativeResumeOverrides
  ): Promise<NativeJobSnapshot>
  openRun(): Promise<NativeJobSnapshot | null>
  openSolution(): Promise<NativeJobSnapshot | null>
  exportArtifact(
    jobId: string,
    artifact: ArtifactKind
  ): Promise<NativeFileReceipt | null>
}

export class NativeUnavailableError extends Error {
  constructor() {
    super("この操作はSolversデスクトップ版でのみ利用できます。")
    this.name = "NativeUnavailableError"
  }
}

function unwrapNullable<T>(value: T | { value: T } | null): T | null {
  if (value === null) {
    return null
  }
  if (typeof value === "object" && value !== null && "value" in value) {
    return value.value
  }
  return value
}

function unwrapJob(
  value: NativeJobSnapshot | { job: NativeJobSnapshot } | null
): NativeJobSnapshot | null {
  if (value === null) {
    return null
  }
  return "job" in value ? value.job : value
}

function normalizeStrategy(
  source: NativeStrategySnapshot
): LocalStrategySnapshot {
  return {
    schemaVersion: 1,
    jobId: source.jobId,
    revision: String(source.revision),
    status: source.status,
    strategyKind: source.strategyKind,
    generatedAt: source.generatedAt,
    asOfSweeps: source.asOfSweeps,
    currentSweeps: source.currentSweeps,
    node: {
      ...source.node,
      potMilliBb:
        source.node.potMilliBb === null ? null : String(source.node.potMilliBb),
      breadcrumb: source.node.breadcrumb.map((item) => ({
        actorSeat: item.actorSeat,
        actionLabel: item.action.label,
      })),
    },
    actions: source.actions.map((action) => ({
      ...action,
      amountMilliBb:
        action.amountMilliBb === null ? null : String(action.amountMilliBb),
    })),
    view: {
      kind: source.view.kind,
      entries: source.view.entries.map((entry) => ({
        ...entry,
        weight: String(entry.weight),
        ev: entry.ev
          ? { value: String(entry.ev.value), unit: entry.ev.unit }
          : null,
      })),
    },
    approximate: true,
    coverage: String(source.coverage),
  }
}

export function errorMessage(error: unknown) {
  if (error instanceof Error) {
    return error.message
  }
  if (typeof error === "string") {
    return error
  }
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return error.message
  }
  return "不明なエラーが発生しました。"
}

export function createNativeGateway(): DesktopSolveGateway {
  const native = isTauri()
  const availability: NativeAvailability = native
    ? { available: true, reason: null }
    : {
        available: false,
        reason:
          "ブラウザpreviewではnative Solverとファイルダイアログを利用できません。",
      }

  const invokeNative = async <T>(
    command: string,
    args?: Record<string, unknown>
  ): Promise<T> => {
    if (!native) {
      throw new NativeUnavailableError()
    }
    return invoke<T>(command, args)
  }

  return {
    availability,

    async loadConfig() {
      return unwrapNullable(
        await invokeNative<LoadedConfig | { value: LoadedConfig } | null>(
          "local_pick_config"
        )
      )
    },

    async pickTreeScript() {
      return unwrapNullable(
        await invokeNative<
          LoadedTreeSource | { value: LoadedTreeSource } | null
        >("local_pick_tree_script")
      )
    },

    async saveConfig(configToml, suggestedName) {
      return unwrapNullable(
        await invokeNative<
          NativeFileReceipt | { value: NativeFileReceipt } | null
        >("local_save_config_dialog", { configToml, suggestedName })
      )
    },

    validateConfig(configToml, sourceId) {
      return invokeNative<ValidationResult>("local_validate_config", {
        configToml,
        sourceId,
      })
    },

    startJob(name, effectiveConfigToml, configFingerprint) {
      return invokeNative<NativeJobSnapshot>("local_start_job", {
        name,
        effectiveConfigToml,
        configFingerprint,
      })
    },

    getJob(jobId) {
      return invokeNative<NativeJobSnapshot>("local_get_job", { jobId })
    },

    getEvents(jobId, afterSequence) {
      return invokeNative<NativeProgressEventPage>("local_get_progress", {
        jobId,
        afterSequence,
      })
    },

    getResult(jobId) {
      return invokeNative<NativeJobResult>("local_get_result", { jobId })
    },

    async getStrategy(jobId, nodeId) {
      const source = await invokeNative<NativeStrategySnapshot>(
        "local_get_strategy",
        { jobId, nodeId }
      )
      return normalizeStrategy(source)
    },

    cancelJob(jobId) {
      return invokeNative<NativeJobSnapshot>("local_cancel_job", { jobId })
    },

    resumeJob(jobId, name, overrides) {
      return invokeNative<NativeJobSnapshot>("local_resume_job", {
        jobId,
        name,
        overrides,
      })
    },

    async pickCheckpoint() {
      return unwrapNullable(
        await invokeNative<
          CheckpointSource | { value: CheckpointSource } | null
        >("local_pick_checkpoint")
      )
    },

    resumeCheckpoint(sourceId, name, overrides) {
      return invokeNative<NativeJobSnapshot>("local_resume_checkpoint", {
        sourceId,
        name,
        overrides,
      })
    },

    async openRun() {
      return unwrapJob(
        await invokeNative<
          NativeJobSnapshot | { job: NativeJobSnapshot } | null
        >("local_pick_run")
      )
    },

    async openSolution() {
      return unwrapJob(
        await invokeNative<
          NativeJobSnapshot | { job: NativeJobSnapshot } | null
        >("local_pick_solution")
      )
    },

    async exportArtifact(jobId, artifact) {
      return unwrapNullable(
        await invokeNative<
          NativeFileReceipt | { value: NativeFileReceipt } | null
        >("local_export_artifact_dialog", { jobId, artifact })
      )
    },
  }
}
