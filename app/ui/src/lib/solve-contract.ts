export type AppScreen = "setup" | "solving" | "results"

/*
 * Visual fixture models
 *
 * These types make the implemented screen fixtures internally consistent.
 * They are deliberately named separately from the target v3 wire contract
 * below: no fixture object is valid evidence of a real Solve.
 */

export type LocalConnectionProfileViewModel = {
  id: "local"
  name: string
  kind: "local"
  transport: "in-process"
  status: "fixture-only"
}

export type RemoteConnectionProfileViewModel = {
  id: string
  name: string
  kind: "remote"
  transport: "encrypted-tunnel"
  endpoint: string
  credentialRef?: string
  status: "contract-only"
}

export type ConnectionProfileViewModel =
  LocalConnectionProfileViewModel | RemoteConnectionProfileViewModel

/** Exact BB tokens stay strings until the future Rust normalizer accepts them. */
export type DecimalBb = string

export type SeatConfigViewModel = {
  seat: number
  stackBb: DecimalBb
  liveBlindBb: DecimalBb
  anteBb: DecimalBb
  range: string
}

export type MaterializedTreeViewModel = {
  aggressiveActionCap: number
  openToBb: DecimalBb
  reraiseMultiplier: DecimalBb
  postflopBetFraction: DecimalBb
  postflopRaiseFraction: DecimalBb
  donkBet: boolean
  legalAllIn: boolean
}

export type SolveDraftViewModel = {
  schema: "solvers.multiway-preflop/v1"
  name: string
  game: {
    kind: "preflop-multiway"
    seatCount: number
    button: number
    standardBlinds: boolean
    firstToAct?: number
    commonAnteBb: DecimalBb
    seats: SeatConfigViewModel[]
    tree:
      | { kind: "standard"; rules: MaterializedTreeViewModel }
      | {
          kind: "script"
          sourceName: string
          materializedRules: MaterializedTreeViewModel
        }
  }
  economics:
    | {
        kind: "cash"
        rake: {
          enabled: boolean
          percent: DecimalBb
          capBb: DecimalBb
        }
      }
    | {
        kind: "tournament-icm"
        payouts: DecimalBb[]
      }
  solver: {
    algorithm: "external-sampling-mccfr"
    rolloutsPerState: number
    recall: "current-street" | "bucket-history"
    abstractionBuckets: [number, number, number]
  }
  run: {
    maxSweeps: number
    stopTarget: {
      metric: "measured-deviation-gain"
      valueBbPerHand: DecimalBb
      consecutiveConfirmations: number
    }
    evaluationSamples: number
    threads: "auto" | number
    memory: "auto" | string
    checkpointEverySweeps: number
  }
}

export type SolveProgressViewModel = {
  sweep: number
  maxSweeps: number
  elapsedSeconds: number
  /** A sweep-throughput projection, never a convergence prediction. */
  sweepBasedRemainingSeconds?: number
  measuredDeviation: number
  target: number
  checkpointAgeSeconds: number
}

export type ActionSemantic =
  "bet-to" | "raise-to" | "all-in" | "check" | "call" | "fold"

export type StrategyActionViewModel = {
  id: string
  label: string
  semantic: ActionSemantic
  amountMilliBb?: number
}

export type ActionProbabilityViewModel = {
  actionId: string
  probabilityU16: number
}

export type StrategyCellViewModel =
  | {
      hand: string
      combos: number
      status: "visited"
      probabilities: ActionProbabilityViewModel[]
      evMilliBb: number
      reachWeightU16?: number
    }
  | {
      hand: string
      combos: number
      status: "unvisited"
      probabilities: null
      evMilliBb: null
      reachWeightU16?: null
    }

export type StrategyBucketViewModel = {
  bucketId: string
  label: string
  status: "visited" | "unvisited"
  probabilities: ActionProbabilityViewModel[] | null
  evMilliBb: number | null
  reachWeightU16: number | null
}

type StrategySnapshotViewModelBase = {
  schema: "solvers.strategy-snapshot/v1"
  revision: number
  generatedAt: string
  status: "live" | "stale" | "final"
  source: "live-average" | "solution"
  asOfSweeps: number
  street: "preflop" | "flop" | "turn" | "river"
  node: {
    id: string
    publicHistory: string[]
    label: string
    actor: number
  }
  actions: StrategyActionViewModel[]
}

export type StrategySnapshotViewModel = StrategySnapshotViewModelBase &
  (
    | {
        view: "preflop-hand-classes"
        cells: StrategyCellViewModel[]
      }
    | {
        view: "postflop-abstraction-buckets"
        buckets: StrategyBucketViewModel[]
      }
  )

export const localProfile: LocalConnectionProfileViewModel = {
  id: "local",
  name: "このマシン",
  kind: "local",
  transport: "in-process",
  status: "fixture-only",
}

export const remoteProfile: RemoteConnectionProfileViewModel = {
  id: "remote-lab",
  name: "計算サーバー",
  kind: "remote",
  transport: "encrypted-tunnel",
  endpoint: "https://solver.internal",
  credentialRef: "os-keychain://solvers/remote-lab",
  status: "contract-only",
}

/*
 * Target product contract
 *
 * This mirrors docs/gui-spec.jp.md. It is a compile-time contract sketch only;
 * no LocalGateway, RemoteGateway, credential, filesystem, or network adapter is
 * implemented in this UI turn. The Japanese specification remains normative.
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
    peakMemoryBytes: UInt64String
    abstraction: string
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
  mean: number
  stderr: number
  ci95: [number, number]
}

export type MultiwayProgressV3 = {
  sweeps: UInt64String
  maxSweeps: UInt64String
  elapsedSecs: number
  stopTarget: number
  stopTargetUnit: string
  memoryBytes: UInt64String
  traversalsPerSecond: number
  handUpdatesPerSecond: number
  infosets: UInt64String
  checkpoint: {
    available: boolean
    generatedAt: string | null
  }
  seats: Array<{
    seat: number
    profileEv: Estimate | null
    averagePositiveRegret: number
    strategyDriftL1: number
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
  amountMilliBb: number | null
  allIn: boolean
  fullRaise: boolean | null
  label: string
}

export type StrategyEntry = {
  id: string
  label: string
  status: "visited" | "unvisited"
  weight: number
  comboCount: number | null
  bucketPath: number[] | null
  probabilityU16: number[] | null
  ev: { value: number; unit: string } | null
}

export type StrategySnapshotV1 = {
  schemaVersion: 1
  jobId: string
  revision: number
  status: "live-average" | "stale" | "final"
  strategyKind: "linear-average"
  generatedAt: string
  asOfSweeps: UInt64String
  currentSweeps: UInt64String
  node: {
    nodeId: string
    actorSeat: number
    street: "preflop" | "flop" | "turn" | "river"
    potMilliBb: number
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
  coverage: number
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
