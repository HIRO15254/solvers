// Wire types of `solversd` protocol v1 (crates/protocol). Field names follow
// the daemon's camelCase; CLI pass-through payloads keep the CLI's names.

export const PROTOCOL_VERSION = 1;

export type RunState = "queued" | "running" | "completed" | "failed" | "canceled" | "interrupted";

export function isTerminal(state: RunState): boolean {
  return state !== "queued" && state !== "running";
}

export interface ServerInfo {
  protocolVersion: number;
  cliVersion: string;
  maxConcurrentRuns: number;
}

export interface RunSummary {
  runId: string;
  state: RunState;
  relockedFrom?: string;
  gameKind: string;
  configSchema?: string;
  configHash: string;
  completion?: string;
  failure?: string;
  /** The run's last `progress.jsonl` row, as written (P1: see `HuProgressRow`). */
  lastProgress?: Record<string, number>;
  progress?: number;
  elapsedSecs?: number;
  eventsOffset: number;
  resumable: boolean;
}

export type RunEvent = { seq: number; unixMs: number; level: "info" | "warn" | "error" } & (
  | { kind: "state"; state: RunState }
  | { kind: "checkpoint"; sweeps: number }
  | { kind: "stop"; reason: string }
  | { kind: "notice"; message: string }
  | { kind: "failure"; message: string }
);

export interface EventPage {
  events: RunEvent[];
  nextOffset: number;
  terminal: boolean;
}

export interface ArtifactEntry {
  name: string;
  bytes: number;
}

export interface CreateRunResponse {
  runId: string;
  state: RunState;
}

export interface RelockRequest {
  profileToml: string;
  replace: boolean;
  freezeUpstream: boolean;
  clearProfile: boolean;
}

export interface ValidateResponse {
  effectiveConfigToml: string;
  summary: ValidateSummary;
}

export type ErrorCode =
  | "unauthorized"
  | "not-found"
  | "invalid-config"
  | "config-not-self-contained"
  | "invalid-request"
  | "conflict"
  | "unavailable"
  | "internal";

// --- `solvers validate --format json` (docs/cli-reference.jp.md) ---------

export type LineAction =
  | "fold"
  | "check"
  | { call: { amount: number; all_in: boolean } }
  | { "bet-to": { to: number; all_in: boolean; full_raise: boolean } }
  | { "raise-to": { to: number; all_in: boolean; full_raise: boolean } }
  | Record<string, unknown>;

export interface LineStep {
  action: LineAction;
  implicit: boolean;
  position: string;
  seat: number;
  street: string;
}

// --- `solvers line --format json` (docs/cli-reference.jp.md) -------------

export type LineOption =
  | { kind: "fold" | "check"; token: string }
  | { kind: "call" | "allIn"; token: string; amount: number }
  | { kind: "bet" | "raise"; token: string; min: number; max: number };

export interface LineNext {
  seat: number;
  position: string;
  street: string;
  toCall: number;
  stack: number;
  wager: number;
  options: LineOption[];
}

export interface LineSeat {
  seat: number;
  position: string;
  /** Remaining stack, BB. */
  stack: number;
  /** Current-street wager, BB. */
  wager: number;
  /** Everything put in this hand (antes and blinds included), BB. */
  committed: number;
  status: "active" | "folded" | "allIn";
}

export interface LineCursor {
  /** Number of table chips per big blind. */
  bbChips: number;
  /** Explicit moves in acting order, folds included. */
  moves: string[];
  /** Canonical `spot.line` text for `moves` (folds omitted). */
  line: string;
  actions: LineStep[];
  street: string;
  pot: number;
  /** `line` is a valid `spot.line` that starts a postflop street. */
  complete: boolean;
  boardCards: number | null;
  players: number;
  next: LineNext | null;
  terminal: boolean;
  /** Seat id of the button. */
  button: number;
  /** Every seat in table order (clockwise). */
  seats: LineSeat[];
}

export interface StartSeat {
  position: string;
  seat: number;
}

export interface StartState {
  board: string[];
  street: string;
  pot: number;
  effective_stack: number;
  oop: StartSeat;
  ip: StartSeat;
  previous_street_aggressor: number | null;
  seats: { seat: number; position: string; starting_stack: number; remaining_stack: number; folded: boolean }[];
}

export interface HuResources {
  nodes: number;
  terminals: number;
  storage: string;
  f32Bytes: number;
  i16Bytes: number;
  i16F32avgBytes: number;
  memoryEstimateBytes: number;
  memoryLimitBytes: number;
  withinLimit: boolean;
}

export interface TreeParam {
  name: string;
  kind: "number" | "bool" | "token";
  value: string;
  description: string | null;
}

export interface ValidateSummary {
  status: string;
  schema: string;
  gameKind: string;
  product: string;
  amountUnit: string;
  utilityUnit: string;
  start?: StartState;
  actions?: LineStep[];
  effective_stack?: number | null;
  tree?: { params: TreeParam[]; rules: string[] };
  warnings: string[];
  resources?: HuResources;
}

// --- P1 `solvers export` views (docs/hu-postflop.jp.md §8) ---------------

export type NodeKind = "action" | "chance" | "terminal";
export type Seat = "oop" | "ip";

export interface TreeRow {
  history: string;
  kind: NodeKind;
  street: string;
  actor: Seat | null;
  pot: number | null;
  actions: string[];
  children: string[];
  child_kinds: NodeKind[];
  stored: boolean;
}

/** One node of the tree a config defines, before solving (`POST /v1/tree`). */
export type PreviewTreeRow = Omit<TreeRow, "stored">;

export interface StrategyRow {
  history: string;
  actor: Seat;
  combo: string;
  weight: number;
  probabilities: number[];
}

export interface ValueRow {
  history: string;
  seat: Seat;
  combo: string;
  weight: number;
  ev: number;
}

export interface ActionRow {
  history: string;
  street: string;
  actor: Seat;
  action: string;
  frequency: number;
}

/** The `actions`, `strategy` and `ev` views of the same nodes from one export (`/solution/decision`). */
export interface DecisionRows {
  actions: ActionRow[];
  strategy: StrategyRow[];
  ev: ValueRow[];
}

/** Profile diagnostics stored in a P1 solution. Entries index the corresponding config arrays. */
export interface ProfileNode {
  node: number;
  history: string;
  kind: "lock" | "incentive" | "frequency" | "frozen";
  entry: number | null;
  confidence: number | null;
  partial: boolean | null;
}

export interface ProfileReport {
  summary: {
    matched_nodes: number;
    matched_hands: number;
    iso_restricted: boolean;
    selectors: { kind: string; node: string; matched_nodes: number; matched_hands: number }[];
    resolved_nodes: ProfileNode[];
  };
  frozen_nodes: number[];
  general_sum: boolean;
  frequencies: {
    entry: number;
    node: number;
    target: number;
    achieved: number | null;
    error: number | null;
    lambda: number;
  }[];
}

export interface SolSummary {
  board: string;
  pot: number;
  effective_stack: number;
  min_bet: number;
  iterations: number;
  ev_oop: number;
  ev_ip: number;
  expl_oop: number;
  expl_ip: number;
  nash_conv: number;
  storage: string;
  wall_secs: number;
  streets_stored: string;
  nodes: number;
  stored_nodes: number;
  profile?: ProfileReport | null;
  profile_config?: Record<string, unknown>;
}

/** One `progress.jsonl` row of a P1 run (docs/hu-postflop.jp.md §6). */
export interface HuProgressRow {
  iteration: number;
  elapsed_secs: number;
  expl_p0: number;
  expl_p1: number;
  nash_conv: number;
}
