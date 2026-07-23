import type {
  ActionProbabilityViewModel,
  SolveDraftViewModel,
  SolveProgressViewModel,
  StrategyActionViewModel,
  StrategyCellViewModel,
  StrategySnapshotViewModel,
} from "@/lib/solve-contract"

export const demoSession = {
  id: "MW-DEMO-018",
  name: "6-max Cash · 100BB",
  startedAt: "2026/07/23 09:47",
  finishedAt: "2026/07/23 13:45",
  stopStatus: "sweep-limit" as const,
}

export const ranks = [
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

export const demoActions: StrategyActionViewModel[] = [
  {
    id: "raise-2500",
    label: "Raise to 2.500 BB",
    semantic: "raise-to",
    amountMilliBb: 2_500,
  },
  { id: "call", label: "Call", semantic: "call" },
  { id: "fold", label: "Fold", semantic: "fold" },
]

function handLabel(row: number, column: number) {
  if (row === column) {
    return `${ranks[row]}${ranks[column]}`
  }
  if (row < column) {
    return `${ranks[row]}${ranks[column]}s`
  }
  return `${ranks[column]}${ranks[row]}o`
}

function round(value: number) {
  return Math.round(value * 10) / 10
}

function asFixedPoint(
  raisePercent: number,
  callPercent: number
): ActionProbabilityViewModel[] {
  const raise = Math.round((raisePercent / 100) * 65_535)
  const call = Math.round((callPercent / 100) * 65_535)
  return [
    { actionId: "raise-2500", probabilityU16: raise },
    { actionId: "call", probabilityU16: call },
    { actionId: "fold", probabilityU16: 65_535 - raise - call },
  ]
}

export const strategyCells: StrategyCellViewModel[] = ranks.flatMap((_, row) =>
  ranks.map((__, column): StrategyCellViewModel => {
    const hand = handLabel(row, column)
    const combos = row === column ? 6 : row < column ? 4 : 12

    // Deliberately preserve these two classes as truly unvisited. No strategy
    // or EV is synthesized for visual completeness.
    if (row > 10 && column > 10) {
      return {
        hand,
        combos,
        status: "unvisited",
        probabilities: null,
        evMilliBb: null,
        reachWeightU16: null,
      }
    }

    const pairBonus = row === column ? 34 - row * 1.9 : 0
    const suitedBonus = row < column ? 8 : 0
    const connectedBonus = Math.max(0, 6 - Math.abs(row - column)) * 2.1
    const highCard = Math.max(0, 48 - (row + column) * 3.25)
    const raise = round(
      Math.max(
        0,
        Math.min(96, highCard + pairBonus + suitedBonus + connectedBonus - 10)
      )
    )
    const call = round(
      Math.min(
        Math.max(
          0,
          Math.min(56, 42 - Math.abs(row - column) * 4 - (row + column) * 1.25)
        ),
        100 - raise
      )
    )
    const fold = Math.max(0, 100 - raise - call)
    const evBb = (raise * 0.018 + call * 0.006 - fold * 0.001) / 10

    return {
      hand,
      combos,
      status: "visited",
      probabilities: asFixedPoint(raise, call),
      evMilliBb: Math.round(evBb * 1_000),
      reachWeightU16: Math.round(0.842 * 65_535),
    }
  })
)

export const demoSnapshot: StrategySnapshotViewModel = {
  schema: "solvers.strategy-snapshot/v1",
  revision: 324,
  generatedAt: "2026-07-23T12:31:42+09:00",
  status: "live",
  source: "live-average",
  asOfSweeps: 3_240_000,
  street: "preflop",
  node: {
    id: "preflop:utg-r2500:hj",
    publicHistory: ["raise-to:2500:seat-3"],
    label: "Root / UTG raises to 2.500 BB / HJ decision",
    actor: 4,
  },
  actions: demoActions,
  view: "preflop-hand-classes",
  cells: strategyCells,
}

export const convergenceData = [
  { sweep: 0.1, deviation: 0.42, target: 0.05 },
  { sweep: 0.35, deviation: 0.31, target: 0.05 },
  { sweep: 0.7, deviation: 0.244, target: 0.05 },
  { sweep: 1.05, deviation: 0.188, target: 0.05 },
  { sweep: 1.42, deviation: 0.153, target: 0.05 },
  { sweep: 1.8, deviation: 0.129, target: 0.05 },
  { sweep: 2.15, deviation: 0.111, target: 0.05 },
  { sweep: 2.5, deviation: 0.097, target: 0.05 },
  { sweep: 2.85, deviation: 0.086, target: 0.05 },
  { sweep: 3.1, deviation: 0.079, target: 0.05 },
  { sweep: 3.24, deviation: 0.074, target: 0.05 },
]

export const demoProgress: SolveProgressViewModel = {
  sweep: 3_240_000,
  maxSweeps: 5_000_000,
  elapsedSeconds: 9_822,
  sweepBasedRemainingSeconds: 4_731,
  measuredDeviation: 0.074,
  target: 0.05,
  checkpointAgeSeconds: 182,
}

const defaultTree = {
  aggressiveActionCap: 4,
  openToBb: "2.500",
  reraiseMultiplier: "3.000",
  postflopBetFraction: "0.500",
  postflopRaiseFraction: "0.750",
  donkBet: true,
  legalAllIn: true,
} as const

export const demoDraft: SolveDraftViewModel = {
  schema: "solvers.multiway-preflop/v1",
  name: demoSession.name,
  game: {
    kind: "preflop-multiway",
    seatCount: 6,
    button: 0,
    standardBlinds: true,
    commonAnteBb: "0.000",
    seats: Array.from({ length: 6 }, (_, seat) => ({
      seat,
      stackBb: "100.000",
      liveBlindBb: seat === 1 ? "0.500" : seat === 2 ? "1.000" : "0.000",
      anteBb: "0.000",
      range: seat === 3 ? "22+,A2s+,K9s+,QTs+,JTs,ATo+,KQo" : "random",
    })),
    tree: { kind: "standard", rules: defaultTree },
  },
  economics: {
    kind: "cash",
    rake: { enabled: false, percent: "0.000", capBb: "0.000" },
  },
  solver: {
    algorithm: "external-sampling-mccfr",
    rolloutsPerState: 512,
    recall: "current-street",
    abstractionBuckets: [64, 64, 64],
  },
  run: {
    maxSweeps: 5_000_000,
    stopTarget: {
      metric: "measured-deviation-gain",
      valueBbPerHand: "0.050",
      consecutiveConfirmations: 3,
    },
    evaluationSamples: 8_192,
    threads: "auto",
    memory: "auto",
    checkpointEverySweeps: 250_000,
  },
}

export const seatRows = [
  { seat: 0, position: "BTN", stack: 100, ev: 0.42, ci: "±0.03" },
  { seat: 1, position: "SB", stack: 100, ev: -0.31, ci: "±0.04" },
  { seat: 2, position: "BB", stack: 100, ev: -0.67, ci: "±0.04" },
  { seat: 3, position: "UTG", stack: 100, ev: 0.08, ci: "±0.03" },
  { seat: 4, position: "HJ", stack: 100, ev: 0.18, ci: "±0.03" },
  { seat: 5, position: "CO", stack: 100, ev: 0.3, ci: "±0.03" },
]

export const nodeRows = [
  { path: "Root", actor: "UTG", pot: "1.5 BB" },
  { path: "UTG raises 2.5 BB", actor: "HJ", pot: "4.0 BB", active: true },
  { path: "HJ folds", actor: "CO", pot: "4.0 BB" },
  { path: "CO calls", actor: "BTN", pot: "6.5 BB" },
  { path: "BTN raises 10 BB", actor: "SB", pot: "16.5 BB" },
]

export const solveEvents = [
  {
    time: "12:31:42",
    title: "評価 #324",
    detail: "measured deviation 0.074 BB/hand",
  },
  {
    time: "12:27:05",
    title: "Checkpoint",
    detail: "checkpoint.mwckpt · 1.8 GB",
  },
  {
    time: "12:16:41",
    title: "評価 #323",
    detail: "measured deviation 0.079 BB/hand",
  },
  {
    time: "11:52:18",
    title: "Resource fixture",
    detail: "12 threads · peak 9.2 GiB",
  },
]
