export type AppScreen = "setup" | "solving" | "results"

export type LocalConnectionProfileViewModel = {
  id: "local"
  name: string
  kind: "local"
  transport: "in-process"
  status: "available"
}

export type RemoteConnectionProfileViewModel = {
  id: string
  name: string
  kind: "remote"
  transport: "encrypted-tunnel"
  endpoint: string
  credentialRef?: string
  status: "spec-only"
}

export type ConnectionProfileViewModel =
  LocalConnectionProfileViewModel | RemoteConnectionProfileViewModel

export const localProfile: LocalConnectionProfileViewModel = {
  id: "local",
  name: "このマシン",
  kind: "local",
  transport: "in-process",
  status: "available",
}

export const remoteProfile: RemoteConnectionProfileViewModel = {
  id: "remote-lab",
  name: "計算サーバー",
  kind: "remote",
  transport: "encrypted-tunnel",
  endpoint: "https://solver.internal",
  credentialRef: "os-keychain://solvers/remote-lab",
  status: "spec-only",
}

/*
 * Target product contract
 *
 * This mirrors docs/gui-spec.jp.md. The desktop UI maps the local subset to
 * Tauri commands in native-gateway.ts. Remote transport, credentials, and
 * network adapters remain a specification-only surface.
 */

export type DecimalString = string
export type IntegerString = string
export type UInt64String = string
export type DurationString = string
export type MemoryString = string

type LosslessDraftValue =
  | string
  | boolean
  | null
  | LosslessDraftValue[]
  | { [field: string]: LosslessDraftValue }

export type MultiwayV1Draft = {
  schema: "solvers.multiway-preflop/v1"
  name: string
  /** All numeric input tokens remain strings in this lossless editor tree. */
  document: { [field: string]: LosslessDraftValue }
}

export type ValidationResult = {
  valid: boolean
  errors: Array<{ code: string; path: string; message: string }>
  warnings: Array<{ code: string; path: string; message: string }>
  effectiveConfigToml: string | null
  configFingerprint: string | null
  preflight: {
    threads: string
    abstraction: string
    economics:
      | { kind: "cash" }
      | {
          kind: "tournament-icm"
          fieldPlayers: UInt64String
          paidPlaces: UInt64String
          mode: "exact" | "sampled"
          samples: UInt64String | null
          seed: UInt64String | null
          preparedBytes: UInt64String | null
          preparedLimitBytes: UInt64String | null
          fitsPreparedLimit: boolean | null
        }
    tree: {
      recallMode: "current-street"
      decisionNodes: UInt64String
      terminalEdges: UInt64String | null
      policyColumns: UInt64String | null
      policySlots: UInt64String | null
    }
    memory: {
      estimateKind: "exact-dense" | "prefix-lower-bound"
      solverStateBytes: UInt64String | null
      budgetMode: "auto" | "explicit"
      availableBytes: UInt64String | null
      budgetBytes: UInt64String
      headroomBytes: UInt64String | null
      fitsBudget: boolean | null
    }
  } | null
  guaranteeBoundary: string
  units: { utility: string; chipUnitBb: "0.001" }
}

export type ConnectionProfile =
  | {
      id: string
      kind: "local"
      name: string
    }
  | {
      id: string
      kind: "remote"
      name: string
      baseUrl: string
      credentialRef: string
      serverId: string
    }

export type ArtifactSchemaCapabilities = {
  runJson: number[]
  progressJsonl: number[]
  mwsol: number[]
  mwckpt: number[]
  strategySnapshot: number[]
}

export type BridgeCapabilitiesV3 = {
  service: "solvers"
  serverId: string
  solverVersion: string
  apiVersion: 3
  busy: boolean
  configSchemas: Array<"solvers.multiway-preflop/v1">
  artifactSchemas: ArtifactSchemaCapabilities
  features: {
    liveStrategySnapshots: boolean
    resume: boolean
    durableJobs: boolean
    eventTransports: Array<"sse" | "poll">
  }
  retention: {
    events: "job-lifetime"
    jobs: "operator-managed"
  }
  limits: {
    maxPlayers: number
    maxConcurrentJobs: number
    memoryBytes: UInt64String
  }
}

export type JobState =
  | "queued"
  | "validating"
  | "running"
  | "cancelling"
  | "target-reached"
  | "sweep-limit"
  | "time-limit"
  | "cancelled"
  | "resource-limit"
  | "failed"

export type Estimate = {
  mean: DecimalString
  stderr: DecimalString
  ci95: [DecimalString, DecimalString]
}

export type MultiwayProgressV3 = {
  sweeps: UInt64String
  maxSweeps: UInt64String
  elapsedSecs: DecimalString | null
  stopTarget: DecimalString
  stopTargetUnit: string
  memoryBytes: UInt64String | null
  traversalsPerSecond: DecimalString | null
  handUpdatesPerSecond: DecimalString | null
  infosets: UInt64String | null
  checkpoint: {
    available: boolean
    generatedAt: string | null
  }
  seats: Array<{
    seat: number
    profileEv: Estimate | null
    onlineTrainingEv?: {
      mean: DecimalString
      observations: UInt64String
      totalWeight: DecimalString
    } | null
    averagePositiveRegret: DecimalString | null
    strategyDriftL1: DecimalString | null
    deviationGain: Estimate | null
  }>
}

export type ArtifactDescriptor = {
  available: boolean
  byteLength: UInt64String | null
  sha256: string | null
}

export type JobSnapshot = {
  id: string
  state: JobState
  createdAt: string
  startedAt: string | null
  finishedAt: string | null
  configFingerprint: string
  resumedFromJobId: string | null
  progress: MultiwayProgressV3 | null
  resumeAvailable: boolean
  artifacts: {
    run: ArtifactDescriptor
    progress: ArtifactDescriptor
    solution: ArtifactDescriptor
    checkpoint: ArtifactDescriptor
  }
  error: {
    code: string
    message: string
    retryable: boolean
  } | null
}

export type JobEvent = {
  sequence: UInt64String
  jobId: string
  generatedAt: string
  kind: "state" | "progress" | "checkpoint" | "resource-warning"
  state: JobState
  progress: MultiwayProgressV3 | null
}

export type JobEventPage = {
  events: JobEvent[]
  lastSequence: UInt64String | null
  terminal: boolean
}

export type JobResult = {
  job: JobSnapshot
  terminalStatus:
    | "target-reached"
    | "sweep-limit"
    | "time-limit"
    | "cancelled"
    | "resource-limit"
    | "failed"
  effectiveConfigToml: string
  guaranteeBoundary: string
  units: { utility: string; chipUnitBb: "0.001" }
}

export type JobPage = {
  items: JobSnapshot[]
  nextCursor: string | null
}

export type ExportReceipt = {
  fileName: string
  byteLength: UInt64String
  sha256: string
}

export type CreateJobRequest = {
  idempotencyKey: string
  name: string
  schema: "solvers.multiway-preflop/v1"
  effectiveConfigToml: string
  configFingerprint: string
}

export type ResumeJobRequest = {
  idempotencyKey: string
  name: string
  overrides: {
    maxSweeps?: IntegerString
    maxTime?: DurationString
    stopTarget?: DecimalString | "default"
    evaluationSamples?: IntegerString
    checkEverySweeps?: IntegerString
    checkpointInterval?: DurationString
    threads?: IntegerString | "auto"
    memory?: MemoryString | "auto"
  }
}

export type ExportRequest =
  | {
      kind: "artifact"
      artifact: "run" | "progress" | "solution" | "checkpoint"
    }
  | {
      kind: "view"
      view: "node" | "seats" | "quality"
      format: "json" | "csv"
      nodeId?: string
    }

export type TypedAction = {
  id: string
  semantic: "fold" | "check" | "call" | "bet-to" | "raise-to"
  amountMilliBb: IntegerString | null
  allIn: boolean
  fullRaise: boolean | null
  label: string
}

export type StrategyEntry = {
  id: string
  label: string
  status: "visited" | "unvisited"
  weight: DecimalString
  comboCount: number | null
  bucketPath: number[] | null
  probabilityU16: number[] | null
  ev: { value: DecimalString; unit: string } | null
}

export type StrategySnapshotV1 = {
  schemaVersion: 1
  jobId: string
  revision: UInt64String
  status: "live-average" | "stale" | "final"
  strategyKind: "linear-average"
  generatedAt: string
  asOfSweeps: UInt64String
  currentSweeps: UInt64String
  node: {
    nodeId: string
    actorSeat: number
    street: "preflop" | "flop" | "turn" | "river"
    potMilliBb: IntegerString | null
    activeOpponents: number
    breadcrumb: Array<{
      actorSeat: number
      action: TypedAction
    }>
  }
  actions: TypedAction[]
  view:
    | { kind: "preflop-hand-classes"; entries: StrategyEntry[] }
    | { kind: "postflop-buckets"; entries: StrategyEntry[] }
  approximate: true
  coverage: DecimalString
}

export interface SolveGateway {
  getCapabilities(profileId: string): Promise<BridgeCapabilitiesV3>
  validate(profileId: string, draft: MultiwayV1Draft): Promise<ValidationResult>
  createJob(profileId: string, request: CreateJobRequest): Promise<JobSnapshot>
  listJobs(profileId: string, cursor?: string): Promise<JobPage>
  getJob(profileId: string, jobId: string): Promise<JobSnapshot>
  getEvents(
    profileId: string,
    jobId: string,
    afterSequence?: UInt64String
  ): Promise<JobEventPage>
  getResult(profileId: string, jobId: string): Promise<JobResult>
  getStrategy(
    profileId: string,
    jobId: string,
    nodeId: string
  ): Promise<StrategySnapshotV1>
  exportArtifact(
    profileId: string,
    jobId: string,
    request: ExportRequest
  ): Promise<ExportReceipt>
  cancelJob(profileId: string, jobId: string): Promise<JobSnapshot>
  resumeJob(
    profileId: string,
    jobId: string,
    request: ResumeJobRequest
  ): Promise<JobSnapshot>
}
