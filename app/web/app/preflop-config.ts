export type PostflopModel = "equity" | "bucketed";
export type StorageKind = "f32" | "i16";
export type WorkbenchMode = "hu" | "multiway";
export type AnteMode = "none" | "ante" | "big-blind-ante";
export type UtilityMode = "cash" | "icm";
export type IcmMethod = "auto" | "exact" | "sampled";
export type ScheduleKind =
  | "vanilla"
  | "cfr-plus"
  | "dcfr"
  | "linear-cfr"
  | "hs-dcfr";
export type RakeMode = "none" | "percent-cap" | "gg-preflop";

export interface StreetValues<T> {
  flop: T;
  turn: T;
  river: T;
}

export interface MultiwayStreetBetting {
  betSizes: number[];
  raiseSizes: number[];
  maxAggressiveActions: number;
  includeAllin: boolean;
}

export interface MultiwaySeatBetting {
  openSizesBb: number[];
  isolateSizesBb: number[];
  raiseFactors: number[];
  postflop: StreetValues<MultiwayStreetBetting>;
}

export interface MultiwaySeat {
  id: string;
  position: string;
  stackBb: number;
  range: string;
  betting?: MultiwaySeatBetting;
}

export interface MultiwayBucketProfile {
  activePlayers: number;
  preflop: number;
  flop: number;
  turn: number;
  river: number;
}

export interface PreflopSettings {
  mode: WorkbenchMode;
  effectiveStackBb: number;
  sbBb: number;
  openSizesBb: number[];
  isolateSizesBb: number[];
  raiseFactors: number[][];
  maxRaises: number;
  includeAllin: boolean;
  allowLimp: boolean;
  sbRange: string;
  bbRange: string;
  equityRealization: {
    sb: number;
    bb: number;
  };
  postflopModel: PostflopModel;
  buckets: StreetValues<number>;
  postflopBetSizes: StreetValues<number[]>;
  multiwayPostflopBetting: StreetValues<MultiwayStreetBetting>;
  iterations: number;
  checkEvery: number;
  storage: StorageKind;
  schedule: ScheduleKind;
  cacheEnabled: boolean;
  rakeMode: RakeMode;
  rakeRate: number;
  rakeCap: number;
  noFlopNoDrop: boolean;
  tableSize: number;
  seats: MultiwaySeat[];
  anteMode: AnteMode;
  anteBb: number;
  utilityMode: UtilityMode;
  payoutsText: string;
  outsideStacksText: string;
  icmMethod: IcmMethod;
  icmSamples: number;
  icmSeed: number;
  bucketProfiles: MultiwayBucketProfile[];
  externalSamplingSweeps: number;
  externalSamplingSeed: number;
  checkpointEvery: number;
  evaluationCadence: number;
  evaluationSamples: number;
  maxMemoryBytes: number;
  resumeCheckpoint: string;
}

export interface PreflopPreset {
  id: string;
  name: string;
  description: string;
  settings: PreflopSettings;
}

const POSITION_TABLE: Record<number, string[]> = {
  3: ["BTN", "SB", "BB"],
  4: ["CO", "BTN", "SB", "BB"],
  5: ["HJ", "CO", "BTN", "SB", "BB"],
  6: ["UTG", "HJ", "CO", "BTN", "SB", "BB"],
  7: ["UTG", "LJ", "HJ", "CO", "BTN", "SB", "BB"],
  8: ["UTG", "MP", "LJ", "HJ", "CO", "BTN", "SB", "BB"],
  9: ["UTG", "UTG+1", "MP", "LJ", "HJ", "CO", "BTN", "SB", "BB"],
};

export function positionsForTableSize(tableSize: number): string[] {
  return [...(POSITION_TABLE[tableSize] ?? POSITION_TABLE[9])];
}

export function createMultiwaySeats(tableSize: number, stackBb = 30): MultiwaySeat[] {
  return positionsForTableSize(tableSize).map((position, index) => ({
    id: `seat-${index}`,
    position,
    stackBb,
    range: "",
    betting: undefined,
  }));
}

export function createBucketProfiles(
  tableSize: number,
  postflopBuckets = 64,
): MultiwayBucketProfile[] {
  return Array.from({ length: tableSize - 1 }, (_, index) => ({
    activePlayers: index + 2,
    preflop: 169,
    flop: postflopBuckets,
    turn: postflopBuckets,
    river: postflopBuckets,
  }));
}

function createMultiwayPostflopBetting(
  betSizes: StreetValues<number[]>,
  raiseSizes: StreetValues<number[]> = betSizes,
  maxAggressiveActions = 2,
  includeAllin = true,
): StreetValues<MultiwayStreetBetting> {
  return Object.fromEntries(
    (["flop", "turn", "river"] as const).map((street) => [
      street,
      {
        betSizes: [...betSizes[street]],
        raiseSizes: [...raiseSizes[street]],
        maxAggressiveActions,
        includeAllin,
      },
    ]),
  ) as unknown as StreetValues<MultiwayStreetBetting>;
}

const DEFAULT_MULTIWAY_MEMORY_BYTES = 2 * 1024 * 1024 * 1024;

const standard100Bb = (): PreflopSettings => ({
  mode: "hu",
  effectiveStackBb: 100,
  sbBb: 0.5,
  openSizesBb: [2.5],
  isolateSizesBb: [3.5],
  raiseFactors: [[3], [2.5]],
  maxRaises: 4,
  includeAllin: true,
  allowLimp: true,
  sbRange: "",
  bbRange: "",
  equityRealization: { sb: 1, bb: 1 },
  postflopModel: "equity",
  buckets: { flop: 50, turn: 20, river: 8 },
  postflopBetSizes: { flop: [0.5], turn: [0.75], river: [0.75] },
  multiwayPostflopBetting: createMultiwayPostflopBetting({
    flop: [0.5],
    turn: [0.75],
    river: [0.75],
  }),
  iterations: 5_000,
  checkEvery: 250,
  storage: "f32",
  schedule: "dcfr",
  cacheEnabled: true,
  rakeMode: "none",
  rakeRate: 0.05,
  rakeCap: 3,
  noFlopNoDrop: true,
  tableSize: 3,
  seats: createMultiwaySeats(3),
  anteMode: "none",
  anteBb: 0.125,
  utilityMode: "cash",
  payoutsText: "50\n30\n20",
  outsideStacksText: "",
  icmMethod: "auto",
  icmSamples: 10_000,
  icmSeed: 7,
  bucketProfiles: createBucketProfiles(3),
  externalSamplingSweeps: 250_000,
  externalSamplingSeed: 7,
  checkpointEvery: 25_000,
  evaluationCadence: 25_000,
  evaluationSamples: 32,
  maxMemoryBytes: DEFAULT_MULTIWAY_MEMORY_BYTES,
  resumeCheckpoint: "",
});

const bucketed100Bb = (): PreflopSettings => ({
  ...standard100Bb(),
  postflopModel: "bucketed",
  iterations: 2_000,
  checkEvery: 100,
});

const pushFold10Bb = (): PreflopSettings => ({
  ...standard100Bb(),
  effectiveStackBb: 10,
  openSizesBb: [],
  raiseFactors: [],
  maxRaises: 1,
  allowLimp: false,
  iterations: 2_000,
  checkEvery: 200,
});

function multiwayBase(tableSize: number, stackBb: number): PreflopSettings {
  return {
    ...bucketed100Bb(),
    mode: "multiway",
    effectiveStackBb: stackBb,
    tableSize,
    seats: createMultiwaySeats(tableSize, stackBb),
    postflopModel: "bucketed",
    buckets: { flop: 64, turn: 64, river: 64 },
    bucketProfiles: createBucketProfiles(tableSize, 64),
    postflopBetSizes: { flop: [0.5], turn: [0.75], river: [0.75] },
    multiwayPostflopBetting: createMultiwayPostflopBetting({
      flop: [0.5],
      turn: [0.75],
      river: [0.75],
    }),
    maxRaises: 2,
    raiseFactors: [[3], [2.5]],
    utilityMode: "cash",
    rakeMode: "none",
    externalSamplingSweeps: 250_000,
    checkpointEvery: 25_000,
    evaluationCadence: 25_000,
  };
}

const pushFold9Max = (): PreflopSettings => ({
  ...multiwayBase(9, 10),
  openSizesBb: [],
  isolateSizesBb: [],
  raiseFactors: [],
  maxRaises: 1,
  allowLimp: false,
  externalSamplingSweeps: 100_000,
  checkpointEvery: 10_000,
  evaluationCadence: 10_000,
  bucketProfiles: createBucketProfiles(9, 32),
});

const mtt9Max = (): PreflopSettings => {
  const settings = multiwayBase(9, 20);
  return {
    ...settings,
    seats: settings.seats.map((seat, index) => ({
      ...seat,
      stackBb: [14, 16, 18, 20, 22, 24, 27, 31, 36][index],
    })),
    anteMode: "big-blind-ante",
    anteBb: 1,
    utilityMode: "icm",
    payoutsText: "100\n70\n50\n35\n25\n18\n12\n8\n5",
    outsideStacksText: "18\n26\n34\n42\n55\n70",
    icmMethod: "auto",
    externalSamplingSweeps: 1_000_000,
    checkpointEvery: 50_000,
    evaluationCadence: 50_000,
    bucketProfiles: createBucketProfiles(9, 64),
  };
};

const cash6Max = (): PreflopSettings => {
  const settings = multiwayBase(6, 100);
  return {
    ...settings,
    externalSamplingSweeps: 1_000_000,
    checkpointEvery: 50_000,
    evaluationCadence: 50_000,
    bucketProfiles: createBucketProfiles(6, 64),
  };
};

const research9Max = (): PreflopSettings => {
  const settings = multiwayBase(9, 100);
  return {
    ...settings,
    openSizesBb: [2, 2.25, 2.5],
    postflopBetSizes: {
      flop: [0.33, 0.5, 0.75],
      turn: [0.5, 0.75, 1.25],
      river: [0.5, 0.75, 1.5],
    },
    multiwayPostflopBetting: createMultiwayPostflopBetting(
      {
        flop: [0.33, 0.5, 0.75],
        turn: [0.5, 0.75, 1.25],
        river: [0.5, 0.75, 1.5],
      },
      {
        flop: [0.5, 0.75, 1.25],
        turn: [0.75, 1.25],
        river: [0.75, 1.5],
      },
      3,
    ),
    maxRaises: 3,
    externalSamplingSweeps: 5_000_000,
    checkpointEvery: 100_000,
    evaluationCadence: 100_000,
    bucketProfiles: createBucketProfiles(9, 200),
  };
};

/** The CLI's `examples/preflop_hu_100bb.toml`, expressed as UI state. */
export const DEFAULT_SETTINGS: PreflopSettings = standard100Bb();

/** Presets mirror the checked-in preflop CLI examples. */
export const PRESETS: PreflopPreset[] = [
  {
    id: "equity-100bb",
    name: "100bb · Equity showdown",
    description: "Fast 169-class trunk with equity-realization continuations.",
    settings: standard100Bb(),
  },
  {
    id: "bucketed-100bb",
    name: "100bb · Bucketed blueprint",
    description: "EHS² 50/20/8 postflop blueprint with one size per street.",
    settings: bucketed100Bb(),
  },
  {
    id: "pushfold-10bb",
    name: "10bb · Push / fold",
    description: "Exact two-action short-stack game: the SB can jam or fold.",
    settings: pushFold10Bb(),
  },
  {
    id: "multiway-9max-pushfold",
    name: "9-max · 10bb push / fold",
    description: "Short-stack nine-handed jam-or-fold research tree.",
    settings: pushFold9Max(),
  },
  {
    id: "multiway-9max-mtt-icm",
    name: "9-max · 20bb MTT + BBA + ICM",
    description: "Unequal stacks, big-blind ante, and a 15-player exact/auto ICM field.",
    settings: mtt9Max(),
  },
  {
    id: "multiway-6max-cash",
    name: "6-max · 100bb cash",
    description: "Six-handed deep cash blueprint with all-street abstraction.",
    settings: cash6Max(),
  },
  {
    id: "multiway-9max-research",
    name: "9-max · 100bb research",
    description: "Large sizing menu, 200-bucket profiles, and a five-million-sweep run.",
    settings: research9Max(),
  },
];

const U32_MAX = 4_294_967_295;
const MAX_ESTIMATE = Number.MAX_SAFE_INTEGER;
const PREFLOP_DIMENSION = 169;
const CHIPS_PER_BB = 10;
const MULTIWAY_CHIPS_PER_BB = 1_000;
const POSTFLOP_MAX_RAISES = 2;
const GG_EXEMPT_POT = 15;
const MAX_MULTIWAY_SWEEPS = 10_000_000;
const MAX_MULTIWAY_EVALUATION_SAMPLES = 1_000_000;
const MAX_MULTIWAY_MEMORY_BYTES = 2 * 1024 * 1024 * 1024;
const MAX_MULTIWAY_STACK_BB = 1_000;
const MAX_MULTIWAY_RANGE_BYTES = 4_096;
const MAX_MULTIWAY_SIZES_PER_LEVEL = 16;
const MAX_MULTIWAY_AGGRESSIVE_ACTIONS = 16;
const MAX_MULTIWAY_BUCKETS = 4_096;

const schedules: readonly ScheduleKind[] = [
  "vanilla",
  "cfr-plus",
  "dcfr",
  "linear-cfr",
  "hs-dcfr",
];
const storageKinds: readonly StorageKind[] = ["f32", "i16"];
const rakeModes: readonly RakeMode[] = ["none", "percent-cap", "gg-preflop"];

function isFiniteNumber(value: number): boolean {
  return Number.isFinite(value);
}

function validateFloat(
  errors: string[],
  label: string,
  value: number,
  minimumExclusive = 0,
): void {
  if (!isFiniteNumber(value) || value <= minimumExclusive) {
    errors.push(`${label} must be greater than ${minimumExclusive}.`);
  }
}

function validateU32(
  errors: string[],
  label: string,
  value: number,
  minimum: number,
): void {
  if (!Number.isSafeInteger(value) || value < minimum || value > U32_MAX) {
    errors.push(`${label} must be an integer from ${minimum} to ${U32_MAX}.`);
  }
}

function validateFloatList(
  errors: string[],
  label: string,
  values: number[],
  minimumExclusive: number,
): void {
  if (!Array.isArray(values)) {
    errors.push(`${label} must be a list.`);
    return;
  }
  values.forEach((value, index) => {
    if (!isFiniteNumber(value) || value <= minimumExclusive) {
      errors.push(`${label} #${index + 1} must be greater than ${minimumExclusive}.`);
    }
  });
}

function validateListLength(errors: string[], label: string, values: number[]): void {
  if (values.length > MAX_MULTIWAY_SIZES_PER_LEVEL) {
    errors.push(`${label} supports at most ${MAX_MULTIWAY_SIZES_PER_LEVEL} sizes.`);
  }
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).length;
}

const RANKS = "23456789TJQKA";
const SUITS = "cdhs";

function rankIndex(value: string): number {
  return RANKS.indexOf(value.toUpperCase());
}

function parseClass(value: string): [number, number, "s" | "o" | null] | null {
  if (value.length !== 2 && value.length !== 3) return null;
  const first = rankIndex(value[0]);
  const second = rankIndex(value[1]);
  if (first < 0 || second < 0) return null;

  let suitedness: "s" | "o" | null = null;
  if (value.length === 3) {
    const suffix = value[2].toLowerCase();
    if (suffix !== "s" && suffix !== "o") return null;
    suitedness = suffix;
  }
  if (first === second && suitedness !== null) return null;
  return [Math.max(first, second), Math.min(first, second), suitedness];
}

function isCard(value: string): boolean {
  return (
    value.length === 2 &&
    rankIndex(value[0]) >= 0 &&
    SUITS.includes(value[1].toLowerCase())
  );
}

function isRangeSpec(value: string): boolean {
  if (value.length === 4 && isCard(value.slice(0, 2)) && isCard(value.slice(2))) {
    return value.slice(0, 2).toLowerCase() !== value.slice(2).toLowerCase();
  }

  const dash = value.indexOf("-");
  if (dash >= 0) {
    if (value.indexOf("-", dash + 1) >= 0) return false;
    const first = parseClass(value.slice(0, dash).trim());
    const second = parseClass(value.slice(dash + 1).trim());
    if (first === null || second === null || first[2] !== second[2]) return false;
    const bothPairs = first[0] === first[1] && second[0] === second[1];
    if (bothPairs) return true;
    return first[0] === second[0] && first[0] !== first[1] && second[0] !== second[1];
  }

  const plain = value.endsWith("+") ? value.slice(0, -1) : value;
  return parseClass(plain) !== null;
}

const DECIMAL = /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?$/i;

/**
 * Mirrors the dependency-free grammar in `cards::Range`: comma-separated
 * classes, class ranges, explicit combos, and optional `:0..1` weights.
 * An empty UI value means the CLI's omitted range (the full range).
 */
function validateRange(errors: string[], label: string, range: string): void {
  if (range.trim() === "") return;

  let entries = 0;
  let hasPositiveWeight = false;
  for (const raw of range.split(",")) {
    const entry = raw.trim();
    if (entry === "") continue;
    entries += 1;

    const colon = entry.indexOf(":");
    let spec = entry;
    let weight = 1;
    if (colon >= 0) {
      if (entry.indexOf(":", colon + 1) >= 0) {
        errors.push(`${label} contains an invalid entry: ${entry}.`);
        continue;
      }
      spec = entry.slice(0, colon).trim();
      const rawWeight = entry.slice(colon + 1).trim();
      if (!DECIMAL.test(rawWeight)) {
        errors.push(`${label} contains an invalid weight: ${entry}.`);
        continue;
      }
      weight = Number(rawWeight);
      if (!Number.isFinite(weight) || weight < 0 || weight > 1) {
        errors.push(`${label} weights must be between 0 and 1: ${entry}.`);
        continue;
      }
    }

    if (!isRangeSpec(spec)) {
      errors.push(`${label} contains an invalid entry: ${entry}.`);
    } else if (weight > 0) {
      hasPositiveWeight = true;
    }
  }

  if (entries === 0 || !hasPositiveWeight) {
    errors.push(`${label} must contain at least one positive-weight hand.`);
  }
}

export function parseNumberPaste(input: string): number[] {
  const matches = input.match(/[-+]?(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?/g) ?? [];
  return matches.map((value) => Number(value.replaceAll(",", "")));
}

export function parseSizingListDraft(input: string): number[] {
  return input
    .split(",")
    .map((value) => value.trim())
    .filter((value) => value !== "")
    .map(Number)
    .filter(Number.isFinite);
}

function validateMultiwaySettings(settings: PreflopSettings): string[] {
  const errors: string[] = [];
  if (!Number.isSafeInteger(settings.tableSize) || settings.tableSize < 3 || settings.tableSize > 9) {
    errors.push("Table size must be an integer from 3 to 9.");
  }

  const expectedPositions = positionsForTableSize(settings.tableSize);
  if (settings.seats.length !== settings.tableSize) {
    errors.push("Seat count must match table size.");
  }
  settings.seats.forEach((seat, index) => {
    if (seat.position !== expectedPositions[index]) {
      errors.push(`Seat #${index + 1} must use the standard position label.`);
    }
    const seatLabel = seat.position || `Seat #${index + 1}`;
    validateFloat(errors, `${seatLabel} stack`, seat.stackBb);
    if (isFiniteNumber(seat.stackBb) && seat.stackBb > MAX_MULTIWAY_STACK_BB) {
      errors.push(`${seatLabel} stack may not exceed ${MAX_MULTIWAY_STACK_BB}bb.`);
    }
    validateRange(errors, `${seatLabel} range`, seat.range);
    if (utf8Length(seat.range) > MAX_MULTIWAY_RANGE_BYTES) {
      errors.push(`${seatLabel} range exceeds the ${MAX_MULTIWAY_RANGE_BYTES}-byte limit.`);
    }
    if (seat.betting) {
      const label = `${seatLabel} override`;
      validateFloatList(errors, `${label} open size`, seat.betting.openSizesBb, 0);
      validateListLength(errors, `${label} open size`, seat.betting.openSizesBb);
      seat.betting.openSizesBb.forEach((size, sizeIndex) => {
        if (isFiniteNumber(size) && size < 2) {
          errors.push(`${label} open size #${sizeIndex + 1} must be at least 2bb.`);
        }
      });
      validateFloatList(errors, `${label} isolate size`, seat.betting.isolateSizesBb, 0);
      validateListLength(errors, `${label} isolate size`, seat.betting.isolateSizesBb);
      seat.betting.isolateSizesBb.forEach((size, sizeIndex) => {
        if (isFiniteNumber(size) && size < 2) {
          errors.push(`${label} isolate size #${sizeIndex + 1} must be at least 2bb.`);
        }
      });
      validateFloatList(errors, `${label} raise factor`, seat.betting.raiseFactors, 1);
      validateListLength(errors, `${label} raise factor`, seat.betting.raiseFactors);
      (["flop", "turn", "river"] as const).forEach((street) => {
        const streetBetting = seat.betting?.postflop[street];
        validateFloatList(
          errors,
          `${label} ${street} bet size`,
          streetBetting?.betSizes ?? [],
          0,
        );
        validateListLength(
          errors,
          `${label} ${street} bet size`,
          streetBetting?.betSizes ?? [],
        );
        validateFloatList(
          errors,
          `${label} ${street} raise size`,
          streetBetting?.raiseSizes ?? [],
          0,
        );
        validateListLength(
          errors,
          `${label} ${street} raise size`,
          streetBetting?.raiseSizes ?? [],
        );
        validateU32(
          errors,
          `${label} ${street} aggressive-action cap`,
          streetBetting?.maxAggressiveActions ?? -1,
          0,
        );
        if ((streetBetting?.maxAggressiveActions ?? 0) > MAX_MULTIWAY_AGGRESSIVE_ACTIONS) {
          errors.push(
            `${label} ${street} aggressive-action cap may not exceed ${MAX_MULTIWAY_AGGRESSIVE_ACTIONS}.`,
          );
        }
      });
    }
  });

  validateFloat(errors, "Small blind", settings.sbBb);
  if (
    isFiniteNumber(settings.sbBb) &&
    (Math.round(settings.sbBb * MULTIWAY_CHIPS_PER_BB) <= 0 ||
      Math.round(settings.sbBb * MULTIWAY_CHIPS_PER_BB) >= MULTIWAY_CHIPS_PER_BB)
  ) {
    errors.push("Small blind must round to between 0.001bb and 0.999bb.");
  }
  if (!["none", "ante", "big-blind-ante"].includes(settings.anteMode)) {
    errors.push("Select a supported ante mode.");
  }
  if (!isFiniteNumber(settings.anteBb) || settings.anteBb < 0) {
    errors.push("Ante must be zero or greater.");
  }
  if (isFiniteNumber(settings.anteBb) && settings.anteBb > MAX_MULTIWAY_STACK_BB) {
    errors.push(`Ante may not exceed ${MAX_MULTIWAY_STACK_BB}bb.`);
  }

  validateFloatList(errors, "Open size", settings.openSizesBb, 0);
  validateListLength(errors, "Open size", settings.openSizesBb);
  settings.openSizesBb.forEach((size, index) => {
    if (isFiniteNumber(size) && size < 2) {
      errors.push(`Open size #${index + 1} must be at least the 2bb minimum raise.`);
    }
  });
  validateFloatList(errors, "Isolate size", settings.isolateSizesBb, 0);
  validateListLength(errors, "Isolate size", settings.isolateSizesBb);
  settings.isolateSizesBb.forEach((size, index) => {
    if (isFiniteNumber(size) && size < 2) {
      errors.push(`Isolate size #${index + 1} must be at least the 2bb minimum raise.`);
    }
  });
  if (settings.raiseFactors.length > MAX_MULTIWAY_SIZES_PER_LEVEL) {
    errors.push(`Raise factors support at most ${MAX_MULTIWAY_SIZES_PER_LEVEL} levels.`);
  }
  settings.raiseFactors.forEach((level, index) => {
    validateFloatList(errors, `Raise level ${index + 1}`, level, 1);
    validateListLength(errors, `Raise level ${index + 1}`, level);
  });
  validateU32(errors, "Maximum raises", settings.maxRaises, 0);
  if (settings.maxRaises > MAX_MULTIWAY_AGGRESSIVE_ACTIONS) {
    errors.push(`Maximum raises may not exceed ${MAX_MULTIWAY_AGGRESSIVE_ACTIONS}.`);
  }
  (["flop", "turn", "river"] as const).forEach((street) => {
    const streetBetting = settings.multiwayPostflopBetting[street];
    validateFloatList(errors, `${street} bet size`, streetBetting.betSizes, 0);
    validateListLength(errors, `${street} bet size`, streetBetting.betSizes);
    validateFloatList(errors, `${street} raise size`, streetBetting.raiseSizes, 0);
    validateListLength(errors, `${street} raise size`, streetBetting.raiseSizes);
    validateU32(
      errors,
      `${street} aggressive-action cap`,
      streetBetting.maxAggressiveActions,
      0,
    );
    if (streetBetting.maxAggressiveActions > MAX_MULTIWAY_AGGRESSIVE_ACTIONS) {
      errors.push(
        `${street} aggressive-action cap may not exceed ${MAX_MULTIWAY_AGGRESSIVE_ACTIONS}.`,
      );
    }
  });

  const expectedProfiles = Math.max(0, settings.tableSize - 1);
  if (
    settings.bucketProfiles.length !== expectedProfiles ||
    settings.bucketProfiles.some((profile, index) => profile.activePlayers !== index + 2)
  ) {
    errors.push("Bucket profiles must cover every active-player count from 2 through table size.");
  }
  settings.bucketProfiles.forEach((profile) => {
    (["preflop", "flop", "turn", "river"] as const).forEach((street) => {
      validateU32(
        errors,
        `${profile.activePlayers}-player ${street} buckets`,
        profile[street],
        1,
      );
    });
    for (const street of ["flop", "turn", "river"] as const) {
      if (profile[street] > MAX_MULTIWAY_BUCKETS) {
        errors.push(`${profile.activePlayers}-player ${street} buckets may not exceed ${MAX_MULTIWAY_BUCKETS}.`);
      }
    }
    if (profile.preflop > 169) {
      errors.push(`${profile.activePlayers}-player preflop buckets cannot exceed 169.`);
    }
  });

  if (
    !Number.isSafeInteger(settings.externalSamplingSweeps) ||
    settings.externalSamplingSweeps <= 0 ||
    settings.externalSamplingSweeps > MAX_MULTIWAY_SWEEPS
  ) {
    errors.push(`External-sampling sweeps must be an integer from 1 to ${MAX_MULTIWAY_SWEEPS}.`);
  }
  validateU32(errors, "External-sampling seed", settings.externalSamplingSeed, 0);
  validateU32(errors, "Checkpoint cadence", settings.checkpointEvery, 0);
  if (
    settings.checkpointEvery > 0 &&
    settings.checkpointEvery > settings.externalSamplingSweeps
  ) {
    errors.push("Checkpoint cadence cannot exceed the sweep count.");
  }
  if (
    !Number.isSafeInteger(settings.evaluationCadence) ||
    settings.evaluationCadence <= 0 ||
    settings.evaluationCadence > settings.externalSamplingSweeps
  ) {
    errors.push("Evaluation cadence must be a positive integer no greater than the sweep count.");
  }
  if (
    !Number.isSafeInteger(settings.evaluationSamples) ||
    settings.evaluationSamples <= 0 ||
    settings.evaluationSamples > MAX_MULTIWAY_EVALUATION_SAMPLES
  ) {
    errors.push(
      `Evaluation samples must be an integer from 1 to ${MAX_MULTIWAY_EVALUATION_SAMPLES}.`,
    );
  }
  if (
    !Number.isSafeInteger(settings.maxMemoryBytes) ||
    settings.maxMemoryBytes <= 0 ||
    settings.maxMemoryBytes > MAX_MULTIWAY_MEMORY_BYTES
  ) {
    errors.push(
      `Memory limit must be an integer from 1 to ${MAX_MULTIWAY_MEMORY_BYTES} bytes.`,
    );
  }
  const resumeCheckpoint = settings.resumeCheckpoint.trim();
  if (/\r|\n|\0/.test(settings.resumeCheckpoint)) {
    errors.push("Resume checkpoint must be a single managed URL.");
  } else if (
    resumeCheckpoint &&
    !/^\/v2\/jobs\/[0-9a-fA-F]{32}\/checkpoint$/.test(resumeCheckpoint)
  ) {
    errors.push("Resume checkpoint must be a managed /v2/jobs/{id}/checkpoint URL.");
  }

  if (!["cash", "icm"].includes(settings.utilityMode)) {
    errors.push("Utility must be cash or ICM.");
  }
  if (settings.utilityMode === "icm") {
    const payouts = parseNumberPaste(settings.payoutsText);
    const outsideStacks = parseNumberPaste(settings.outsideStacksText);
    const totalPlayers = settings.tableSize + outsideStacks.length;
    if (payouts.length === 0) errors.push("ICM payouts must contain at least one amount.");
    payouts.forEach((payout, index) => {
      if (!isFiniteNumber(payout) || payout < 0) {
        errors.push(`Payout #${index + 1} must be zero or greater.`);
      }
      if (index > 0 && payout > payouts[index - 1]) {
        errors.push("ICM payouts must be ordered from highest to lowest.");
      }
    });
    outsideStacks.forEach((stack, index) => {
      if (!isFiniteNumber(stack) || stack <= 0) {
        errors.push(`Outside stack #${index + 1} must be greater than zero.`);
      }
      if (isFiniteNumber(stack) && stack > MAX_MULTIWAY_STACK_BB) {
        errors.push(`Outside stack #${index + 1} may not exceed ${MAX_MULTIWAY_STACK_BB}bb.`);
      }
    });
    if (totalPlayers > 100) errors.push("ICM supports at most 100 remaining players.");
    if (payouts.length > totalPlayers) {
      errors.push("Payout count cannot exceed the remaining-player count.");
    }
    if (payouts.length > 0 && payouts.length <= totalPlayers) {
      const paddedPayouts = [...payouts, ...Array(totalPlayers - payouts.length).fill(0)];
      if (paddedPayouts.every((payout) => payout === paddedPayouts[0])) {
        errors.push(
          "ICM payouts must contain at least two distinct amounts after unpaid places are padded with zero.",
        );
      }
    }
    if (settings.icmMethod === "exact" && totalPlayers > 15) {
      errors.push("Exact ICM supports at most 15 remaining players; use auto or sampled.");
    }
    if (
      !Number.isSafeInteger(settings.icmSamples) ||
      settings.icmSamples < 100 ||
      settings.icmSamples > 1_000_000
    ) {
      errors.push("Sampled ICM runs must be an integer from 100 to 1000000.");
    }
    validateU32(errors, "ICM seed", settings.icmSeed, 0);
    if (settings.rakeMode !== "none") {
      errors.push("Tournament ICM cannot be combined with cash-game rake.");
    }
  }

  if (!storageKinds.includes(settings.storage)) errors.push("Storage must be f32 or i16.");
  if (!rakeModes.includes(settings.rakeMode)) errors.push("Select a supported rake mode.");
  return errors;
}

export function validateSettings(settings: PreflopSettings): string[] {
  if (settings.mode === "multiway") return validateMultiwaySettings(settings);

  const errors: string[] = [];

  validateFloat(errors, "Effective stack", settings.effectiveStackBb, 1);
  if (
    isFiniteNumber(settings.effectiveStackBb) &&
    Math.round(settings.effectiveStackBb * CHIPS_PER_BB) > U32_MAX
  ) {
    errors.push("Effective stack is too large for the solver's 0.1bb chip grid.");
  } else if (
    isFiniteNumber(settings.effectiveStackBb) &&
    Math.round(settings.effectiveStackBb * CHIPS_PER_BB) <= CHIPS_PER_BB
  ) {
    errors.push("Effective stack must round above 1bb on the solver's 0.1bb chip grid.");
  }

  validateFloat(errors, "Small blind", settings.sbBb);
  if (
    isFiniteNumber(settings.sbBb) &&
    (Math.round(settings.sbBb * CHIPS_PER_BB) <= 0 ||
      Math.round(settings.sbBb * CHIPS_PER_BB) >= CHIPS_PER_BB)
  ) {
    errors.push("Small blind must round to between 0.1bb and 0.9bb.");
  }

  validateFloatList(errors, "Open size", settings.openSizesBb, 0);
  settings.openSizesBb.forEach((size, index) => {
    if (isFiniteNumber(size) && size < 2) {
      errors.push(`Open size #${index + 1} must be at least the 2bb minimum raise.`);
    }
  });

  if (!Array.isArray(settings.raiseFactors)) {
    errors.push("Raise factors must be a list of lists.");
  } else {
    settings.raiseFactors.forEach((level, index) => {
      validateFloatList(errors, `Raise level ${index + 1}`, level, 1);
    });
  }

  validateU32(errors, "Maximum raises", settings.maxRaises, 0);
  if (Number.isSafeInteger(settings.maxRaises) && settings.maxRaises > 12) {
    errors.push("Maximum raises cannot exceed 12 in the interactive estimator.");
  }
  const canRaiseAtRoot =
    settings.maxRaises > 0 &&
    (settings.includeAllin || settings.openSizesBb.length > 0);
  if (!settings.allowLimp && !canRaiseAtRoot) {
    errors.push("Enable limping or offer at least one root raise so the SB has a legal choice.");
  }

  validateRange(errors, "SB range", settings.sbRange);
  validateRange(errors, "BB range", settings.bbRange);
  validateFloat(errors, "SB equity realization", settings.equityRealization.sb);
  validateFloat(errors, "BB equity realization", settings.equityRealization.bb);

  if (settings.postflopModel !== "equity" && settings.postflopModel !== "bucketed") {
    errors.push("Postflop model must be equity or bucketed.");
  }
  if (settings.postflopModel === "bucketed") {
    validateU32(errors, "Flop buckets", settings.buckets.flop, 1);
    validateU32(errors, "Turn buckets", settings.buckets.turn, 1);
    validateU32(errors, "River buckets", settings.buckets.river, 1);
    validateFloatList(errors, "Flop bet size", settings.postflopBetSizes.flop, 0);
    validateFloatList(errors, "Turn bet size", settings.postflopBetSizes.turn, 0);
    validateFloatList(errors, "River bet size", settings.postflopBetSizes.river, 0);
  }

  if (!Number.isSafeInteger(settings.iterations) || settings.iterations <= 0) {
    errors.push("Iterations must be a positive safe integer.");
  }
  if (!Number.isSafeInteger(settings.checkEvery) || settings.checkEvery <= 0) {
    errors.push("Check cadence must be a positive safe integer.");
  } else if (Number.isSafeInteger(settings.iterations) && settings.checkEvery > settings.iterations) {
    errors.push("Check cadence cannot exceed the iteration count.");
  }

  if (!storageKinds.includes(settings.storage)) {
    errors.push("Storage must be f32 or i16.");
  }
  if (!schedules.includes(settings.schedule)) {
    errors.push("Select a supported CFR schedule.");
  }
  if (!rakeModes.includes(settings.rakeMode)) {
    errors.push("Select a supported rake mode.");
  }
  if (settings.rakeMode !== "none") {
    if (!isFiniteNumber(settings.rakeRate) || settings.rakeRate < 0 || settings.rakeRate > 1) {
      errors.push("Rake rate must be between 0 and 1.");
    }
    if (!isFiniteNumber(settings.rakeCap) || settings.rakeCap < 0) {
      errors.push("Rake cap must be zero or greater.");
    }
  }

  return errors;
}

function tomlFloat(value: number): string {
  if (!Number.isFinite(value)) {
    throw new RangeError("TOML generation requires finite numeric settings.");
  }
  const normalized = Object.is(value, -0) ? 0 : value;
  return Number.isInteger(normalized) ? `${normalized}.0` : String(normalized);
}

function tomlInteger(value: number): string {
  if (!Number.isSafeInteger(value)) {
    throw new RangeError("TOML generation requires safe integer settings.");
  }
  return String(value);
}

function tomlString(value: string): string {
  return `"${value.replace(/["\\\u0000-\u001f\u007f]/g, (character) => {
    switch (character) {
      case '"':
        return '\\"';
      case "\\":
        return "\\\\";
      case "\b":
        return "\\b";
      case "\t":
        return "\\t";
      case "\n":
        return "\\n";
      case "\f":
        return "\\f";
      case "\r":
        return "\\r";
      default:
        return `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`;
    }
  })}"`;
}

function tomlFloatList(values: number[]): string {
  return `[${values.map(tomlFloat).join(", ")}]`;
}

function tomlNestedFloatList(values: number[][]): string {
  return `[${values.map(tomlFloatList).join(", ")}]`;
}
function tomlObjectList(
  kind: "to-bb" | "pot-after-call" | "previous-bet-multiple",
  key: "value" | "fraction" | "factor",
  values: number[],
): string {
  return `[${values
    .map(
      (value) =>
        `{ kind = ${tomlString(kind)}, ${key} = ${tomlFloat(value)} }`,
    )
    .join(", ")}]`;
}

function appendMultiwayBettingToml(
  lines: string[],
  prefix: string,
  sizes: MultiwaySeatBetting,
  allowLimp: boolean,
  maxRaises: number,
  includeAllin: boolean,
): void {
  const raises = [...new Set(sizes.raiseFactors)];
  lines.push(
    "",
    `[${prefix}]`,
    `allow_limp = ${allowLimp}`,
    "",
    `[${prefix}.preflop]`,
    `bet_sizes = ${tomlObjectList("to-bb", "value", sizes.openSizesBb)}`,
    `isolate_sizes = ${tomlObjectList("to-bb", "value", sizes.isolateSizesBb)}`,
    `raise_sizes = ${tomlObjectList("previous-bet-multiple", "factor", raises)}`,
    `max_aggressive_actions = ${tomlInteger(maxRaises)}`,
    `include_allin = ${includeAllin}`,
  );

  (["flop", "turn", "river"] as const).forEach((street) => {
    const streetBetting = sizes.postflop[street];
    lines.push(
      "",
      `[${prefix}.${street}]`,
      `bet_sizes = ${tomlObjectList("pot-after-call", "fraction", streetBetting.betSizes)}`,
      `raise_sizes = ${tomlObjectList("pot-after-call", "fraction", streetBetting.raiseSizes)}`,
      `max_aggressive_actions = ${tomlInteger(streetBetting.maxAggressiveActions)}`,
      `include_allin = ${streetBetting.includeAllin}`,
    );
  });
}

function generateMultiwayToml(settings: PreflopSettings): string {
  const button = Math.max(
    0,
    settings.seats.findIndex((seat) => seat.position === "BTN"),
  );
  const fullTableProfile =
    settings.bucketProfiles.find(
      (profile) => profile.activePlayers === settings.tableSize,
    ) ??
    settings.bucketProfiles[settings.bucketProfiles.length - 1] ?? {
      activePlayers: settings.tableSize,
      preflop: 169,
      flop: 32,
      turn: 32,
      river: 32,
    };
  const lines: string[] = [
    "# Multiway preflop configuration generated by Solvers",
    "[game]",
    'kind = "preflop-multiway"',
    `button = ${tomlInteger(button)}`,
  ];

  settings.seats.forEach((seat) => {
    lines.push(
      "",
      "[[game.seats]]",
      `name = ${tomlString(seat.position)}`,
      `stack_bb = ${tomlFloat(seat.stackBb)}`,
      `range = ${tomlString(seat.range.trim())}`,
    );
    if (seat.betting) {
      appendMultiwayBettingToml(
        lines,
        "game.seats.betting",
        seat.betting,
        settings.allowLimp,
        settings.maxRaises,
        settings.includeAllin,
      );
    }
  });

  lines.push(
    "",
    "[game.blinds]",
    `small_bb = ${tomlFloat(settings.sbBb)}`,
    "big_bb = 1.0",
    "",
    "[game.ante]",
  );
  if (settings.anteMode === "none") {
    lines.push('kind = "none"');
  } else {
    lines.push(
      `kind = ${tomlString(
        settings.anteMode === "ante" ? "each" : "big-blind",
      )}`,
      `amount_bb = ${tomlFloat(settings.anteBb)}`,
    );
  }

  appendMultiwayBettingToml(
    lines,
    "game.betting",
    {
      openSizesBb: settings.openSizesBb,
      isolateSizesBb: settings.isolateSizesBb,
      raiseFactors: settings.raiseFactors.flat(),
      postflop: settings.multiwayPostflopBetting,
    },
    settings.allowLimp,
    settings.maxRaises,
    settings.includeAllin,
  );

  lines.push(
    "",
    "# Full-table defaults, followed by budgets for each live-opponent count.",
    "[game.abstraction]",
    `flop_buckets = ${tomlInteger(fullTableProfile.flop)}`,
    `turn_buckets = ${tomlInteger(fullTableProfile.turn)}`,
    `river_buckets = ${tomlInteger(fullTableProfile.river)}`,
    "rollout_samples = 256",
    `seed = ${tomlInteger(settings.externalSamplingSeed)}`,
  );
  settings.bucketProfiles.forEach((profile) => {
    lines.push(
      "",
      "[[game.abstraction.active_opponent_buckets]]",
      `active_opponents = ${tomlInteger(profile.activePlayers - 1)}`,
      `flop_buckets = ${tomlInteger(profile.flop)}`,
      `turn_buckets = ${tomlInteger(profile.turn)}`,
      `river_buckets = ${tomlInteger(profile.river)}`,
    );
  });

  lines.push(
    "",
    "[rake]",
    `kind = ${tomlString(settings.rakeMode)}`,
  );
  if (settings.rakeMode !== "none") {
    lines.push(
      `rate = ${tomlFloat(settings.rakeRate)}`,
      `cap = ${tomlFloat(settings.rakeCap * MULTIWAY_CHIPS_PER_BB)}`,
    );
    if (settings.rakeMode === "percent-cap") {
      lines.push(`no_flop_no_drop = ${settings.noFlopNoDrop}`);
    } else {
      lines.push(`exempt_pot = ${tomlInteger(GG_EXEMPT_POT * MULTIWAY_CHIPS_PER_BB)}`);
    }
  }

  lines.push("", "[utility]");
  if (settings.utilityMode === "cash") {
    lines.push('kind = "chip-ev"');
  } else {
    const outsideStacks = parseNumberPaste(settings.outsideStacksText);
    const payouts = parseNumberPaste(settings.payoutsText);
    const fieldSize = settings.tableSize + outsideStacks.length;
    while (payouts.length < fieldSize) payouts.push(0);
    lines.push(
      'kind = "tournament-icm"',
      `payouts = ${tomlFloatList(payouts)}`,
      `samples = ${tomlInteger(settings.icmSamples)}`,
      `seed = ${tomlInteger(settings.icmSeed)}`,
    );
    outsideStacks.forEach((stack, index) => {
      lines.push(
        "",
        "[[utility.outside_field]]",
        `name = ${tomlString(`Field ${index + 1}`)}`,
        `stack_bb = ${tomlFloat(stack)}`,
      );
    });
  }

  lines.push(
    "",
    "[algorithm]",
    'schedule = "external-sampling-mccfr"',
    `seed = ${tomlInteger(settings.externalSamplingSeed)}`,
    "exploration_epsilon = 0.06",
    "discount_every = 100000",
    "discount_until = 10000000",
    "",
    "[run]",
    `sweeps = ${tomlInteger(settings.externalSamplingSweeps)}`,
    `seed = ${tomlInteger(settings.externalSamplingSeed)}`,
    `check_every = ${tomlInteger(settings.evaluationCadence)}`,
    'storage = "f32"',
    `max_memory_bytes = ${tomlInteger(settings.maxMemoryBytes)}`,
    `evaluation_samples = ${tomlInteger(settings.evaluationSamples)}`,
    `evaluation_cadence = ${tomlInteger(settings.evaluationCadence)}`,
  );
  if (settings.checkpointEvery > 0) {
    lines.push(
      `checkpoint_every = ${tomlInteger(settings.checkpointEvery)}`,
    );
  }

  return `${lines.join("\n")}\n`;
}



/** Generate a `SolveConfig` accepted by `app/cli/src/config.rs`. */
export function generateToml(settings: PreflopSettings): string {
  if (settings.mode === "multiway") return generateMultiwayToml(settings);

  const lines = [
    "# Heads-up preflop configuration generated by Solvers",
    "[game]",
    'kind = "preflop"',
    `effective_stack_bb = ${tomlFloat(settings.effectiveStackBb)}`,
    `sb_bb = ${tomlFloat(settings.sbBb)}`,
    `open_sizes_bb = ${tomlFloatList(settings.openSizesBb)}`,
    `raise_factors = ${tomlNestedFloatList(settings.raiseFactors)}`,
    `max_raises = ${tomlInteger(settings.maxRaises)}`,
    `include_allin = ${settings.includeAllin}`,
    `allow_limp = ${settings.allowLimp}`,
    `equity_realization = [${tomlFloat(settings.equityRealization.sb)}, ${tomlFloat(settings.equityRealization.bb)}]`,
  ];

  const sbRange = settings.sbRange.trim();
  const bbRange = settings.bbRange.trim();
  if (sbRange !== "") lines.push(`sb_range = ${tomlString(sbRange)}`);
  if (bbRange !== "") lines.push(`bb_range = ${tomlString(bbRange)}`);
  if (settings.cacheEnabled) {
    lines.push('equity_cache = ".cache/preflop_equity.bin"');
  }

  if (settings.postflopModel === "bucketed") {
    const { flop, turn, river } = settings.buckets;
    lines.push(
      "",
      "[game.postflop]",
      'model = "bucketed"',
      `flop-buckets = ${tomlInteger(flop)}`,
      `turn-buckets = ${tomlInteger(turn)}`,
      `river-buckets = ${tomlInteger(river)}`,
      `bets-flop = ${tomlFloatList(settings.postflopBetSizes.flop)}`,
      `bets-turn = ${tomlFloatList(settings.postflopBetSizes.turn)}`,
      `bets-river = ${tomlFloatList(settings.postflopBetSizes.river)}`,
      `max-raises = ${POSTFLOP_MAX_RAISES}`,
      "include-allin = true",
    );
    if (settings.cacheEnabled) {
      const suffix = `${flop}_${turn}_${river}`;
      lines.push(
        `abstraction-cache = ".cache/ehs2_${suffix}.bin"`,
        `artifacts-cache = ".cache/blueprint_${suffix}.bin"`,
      );
    }
  }

  if (settings.rakeMode !== "none") {
    lines.push(
      "",
      "[rake]",
      `kind = ${tomlString(settings.rakeMode)}`,
      `rate = ${tomlFloat(settings.rakeRate)}`,
      `cap = ${tomlFloat(settings.rakeCap * CHIPS_PER_BB)}`,
    );
    if (settings.rakeMode === "percent-cap") {
      lines.push(`no_flop_no_drop = ${settings.noFlopNoDrop}`);
    } else {
      // The preflop tree uses 10 chips/bb, so 15 is the unopened blind pot.
      lines.push(`exempt_pot = ${GG_EXEMPT_POT}`);
    }
  }

  lines.push(
    "",
    "[algorithm]",
    `schedule = ${tomlString(settings.schedule)}`,
    "",
    "[run]",
    `iterations = ${tomlInteger(settings.iterations)}`,
    `check_every = ${tomlInteger(settings.checkEvery)}`,
    `storage = ${tomlString(settings.storage)}`,
  );

  return `${lines.join("\n")}\n`;
}

interface Count {
  nodes: number;
  terminals: number;
  actionNodes: number;
  elements: number;
}

interface PreflopState {
  actor: 0 | 1;
  contrib: [number, number];
  raisesUsed: number;
  lastRaiseTo: number;
  previousRaiseTo: number;
}

type Street = "flop" | "turn" | "river";

interface PostflopState {
  street: Street;
  actor: 0 | 1;
  totalBeforeStreet: [number, number];
  streetContrib: [number, number];
  raisesUsed: number;
  checked: boolean;
}


function saturatedAdd(left: number, right: number): number {
  return Math.min(MAX_ESTIMATE, left + right);
}

function saturatedMultiply(left: number, right: number): number {
  if (left === 0 || right === 0) return 0;
  return Math.min(MAX_ESTIMATE, left * right);
}

function addCount(target: Count, source: Count): void {
  target.nodes = saturatedAdd(target.nodes, source.nodes);
  target.terminals = saturatedAdd(target.terminals, source.terminals);
  target.actionNodes = saturatedAdd(target.actionNodes, source.actionNodes);
  target.elements = saturatedAdd(target.elements, source.elements);
}

function terminalCount(): Count {
  return { nodes: 1, terminals: 1, actionNodes: 0, elements: 0 };
}

function uniqueSorted(values: number[]): number[] {
  return [...new Set(values)].sort((left, right) => left - right);
}

function structuralEstimate(settings: PreflopSettings): Count {
  const stack = Math.max(CHIPS_PER_BB + 1, Math.round(settings.effectiveStackBb * CHIPS_PER_BB));
  const sb = Math.max(1, Math.min(CHIPS_PER_BB - 1, Math.round(settings.sbBb * CHIPS_PER_BB)));
  const preflopMemo = new Map<string, Count>();
  const postflopMemo = new Map<string, Count>();

  const preflopRaiseTargets = (state: PreflopState): number[] => {
    const opponent = state.actor === 0 ? 1 : 0;
    if (state.raisesUsed >= settings.maxRaises || state.contrib[opponent] >= stack) return [];

    const minimum = state.lastRaiseTo + (state.lastRaiseTo - state.previousRaiseTo);
    const raw =
      state.raisesUsed === 0
        ? settings.openSizesBb.map((size) => Math.round(size * CHIPS_PER_BB))
        : settings.raiseFactors.length === 0
          ? []
          : settings.raiseFactors[
              Math.min(state.raisesUsed - 1, settings.raiseFactors.length - 1)
            ].map((factor) => Math.round(factor * state.lastRaiseTo));
    const targets = raw.map((target) => Math.max(minimum, Math.min(stack, target)));
    if (settings.includeAllin) targets.push(stack);
    return uniqueSorted(targets);
  };

  const postflopSizes = (street: Street): number[] => settings.postflopBetSizes[street];

  const postflopRaiseTargets = (state: PostflopState): number[] => {
    if (state.raisesUsed >= POSTFLOP_MAX_RAISES) return [];
    const opponent = state.actor === 0 ? 1 : 0;
    const outstanding = state.streetContrib[opponent] - state.streetContrib[state.actor];
    const totalActor = state.totalBeforeStreet[state.actor] + state.streetContrib[state.actor];
    const stackBehind = stack - totalActor;
    if (stackBehind <= outstanding) return [];

    const potNow =
      state.totalBeforeStreet[0] +
      state.totalBeforeStreet[1] +
      state.streetContrib[0] +
      state.streetContrib[1];
    const targets = postflopSizes(state.street).map((fraction) => {
      const rawExtra = Math.round(fraction * (potNow + outstanding));
      const extra = Math.min(Math.max(rawExtra, 1), stackBehind - outstanding);
      return state.streetContrib[state.actor] + outstanding + extra;
    });
    // `[game.postflop]` uses its contract defaults: all-in is always offered.
    targets.push(state.streetContrib[state.actor] + stackBehind);
    return uniqueSorted(targets);
  };

  const countPostflop = (state: PostflopState): Count => {
    const key = `${state.street}|${state.actor}|${state.totalBeforeStreet.join(",")}|${state.streetContrib.join(",")}|${state.raisesUsed}|${state.checked}`;
    const cached = postflopMemo.get(key);
    if (cached !== undefined) return cached;

    const result: Count = { nodes: 1, terminals: 0, actionNodes: 1, elements: 0 };
    const opponent = state.actor === 0 ? 1 : 0;
    const outstanding = state.streetContrib[opponent] - state.streetContrib[state.actor];
    let actions = 0;

    const streetEnd = (ended: PostflopState): Count => {
      const totals: [number, number] = [
        ended.totalBeforeStreet[0] + ended.streetContrib[0],
        ended.totalBeforeStreet[1] + ended.streetContrib[1],
      ];
      if (ended.street === "river" || totals[0] === stack || totals[1] === stack) {
        return terminalCount();
      }
      const nextStreet: Street = ended.street === "flop" ? "turn" : "river";
      const next = countPostflop({
        street: nextStreet,
        actor: 1,
        totalBeforeStreet: totals,
        streetContrib: [0, 0],
        raisesUsed: 0,
        checked: false,
      });
      const withChance = { ...next, nodes: saturatedAdd(next.nodes, 1) };
      return withChance;
    };

    if (outstanding === 0) {
      actions += 1;
      if (state.checked) {
        addCount(result, streetEnd(state));
      } else {
        addCount(
          result,
          countPostflop({ ...state, actor: opponent as 0 | 1, checked: true }),
        );
      }
    } else {
      actions += 2;
      addCount(result, terminalCount());
      const called: PostflopState = {
        ...state,
        streetContrib: [...state.streetContrib] as [number, number],
      };
      called.streetContrib[state.actor] = state.streetContrib[opponent];
      addCount(result, streetEnd(called));
    }

    const raises = postflopRaiseTargets(state);
    actions += raises.length;
    for (const target of raises) {
      const nextContrib = [...state.streetContrib] as [number, number];
      nextContrib[state.actor] = target;
      addCount(
        result,
        countPostflop({
          ...state,
          actor: opponent as 0 | 1,
          streetContrib: nextContrib,
          raisesUsed: state.raisesUsed + 1,
        }),
      );
    }

    result.elements = saturatedAdd(
      result.elements,
      saturatedMultiply(actions, settings.buckets[state.street]),
    );
    postflopMemo.set(key, result);
    return result;
  };

  const continuation = (contrib: [number, number]): Count => {
    if (settings.postflopModel === "equity" || contrib[0] === stack || contrib[1] === stack) {
      return terminalCount();
    }
    const postflop = countPostflop({
      street: "flop",
      actor: 1,
      totalBeforeStreet: contrib,
      streetContrib: [0, 0],
      raisesUsed: 0,
      checked: false,
    });
    return { ...postflop, nodes: saturatedAdd(postflop.nodes, 1) };
  };

  const countPreflop = (state: PreflopState): Count => {
    const key = `${state.actor}|${state.contrib.join(",")}|${state.raisesUsed}|${state.lastRaiseTo}|${state.previousRaiseTo}`;
    const cached = preflopMemo.get(key);
    if (cached !== undefined) return cached;

    const result: Count = { nodes: 1, terminals: 0, actionNodes: 1, elements: 0 };
    const opponent = state.actor === 0 ? 1 : 0;
    const outstanding = state.contrib[opponent] - state.contrib[state.actor];
    let actions = 0;

    if (outstanding === 0) {
      actions += 1;
      addCount(result, continuation(state.contrib));
    } else if (state.actor === 0 && state.raisesUsed === 0 && state.contrib[opponent] === CHIPS_PER_BB) {
      actions += 1;
      addCount(result, terminalCount());
      if (settings.allowLimp) {
        actions += 1;
        const limped: [number, number] = [...state.contrib];
        limped[state.actor] = CHIPS_PER_BB;
        addCount(result, countPreflop({ ...state, actor: 1, contrib: limped }));
      }
    } else {
      actions += 2;
      addCount(result, terminalCount());
      const called: [number, number] = [...state.contrib];
      called[state.actor] = state.contrib[opponent];
      addCount(result, continuation(called));
    }

    const raises = preflopRaiseTargets(state);
    actions += raises.length;
    for (const target of raises) {
      const nextContrib = [...state.contrib] as [number, number];
      nextContrib[state.actor] = target;
      addCount(
        result,
        countPreflop({
          actor: opponent as 0 | 1,
          contrib: nextContrib,
          raisesUsed: state.raisesUsed + 1,
          lastRaiseTo: target,
          previousRaiseTo: state.lastRaiseTo,
        }),
      );
    }

    result.elements = saturatedAdd(
      result.elements,
      saturatedMultiply(actions, PREFLOP_DIMENSION),
    );
    preflopMemo.set(key, result);
    return result;
  };

  return countPreflop({
    actor: 0,
    contrib: [sb, CHIPS_PER_BB],
    raisesUsed: 0,
    lastRaiseTo: CHIPS_PER_BB,
    previousRaiseTo: 0,
  });
}

function formatCount(value: number): string {
  if (value < 1_000) return `~${Math.round(value)}`;
  const units: Array<[number, string]> = [
    [1e12, "T"],
    [1e9, "B"],
    [1e6, "M"],
    [1e3, "K"],
  ];
  const [divisor, suffix] = units.find(([divisor]) => value >= divisor) ?? [1, ""];
  const scaled = value / divisor;
  return `~${scaled >= 100 ? scaled.toFixed(0) : scaled >= 10 ? scaled.toFixed(1) : scaled.toFixed(2)}${suffix}`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${Math.ceil(bytes)} B`;
  const units: Array<[number, string]> = [
    [1024 ** 4, "TiB"],
    [1024 ** 3, "GiB"],
    [1024 ** 2, "MiB"],
    [1024, "KiB"],
  ];
  const [divisor, suffix] = units.find(([divisor]) => bytes >= divisor) ?? [1, "B"];
  const scaled = bytes / divisor;
  return `~${scaled >= 100 ? scaled.toFixed(0) : scaled >= 10 ? scaled.toFixed(1) : scaled.toFixed(2)} ${suffix}`;
}

function formatDuration(seconds: number): string {
  if (seconds < 1) return "< 1 sec";
  if (seconds < 60) return `~${Math.max(1, Math.round(seconds))} sec`;
  if (seconds < 3_600) return `~${Math.max(1, Math.round(seconds / 60))} min`;
  const hours = seconds / 3_600;
  return `~${hours >= 10 ? hours.toFixed(0) : hours.toFixed(1)} hr`;
}
function estimateMultiwaySolve(settings: PreflopSettings): {
  memory: string;
  time: string;
  nodes: string;
  quality: string;
} {
  const postflopSizingBranches = (["flop", "turn", "river"] as const).reduce(
    (total, street) =>
      total +
      settings.multiwayPostflopBetting[street].betSizes.length +
      settings.multiwayPostflopBetting[street].raiseSizes.length,
    0,
  );
  const sizingBranches =
    2 +
    settings.openSizesBb.length +
    settings.isolateSizesBb.length +
    settings.raiseFactors.flat().length +
    postflopSizingBranches;
  const bucketMass = settings.bucketProfiles.reduce(
    (total, profile) =>
      saturatedAdd(
        total,
        saturatedMultiply(
          profile.activePlayers,
          profile.preflop + profile.flop + profile.turn + profile.river,
        ),
      ),
    0,
  );
  const infosets = saturatedMultiply(
    Math.max(1, bucketMass),
    saturatedMultiply(settings.tableSize, sizingBranches),
  );
  const bytesPerInfoset = settings.storage === "i16" ? 12 : 24;
  const memoryBytes = saturatedMultiply(infosets, bytesPerInfoset);
  const traversals = saturatedMultiply(
    settings.externalSamplingSweeps,
    settings.tableSize,
  );
  const seconds =
    saturatedMultiply(traversals, Math.max(1, sizingBranches)) / 1_500_000;
  const fidelity =
    Math.sqrt(settings.externalSamplingSweeps / 250_000) *
    Math.cbrt(Math.max(1, bucketMass) / Math.max(1, settings.tableSize * 256));
  const utility =
    settings.utilityMode === "icm"
      ? parseNumberPaste(settings.outsideStacksText).length > 6
        ? " · sampled ICM"
        : " · exact/auto ICM"
      : " · chip EV";

  return {
    memory: formatBytes(memoryBytes),
    time: formatDuration(seconds),
    nodes: formatCount(infosets),
    quality: `${
      fidelity >= 6
        ? "Research-grade external sampling"
        : fidelity >= 2
          ? "Balanced external-sampling blueprint"
          : "Draft external-sampling blueprint"
    }${utility}`,
  };
}



function estimateQuality(settings: PreflopSettings): string {
  if (settings.postflopModel === "equity") return "Fast equity approximation";

  const resolution = Math.cbrt(
    (settings.buckets.flop / 50) *
      (settings.buckets.turn / 20) *
      (settings.buckets.river / 8),
  );
  const scheduleFactor = settings.schedule === "dcfr" ? 1 : settings.schedule === "cfr-plus" ? 0.9 : 0.8;
  const score = resolution * Math.sqrt(settings.iterations / 2_000) * scheduleFactor;
  if (score >= 3) return "High-fidelity blueprint";
  if (score >= 0.8) return "Balanced blueprint";
  return "Draft blueprint";
}

/**
 * A deterministic preflight estimate. Tree/storage counting mirrors the Rust
 * dry run; time is a deliberately conservative single-workstation estimate.
 */
export function estimateSolve(settings: PreflopSettings): {
  memory: string;
  time: string;
  nodes: string;
  quality: string;
} {
  if (settings.mode === "multiway") return estimateMultiwaySolve(settings);

  const count = structuralEstimate(settings);
  const storageBytes =
    settings.storage === "f32"
      ? saturatedMultiply(count.elements, 8)
      : saturatedAdd(saturatedMultiply(count.elements, 4), saturatedMultiply(count.actionNodes, 8));
  const treeBytes = saturatedMultiply(count.nodes, 32);
  const equityTableBytes = PREFLOP_DIMENSION * PREFLOP_DIMENSION * 12;
  const artifactBytes =
    settings.postflopModel === "bucketed"
      ? ((PREFLOP_DIMENSION * settings.buckets.flop +
          settings.buckets.flop * settings.buckets.turn +
          settings.buckets.turn * settings.buckets.river) *
          8 +
          settings.buckets.river ** 2 * 16)
      : 0;
  const memoryBytes = saturatedAdd(
    saturatedAdd(storageBytes, treeBytes),
    equityTableBytes + artifactBytes,
  );

  const passes = saturatedAdd(settings.iterations, Math.ceil(settings.iterations / settings.checkEvery));
  const storagePenalty = settings.storage === "i16" ? 1.2 : 1;
  const traversalSeconds =
    (saturatedMultiply(count.elements, passes) / 75_000_000) * storagePenalty;
  const cacheBuildSeconds = settings.cacheEnabled
    ? 0
    : settings.postflopModel === "bucketed"
      ? 10 * 60
      : 60;

  return {
    memory: formatBytes(memoryBytes),
    time: formatDuration(traversalSeconds + cacheBuildSeconds),
    nodes: formatCount(count.nodes),
    quality: estimateQuality(settings),
  };
}
