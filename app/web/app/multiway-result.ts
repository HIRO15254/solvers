export interface ResultEstimate {
  mean: number;
  stderr: number;
  ci95: [number, number];
}

export interface MultiwaySeatResult {
  seat: number;
  profileEv?: ResultEstimate | null;
  averagePositiveRegret: number;
  strategyDriftL1: number;
  deviationGainLowerBound?: ResultEstimate | null;
}

export interface MultiwayResultV2 {
  schemaVersion: 2;
  kind: "preflop-multiway";
  status: "completed" | "resource_limit" | "cancelled";
  approximateProfile: true;
  approximationNotice: string;
  sweeps: number;
  traversals: number;
  infosets: number;
  memoryBytes: number;
  elapsedSecs: number;
  traversalsPerSecond: number;
  seats: MultiwaySeatResult[];
  strategyBlocks: number;
  configHash: string;
}

export interface MultiwayStrategyBlock {
  key: {
    history: number[];
    actor: number;
    street: number;
    active_opponents: number;
    bucket_path: number[];
  };
  public_history?: Array<{
    actor: number;
    action_index: number;
    action: string;
  }>;
  actions: string[];
  probabilities: number[];
}

export interface StrategyPage {
  items: MultiwayStrategyBlock[];
  nextCursor?: string | null;
}

export function historyId(block: MultiwayStrategyBlock): string {
  return block.key.history
    .map((value) => value.toString(16).padStart(2, "0"))
    .join("");
}

export function nodeId(block: MultiwayStrategyBlock): string {
  return [
    historyId(block),
    block.key.actor,
    block.key.street,
    block.key.active_opponents,
  ].join(":");
}

export function streetLabel(street: number): string {
  return ["Preflop", "Flop", "Turn", "River"][street] ?? `Street ${street}`;
}

export function compactBytes(bytes: number): string {
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KiB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GiB`;
}

export function dominantAction(block?: MultiwayStrategyBlock): {
  action: string;
  probability: number;
  index: number;
} | null {
  if (!block || block.actions.length === 0) return null;
  let index = 0;
  for (let candidate = 1; candidate < block.probabilities.length; candidate += 1) {
    if (block.probabilities[candidate] > block.probabilities[index]) {
      index = candidate;
    }
  }
  return {
    action: block.actions[index] ?? "unknown",
    probability: block.probabilities[index] ?? 0,
    index,
  };
}
