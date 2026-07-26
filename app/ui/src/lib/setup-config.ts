export type SeatDraft = {
  stackBb: string
  blindBb: string
  anteBb: string
  range: string
}

export type TreeRuleStreet = "preflop" | "flop" | "turn" | "river" | "postflop"

export type TreeRuleEffect =
  "add" | "remove" | "replace" | "force" | "checkdown"

export type TreeRuleAction = "fold" | "check" | "call" | "bet" | "raise"

export type TreeRuleDraft = {
  /** Editor-only identity. It is deliberately omitted from generated TOML. */
  id: string
  /** Keep numeric input lossless until the Rust normalizer accepts it. */
  priority: string
  street: TreeRuleStreet
  condition: string
  effect: TreeRuleEffect
  action: TreeRuleAction | null
  /** One field per token because geometric sizes contain a comma. */
  sizes: string[]
}

export type EconomicsKind = "cash" | "tournament-icm"

export type EconomicsDraft = {
  kind: EconomicsKind
  cash: {
    rakeEnabled: boolean
    rakeRate: string
    rakeCapEnabled: boolean
    rakeCapBb: string
    rakeWhen: string
    rakeAllocation: "main-first" | "proportional"
    rakeRounding: "down" | "nearest" | "up"
  }
  tournamentIcm: {
    /** Comma, whitespace, or newline separated; tokens remain lossless. */
    payouts: string
    /** One outside-player stack per token, in BB. */
    outsideFieldBb: string
    samples: string
    seed: string
  }
}

export type TreeScriptParamDraft = {
  id: string
  key: string
  type: "string" | "integer" | "float" | "boolean"
  value: string
}

export type FormSolveDraft = {
  name: string
  seatCount: number
  button: number
  standardBlinds: boolean
  preflopFirstToAct: "utg" | string
  commonAnteBb: string
  defaultStackBb: string
  defaultRange: string
  seats: SeatDraft[]
  treeKind: "standard" | "script"
  treeScriptSource: string
  treeScriptSourceId: string | null
  treeScriptParams: TreeScriptParamDraft[]
  treeAllowLimp: "default" | "true" | "false"
  treeAggressionCapsEnabled: boolean
  treeAggressionCapPreflop: string
  treeAggressionCapFlop: string
  treeAggressionCapTurn: string
  treeAggressionCapRiver: string
  treeReraiseJamEnabled: boolean
  treeReraiseJamNumerator: string
  treeReraiseJamDenominator: string
  treeRules: TreeRuleDraft[]
  economics: EconomicsDraft
  abstractionKind: "ehs2-percentile"
  flopBuckets: string
  turnBuckets: string
  riverBuckets: string
  recall: "current-street"
  solverKind: "range-vector" | "single-hand"
  solverSeed: string
  opponentExploration: string
  batchSweeps: string
  discountKind: "periodic" | "none"
  discountEverySweeps: string
  discountUntilSweeps: string
  pruningKind: "regret-based" | "none"
  maxSweeps: string
  maxTime: string
  stopTarget: string
  stopCheckEverySweeps: string
  stopConfirmations: string
  evaluationSamples: string
  deviatorTraversals: string
  threads: "auto" | string
  memory: "auto" | string
  checkpointInterval: string
  probabilityEncoding: "u16" | "f32"
}

const DECIMAL = /^(?:0|[1-9]\d*)(?:\.\d+)?$/
const INTEGER = /^(?:0|[1-9]\d*)$/
const SIGNED_INTEGER = /^-?(?:0|[1-9]\d*)$/
const SIGNED_DECIMAL = /^-?(?:0|[1-9]\d*)(?:\.\d+)?$/
const DURATION = /^(?:[1-9]\d*)[smh]$/
const MEMORY_WITH_UNIT = /^(?:[1-9]\d*)(?:KiB|MiB|GiB)$/
const POSITIVE_INTEGER = /^[1-9]\d*$/
const PRODUCTION_POLICY_ARENA_LIMIT_BYTES = 6n * 1024n * 1024n * 1024n

export const MAX_TREE_RULES = 256
export const MAX_TREE_RULE_SIZES = 32

let nextTreeRuleId = 1
let nextTreeParamId = 1

export class DraftTokenError extends Error {
  readonly path: string

  constructor(path: string, message: string) {
    super(message)
    this.path = path
    this.name = "DraftTokenError"
  }
}

function decimal(path: string, raw: string) {
  const value = raw.trim()
  if (!DECIMAL.test(value)) {
    throw new DraftTokenError(path, `${path}は有限10進数で入力してください。`)
  }
  return value
}

function positiveDecimal(path: string, raw: string) {
  const value = decimal(path, raw)
  if (/^0(?:\.0+)?$/.test(value)) {
    throw new DraftTokenError(
      path,
      `${path}は正の有限10進数で入力してください。`
    )
  }
  return value
}

function integer(path: string, raw: string) {
  const value = raw.trim()
  if (!INTEGER.test(value)) {
    throw new DraftTokenError(path, `${path}は整数で入力してください。`)
  }
  return value
}

function integerAtLeast(path: string, raw: string, minimum: bigint) {
  const value = integer(path, raw)
  if (BigInt(value) < minimum) {
    throw new DraftTokenError(
      path,
      `${path}は${minimum.toString()}以上で入力してください。`
    )
  }
  return value
}

function signedInteger(path: string, raw: string) {
  const value = raw.trim()
  if (!SIGNED_INTEGER.test(value)) {
    throw new DraftTokenError(path, `${path}は符号付き整数で入力してください。`)
  }
  return value
}

function tomlString(value: string) {
  return JSON.stringify(value)
}

export function splitNumericTokenList(raw: string) {
  return raw
    .split(/[\s,]+/)
    .map((value) => value.trim())
    .filter(Boolean)
}

export type DecimalTokenListSummary = {
  tokens: string[]
  valid: boolean
  paidPlaces: number | null
  total: string | null
}

/**
 * Builds a display-only summary without converting decimal input through a
 * JavaScript number. Rust remains the authoritative validator.
 */
export function summarizeDecimalTokenList(
  raw: string
): DecimalTokenListSummary {
  const tokens = splitNumericTokenList(raw)
  if (!tokens.every((token) => DECIMAL.test(token))) {
    return { tokens, valid: false, paidPlaces: null, total: null }
  }

  const scale = tokens.reduce((maximum, token) => {
    const fraction = token.split(".")[1]
    return Math.max(maximum, fraction?.length ?? 0)
  }, 0)
  const total = tokens.reduce((sum, token) => {
    const [whole, fraction = ""] = token.split(".")
    return sum + BigInt(`${whole}${fraction.padEnd(scale, "0")}`)
  }, 0n)
  const divisor = 10n ** BigInt(scale)
  const whole = scale === 0 ? total : total / divisor
  const fraction =
    scale === 0
      ? ""
      : (total % divisor).toString().padStart(scale, "0").replace(/0+$/, "")
  const lastPositiveIndex = tokens.findLastIndex(
    (token) => !/^0(?:\.0+)?$/.test(token)
  )

  return {
    tokens,
    valid: true,
    paidPlaces: lastPositiveIndex + 1,
    total: fraction ? `${whole}.${fraction}` : whole.toString(),
  }
}

function decimalList(path: string, raw: string) {
  return splitNumericTokenList(raw).map((value, index) =>
    decimal(`${path}[${index}]`, value)
  )
}

export function createTreeRuleDraft(): TreeRuleDraft {
  return {
    id: `tree-rule-${nextTreeRuleId++}`,
    priority: "100",
    street: "preflop",
    condition: "unopened",
    effect: "replace",
    action: "raise",
    sizes: ["2.5x"],
  }
}

export function copyTreeRuleDraft(rule: TreeRuleDraft): TreeRuleDraft {
  return {
    ...rule,
    id: `tree-rule-${nextTreeRuleId++}`,
    sizes: [...rule.sizes],
  }
}

export function createTreeScriptParamDraft(): TreeScriptParamDraft {
  return {
    id: `tree-param-${nextTreeParamId++}`,
    key: "open",
    type: "string",
    value: "2.5x",
  }
}

export type PreflopTreeBuilderDraft = {
  unopenedSizes: string[]
  limpedSizes: string[]
  reraiseSizes: string[]
  maxAggressions: string
  postflop: "full-tree" | "checkdown"
}

export type PostflopStreetBuilderDraft = {
  betSizes: string[]
  raiseSizes: string[]
}

export type PostflopTreeBuilderDraft = {
  flop: PostflopStreetBuilderDraft
  turn: PostflopStreetBuilderDraft
  river: PostflopStreetBuilderDraft
  maxAggressions: string
}

export function createDefaultPreflopTreeBuilder(): PreflopTreeBuilderDraft {
  return {
    unopenedSizes: ["2.5x", "allin"],
    limpedSizes: ["2.5x", "allin"],
    reraiseSizes: ["3x", "allin"],
    maxAggressions: "4",
    postflop: "checkdown",
  }
}

export function createPreflopTreeRules(
  builder: PreflopTreeBuilderDraft
): TreeRuleDraft[] {
  const maxAggressions = integerAtLeast(
    "game.tree.preflop.max_aggressions",
    builder.maxAggressions,
    1n
  )
  const rules: TreeRuleDraft[] = [
    {
      id: `tree-rule-${nextTreeRuleId++}`,
      priority: "100",
      street: "preflop",
      condition: "unopened",
      effect: "replace",
      action: "raise",
      sizes: [...builder.unopenedSizes],
    },
    {
      id: `tree-rule-${nextTreeRuleId++}`,
      priority: "110",
      street: "preflop",
      condition: "limpers > 0 && aggressions == 0",
      effect: "replace",
      action: "raise",
      sizes: [...builder.limpedSizes],
    },
    {
      id: `tree-rule-${nextTreeRuleId++}`,
      priority: "120",
      street: "preflop",
      condition: "aggressions >= 1",
      effect: "replace",
      action: "raise",
      sizes: [...builder.reraiseSizes],
    },
    {
      id: `tree-rule-${nextTreeRuleId++}`,
      priority: "130",
      street: "preflop",
      condition: `aggressions >= ${maxAggressions}`,
      effect: "remove",
      action: "raise",
      sizes: [],
    },
  ]

  if (builder.postflop === "checkdown") {
    rules.push({
      id: `tree-rule-${nextTreeRuleId++}`,
      priority: "200",
      street: "postflop",
      condition: "players >= 2",
      effect: "checkdown",
      action: null,
      sizes: [],
    })
  }
  return rules
}

export function createDefaultPostflopTreeBuilder(): PostflopTreeBuilderDraft {
  const street = (): PostflopStreetBuilderDraft => ({
    betSizes: ["50%pot", "allin"],
    raiseSizes: ["75%pot", "allin"],
  })
  return {
    flop: street(),
    turn: street(),
    river: street(),
    maxAggressions: "3",
  }
}

export function createPostflopTreeRules(
  builder: PostflopTreeBuilderDraft
): TreeRuleDraft[] {
  const maxAggressions = integerAtLeast(
    "game.tree.postflop.max_aggressions",
    builder.maxAggressions,
    1n
  )
  const streets = ["flop", "turn", "river"] as const
  return streets.flatMap((street, index): TreeRuleDraft[] => {
    const priority = 300 + index * 10
    return [
      {
        id: `tree-rule-${nextTreeRuleId++}`,
        priority: String(priority),
        street,
        condition: "players >= 2",
        effect: "replace",
        action: "bet",
        sizes: [...builder[street].betSizes],
      },
      {
        id: `tree-rule-${nextTreeRuleId++}`,
        priority: String(priority + 1),
        street,
        condition: "players >= 2",
        effect: "replace",
        action: "raise",
        sizes: [...builder[street].raiseSizes],
      },
      {
        id: `tree-rule-${nextTreeRuleId++}`,
        priority: String(priority + 2),
        street,
        condition: `aggressions >= ${maxAggressions}`,
        effect: "remove",
        action: "raise",
        sizes: [],
      },
    ]
  })
}

function createDefaultTreeRules(): TreeRuleDraft[] {
  return createPreflopTreeRules(createDefaultPreflopTreeBuilder())
}

function presetTreeRule(
  priority: number,
  street: TreeRuleStreet,
  condition: string,
  effect: TreeRuleEffect,
  action: TreeRuleAction | null,
  sizes: string[] = []
): TreeRuleDraft {
  return {
    id: `tree-rule-${nextTreeRuleId++}`,
    priority: String(priority),
    street,
    condition,
    effect,
    action,
    sizes,
  }
}

export const RECOMMENDED_SETUP_PRESETS = [
  {
    id: "cash-6max-100bb-k256",
    label: "Cash 6-max · 100BB · K256",
    status: "暫定推奨",
    description:
      "2026-07-25 S3のCash anchor。5% rake / 4BB capとCanonical Cash Tree。",
  },
  {
    id: "tournament-6max-50bb-k128",
    label: "Tournament 6-max · 50BB · K128",
    status: "暫定推奨",
    description:
      "2026-07-25 S3のTournament anchor。12.5% ante、50/30/20 ICMとCanonical Tournament Tree。",
  },
] as const

export type RecommendedSetupPresetId =
  (typeof RECOMMENDED_SETUP_PRESETS)[number]["id"]

export const TREE_PRESETS = [
  {
    id: "compact-checkdown",
    label: "軽量Checkdown",
    description:
      "GUIの軽量初期Tree。Preflop sizeを編集し、Postflopはcheckdown。",
  },
  {
    id: "cash-canonical",
    label: "Canonical Cash",
    description: "2.5BB open、IP 3x / OOP 5x re-raise、50% pot postflop。",
  },
  {
    id: "tournament-canonical",
    label: "Canonical Tournament",
    description: "2BB open、2.5x 3bet、2x 4bet+、open cold-call制限。",
  },
  {
    id: "push-fold-10bb",
    label: "Push / Fold",
    description: "Preflopをall-in / call / foldへ限定し、Postflopをcheckdown。",
  },
  {
    id: "production-smoke",
    label: "Production Smoke",
    description:
      "mainのproduction smoke相当。標準size、各streetのaggression上限1。",
  },
] as const

export type TreePresetId = (typeof TREE_PRESETS)[number]["id"]

function standardBlindBb(seat: number, seatCount: number, button: number) {
  const smallBlind = seatCount === 2 ? button : (button + 1) % seatCount
  const bigBlind = (button + (seatCount === 2 ? 1 : 2)) % seatCount
  return seat === smallBlind ? "0.500" : seat === bigBlind ? "1.000" : "0.000"
}

function seatDefaults(seat: number, seatCount = 6, button = 0): SeatDraft {
  return {
    stackBb: "100.000",
    blindBb: standardBlindBb(seat, seatCount, button),
    anteBb: "0.000",
    range: "random",
  }
}

function applyStandardBlinds(
  seats: SeatDraft[],
  seatCount: number,
  button: number
) {
  return seats.map((seat, index) => ({
    ...seat,
    blindBb: standardBlindBb(index, seatCount, button),
  }))
}

export function createDefaultFormDraft(): FormSolveDraft {
  return {
    name: "6-max Cash · 100BB",
    seatCount: 6,
    button: 0,
    standardBlinds: true,
    preflopFirstToAct: "utg",
    commonAnteBb: "0.000",
    defaultStackBb: "100.000",
    defaultRange: "random",
    seats: Array.from({ length: 6 }, (_, seat) => seatDefaults(seat, 6, 0)),
    treeKind: "standard",
    treeScriptSource: "",
    treeScriptSourceId: null,
    treeScriptParams: [],
    treeAllowLimp: "false",
    treeAggressionCapsEnabled: false,
    treeAggressionCapPreflop: "6",
    treeAggressionCapFlop: "4",
    treeAggressionCapTurn: "4",
    treeAggressionCapRiver: "4",
    treeReraiseJamEnabled: false,
    treeReraiseJamNumerator: "1",
    treeReraiseJamDenominator: "3",
    treeRules: createDefaultTreeRules(),
    economics: {
      kind: "cash",
      cash: {
        rakeEnabled: false,
        rakeRate: "0.050",
        rakeCapEnabled: true,
        rakeCapBb: "4.000",
        rakeWhen: "flop_dealt",
        rakeAllocation: "main-first",
        rakeRounding: "down",
      },
      tournamentIcm: {
        payouts: "100\n60\n40",
        outsideFieldBb: "",
        samples: "100000",
        seed: "0",
      },
    },
    abstractionKind: "ehs2-percentile",
    flopBuckets: "64",
    turnBuckets: "64",
    riverBuckets: "64",
    recall: "current-street",
    solverKind: "range-vector",
    solverSeed: "0",
    opponentExploration: "0.0",
    batchSweeps: "1",
    discountKind: "periodic",
    discountEverySweeps: "10000",
    discountUntilSweeps: "10000000",
    pruningKind: "regret-based",
    maxSweeps: "5000000",
    maxTime: "",
    stopTarget: "0.050",
    stopCheckEverySweeps: "10000",
    stopConfirmations: "3",
    evaluationSamples: "4096",
    deviatorTraversals: "20000",
    threads: "auto",
    memory: "auto",
    checkpointInterval: "15m",
    probabilityEncoding: "u16",
  }
}

export function applyTreePreset(
  draft: FormSolveDraft,
  presetId: TreePresetId
): FormSolveDraft {
  const base: FormSolveDraft = {
    ...draft,
    treeKind: "standard",
    treeScriptSource: "",
    treeScriptSourceId: null,
    treeScriptParams: [],
    treeAllowLimp: "false",
    treeAggressionCapsEnabled: true,
    treeAggressionCapPreflop: "6",
    treeAggressionCapFlop: "4",
    treeAggressionCapTurn: "4",
    treeAggressionCapRiver: "4",
    treeReraiseJamEnabled: false,
    treeReraiseJamNumerator: "1",
    treeReraiseJamDenominator: "3",
  }

  if (presetId === "compact-checkdown") {
    return {
      ...base,
      treeAggressionCapsEnabled: false,
      treeRules: createDefaultTreeRules(),
    }
  }

  if (presetId === "production-smoke") {
    return {
      ...base,
      treeAggressionCapPreflop: "1",
      treeAggressionCapFlop: "1",
      treeAggressionCapTurn: "1",
      treeAggressionCapRiver: "1",
      treeRules: [],
    }
  }

  if (presetId === "push-fold-10bb") {
    return {
      ...base,
      treeAggressionCapPreflop: "1",
      treeAggressionCapFlop: "1",
      treeAggressionCapTurn: "1",
      treeAggressionCapRiver: "1",
      treeRules: [
        presetTreeRule(100, "preflop", "unopened", "replace", "raise", [
          "allin",
        ]),
        presetTreeRule(200, "postflop", "players >= 2", "checkdown", null),
      ],
    }
  }

  if (presetId === "cash-canonical") {
    return {
      ...base,
      treeReraiseJamEnabled: true,
      treeRules: [
        presetTreeRule(100, "preflop", "unopened", "replace", "raise", [
          "2.5bb",
        ]),
        presetTreeRule(
          100,
          "preflop",
          "aggressions >= 1 && in_position_to_last_aggressor",
          "replace",
          "raise",
          ["3x", "allin"]
        ),
        presetTreeRule(
          100,
          "preflop",
          "aggressions >= 1 && !in_position_to_last_aggressor",
          "replace",
          "raise",
          ["5x", "allin"]
        ),
        presetTreeRule(100, "postflop", "aggressions == 0", "replace", "bet", [
          "50%pot",
          "allin",
        ]),
        presetTreeRule(
          100,
          "postflop",
          "aggressions >= 1",
          "replace",
          "raise",
          ["2.5x", "allin"]
        ),
        presetTreeRule(
          200,
          "preflop",
          'aggressions == 1 && position != "BTN" && position != "SB" && position != "BB"',
          "remove",
          "call"
        ),
        presetTreeRule(
          200,
          "preflop",
          "aggressions >= 2 && !preflop_participant",
          "remove",
          "call"
        ),
      ],
    }
  }

  return {
    ...base,
    treeRules: [
      presetTreeRule(100, "preflop", "unopened", "replace", "raise", [
        "2bb",
        "allin",
      ]),
      presetTreeRule(100, "preflop", "aggressions == 1", "replace", "raise", [
        "2.5x",
        "allin",
      ]),
      presetTreeRule(100, "preflop", "aggressions >= 2", "replace", "raise", [
        "2x",
        "allin",
      ]),
      presetTreeRule(100, "postflop", "aggressions == 0", "replace", "bet", [
        "50%pot",
        "allin",
      ]),
      presetTreeRule(100, "postflop", "aggressions >= 1", "replace", "raise", [
        "2.5x",
        "allin",
      ]),
      presetTreeRule(
        200,
        "preflop",
        'aggressions == 1 && open_cold_calls >= 2 && position != "BB"',
        "remove",
        "call"
      ),
      presetTreeRule(
        200,
        "preflop",
        "aggressions >= 2 && !preflop_participant",
        "remove",
        "call"
      ),
    ],
  }
}

export function createRecommendedSetupPreset(
  presetId: RecommendedSetupPresetId
): FormSolveDraft {
  const isCash = presetId === "cash-6max-100bb-k256"
  let draft = withButton(createDefaultFormDraft(), 3)
  const stackBb = isCash ? "100.0" : "50.0"
  draft = {
    ...draft,
    name: isCash
      ? "Cash 6-max · 100BB · EHS² K256"
      : "Tournament 6-max · 50BB · EHS² K128",
    defaultStackBb: stackBb,
    seats: draft.seats.map((seat) => ({
      ...seat,
      stackBb,
      anteBb: isCash ? "0.0" : "0.125",
      range: "random",
    })),
    economics: {
      ...draft.economics,
      kind: isCash ? "cash" : "tournament-icm",
      cash: {
        ...draft.economics.cash,
        rakeEnabled: true,
        rakeRate: "0.05",
        rakeCapEnabled: true,
        rakeCapBb: "4.0",
        rakeWhen: "flop_dealt",
        rakeAllocation: "main-first",
        rakeRounding: "down",
      },
      tournamentIcm: {
        ...draft.economics.tournamentIcm,
        payouts: "50.0\n30.0\n20.0",
        outsideFieldBb: "",
      },
    },
    flopBuckets: isCash ? "256" : "128",
    turnBuckets: isCash ? "256" : "128",
    riverBuckets: isCash ? "256" : "128",
    solverSeed: "1011",
    opponentExploration: "0.0",
    batchSweeps: "1",
    discountKind: "periodic",
    discountEverySweeps: "10000",
    discountUntilSweeps: "10000000",
    pruningKind: "none",
    maxSweeps: "10000",
    stopTarget: "1000000.0",
    stopCheckEverySweeps: "10000",
    stopConfirmations: "1",
    evaluationSamples: "8192",
    deviatorTraversals: "20000",
    threads: "6",
    memory: "6GiB",
  }
  return applyTreePreset(
    draft,
    isCash ? "cash-canonical" : "tournament-canonical"
  )
}

export function withSeatCount(
  draft: FormSolveDraft,
  seatCount: number
): FormSolveDraft {
  const seats = Array.from(
    { length: seatCount },
    (_, seat) =>
      draft.seats[seat] ?? seatDefaults(seat, seatCount, draft.button)
  )
  const button = Math.min(draft.button, seatCount - 1)
  return {
    ...draft,
    seatCount,
    button,
    seats: draft.standardBlinds
      ? applyStandardBlinds(seats, seatCount, button)
      : seats,
  }
}

export function withButton(
  draft: FormSolveDraft,
  button: number
): FormSolveDraft {
  return {
    ...draft,
    button,
    seats: draft.standardBlinds
      ? applyStandardBlinds(draft.seats, draft.seatCount, button)
      : draft.seats,
  }
}

export function withStandardBlindsSetting(
  draft: FormSolveDraft,
  standardBlinds: boolean
): FormSolveDraft {
  return {
    ...draft,
    standardBlinds,
    seats: standardBlinds
      ? applyStandardBlinds(draft.seats, draft.seatCount, draft.button)
      : draft.seats,
  }
}

function renderTreeRules(rules: TreeRuleDraft[]) {
  if (rules.length > MAX_TREE_RULES) {
    throw new DraftTokenError(
      "game.tree.rules",
      `tree ruleは${MAX_TREE_RULES}件以下にしてください。`
    )
  }

  const lines: string[] = []
  rules.forEach((rule, index) => {
    const path = `game.tree.rules[${index}]`
    const condition = rule.condition.trim()
    if (!condition) {
      throw new DraftTokenError(
        `${path}.when`,
        "tree ruleのwhenは空にできません。"
      )
    }
    if (rule.sizes.length > MAX_TREE_RULE_SIZES) {
      throw new DraftTokenError(
        `${path}.sizes`,
        `sizesは${MAX_TREE_RULE_SIZES}件以下にしてください。`
      )
    }

    const sizes = rule.sizes.map((raw, sizeIndex) => {
      const value = raw.trim()
      if (!value) {
        throw new DraftTokenError(
          `${path}.sizes[${sizeIndex}]`,
          "size tokenは空にできません。"
        )
      }
      return value
    })

    if (rule.effect === "checkdown") {
      if (rule.action !== null || sizes.length > 0) {
        throw new DraftTokenError(
          path,
          "checkdown ruleではactionとsizesを指定できません。"
        )
      }
    } else if (rule.action === null) {
      throw new DraftTokenError(
        `${path}.action`,
        "checkdown以外のtree ruleにはactionが必要です。"
      )
    }

    if (rule.effect === "remove" && sizes.length > 0) {
      throw new DraftTokenError(
        `${path}.sizes`,
        "remove ruleはaction種別全体を削除するためsizesを指定できません。"
      )
    }
    if (
      rule.action !== null &&
      rule.action !== "bet" &&
      rule.action !== "raise" &&
      sizes.length > 0
    ) {
      throw new DraftTokenError(
        `${path}.sizes`,
        "sizesはbetまたはraise actionでのみ指定できます。"
      )
    }

    lines.push(
      "",
      "[[game.tree.rules]]",
      `priority = ${signedInteger(`${path}.priority`, rule.priority)}`,
      `street = ${tomlString(rule.street)}`,
      `when = ${tomlString(condition)}`,
      `effect = ${tomlString(rule.effect)}`
    )
    if (rule.action !== null) {
      lines.push(`action = ${tomlString(rule.action)}`)
    }
    if (sizes.length > 0) {
      lines.push(`sizes = [${sizes.map(tomlString).join(", ")}]`)
    }
  })
  return lines
}

function renderTreeScriptParam(param: TreeScriptParamDraft, index: number) {
  const path = `game.tree.params[${index}]`
  const key = param.key.trim()
  if (!key) {
    throw new DraftTokenError(`${path}.key`, "script parameter名は必須です。")
  }
  const raw = param.value.trim()
  switch (param.type) {
    case "string":
      return `${tomlString(key)} = ${tomlString(param.value)}`
    case "integer":
      return `${tomlString(key)} = ${signedInteger(`${path}.value`, raw)}`
    case "float":
      if (!SIGNED_DECIMAL.test(raw)) {
        throw new DraftTokenError(
          `${path}.value`,
          "script float parameterは有限10進数で入力してください。"
        )
      }
      return `${tomlString(key)} = ${raw}`
    case "boolean":
      if (raw !== "true" && raw !== "false") {
        throw new DraftTokenError(
          `${path}.value`,
          "script boolean parameterはtrueまたはfalseで指定してください。"
        )
      }
      return `${tomlString(key)} = ${raw}`
  }
}

export function renderFormDraftToml(draft: FormSolveDraft) {
  const seatCount = integer("game.seat_count", String(draft.seatCount))
  const button = integer("game.button", String(draft.button))
  const commonAnte = decimal("game.common_ante_bb", draft.commonAnteBb)
  const flopBuckets = integerAtLeast(
    "game.abstraction.buckets.flop",
    draft.flopBuckets,
    1n
  )
  const turnBuckets = integerAtLeast(
    "game.abstraction.buckets.turn",
    draft.turnBuckets,
    1n
  )
  const riverBuckets = integerAtLeast(
    "game.abstraction.buckets.river",
    draft.riverBuckets,
    1n
  )
  const maxSweeps = integer("run.max_sweeps", draft.maxSweeps)
  const maxTime = draft.maxTime.trim()
  if (maxTime && !DURATION.test(maxTime)) {
    throw new DraftTokenError(
      "run.max_time",
      "max timeは30s、15m、12hの形式で入力してください。"
    )
  }
  const stopTarget =
    draft.stopTarget.trim() === "default"
      ? tomlString("default")
      : positiveDecimal("run.stop.target", draft.stopTarget)
  const checkpointInterval = draft.checkpointInterval.trim()
  if (!DURATION.test(checkpointInterval)) {
    throw new DraftTokenError(
      "run.checkpoint.interval",
      "checkpoint intervalは30s、15m、12hの形式で入力してください。"
    )
  }
  const threads =
    draft.threads === "auto"
      ? tomlString("auto")
      : integer("run.resources.threads", draft.threads)
  const memoryToken = draft.memory.trim()
  const memory =
    memoryToken === "auto"
      ? tomlString("auto")
      : POSITIVE_INTEGER.test(memoryToken)
        ? memoryToken
        : MEMORY_WITH_UNIT.test(memoryToken)
          ? tomlString(memoryToken)
          : (() => {
              throw new DraftTokenError(
                "run.resources.memory",
                "memoryはauto、bytes整数、または12GiB等で入力してください。"
              )
            })()
  const memoryBytes =
    memoryToken === "auto"
      ? null
      : POSITIVE_INTEGER.test(memoryToken)
        ? BigInt(memoryToken)
        : (() => {
            const units = memoryToken.slice(-3)
            const amount = BigInt(memoryToken.slice(0, -3))
            const multiplier =
              units === "KiB"
                ? 1024n
                : units === "MiB"
                  ? 1024n * 1024n
                  : 1024n * 1024n * 1024n
            return amount * multiplier
          })()
  if (
    memoryBytes !== null &&
    memoryBytes > PRODUCTION_POLICY_ARENA_LIMIT_BYTES
  ) {
    throw new DraftTokenError(
      "run.resources.memory",
      "productionのpolicy arena上限は6GiBです。"
    )
  }
  const treeRuleLines =
    draft.treeKind === "standard" ? renderTreeRules(draft.treeRules) : []
  const firstActor =
    draft.preflopFirstToAct === "utg"
      ? tomlString("utg")
      : integer("game.preflop_first_to_act", draft.preflopFirstToAct)

  const lines = [
    'schema = "solvers.multiway-preflop/v1"',
    "",
    "[game]",
    `seat_count = ${seatCount}`,
    `button = ${button}`,
    `standard_blinds = ${draft.standardBlinds}`,
    `preflop_first_to_act = ${firstActor}`,
    `common_ante_bb = ${commonAnte}`,
    "",
    "[game.defaults]",
    `stack_bb = ${positiveDecimal(
      "game.defaults.stack_bb",
      draft.defaultStackBb
    )}`,
    `range = ${tomlString(draft.defaultRange)}`,
  ]

  draft.seats.forEach((seat, index) => {
    lines.push(
      "",
      "[[game.players]]",
      `seat = ${index}`,
      `stack_bb = ${decimal(`game.players[${index}].stack_bb`, seat.stackBb)}`,
      `range = ${tomlString(seat.range)}`,
      `blind_bb = ${decimal(`game.players[${index}].blind_bb`, seat.blindBb)}`,
      `ante_bb = ${decimal(`game.players[${index}].ante_bb`, seat.anteBb)}`
    )
  })

  if (draft.treeKind === "standard") {
    lines.push("", "[game.tree]", 'kind = "standard"')
  } else {
    const source = draft.treeScriptSource.trim()
    if (!source) {
      throw new DraftTokenError(
        "game.tree.source",
        "script treeでは.mwtree sourceを選択してください。"
      )
    }
    lines.push(
      "",
      "[game.tree]",
      'kind = "script"',
      `source = ${tomlString(source)}`
    )
  }
  if (draft.treeAllowLimp !== "default") {
    lines.push(`allow_limp = ${draft.treeAllowLimp}`)
  }
  if (draft.treeReraiseJamEnabled) {
    lines.push(
      `reraise_jam_above_actor_starting_stack = { numerator = ${integerAtLeast(
        "game.tree.reraise_jam_above_actor_starting_stack.numerator",
        draft.treeReraiseJamNumerator,
        1n
      )}, denominator = ${integerAtLeast(
        "game.tree.reraise_jam_above_actor_starting_stack.denominator",
        draft.treeReraiseJamDenominator,
        1n
      )} }`
    )
  }
  if (draft.treeAggressionCapsEnabled) {
    lines.push(
      "",
      "[game.tree.max_aggressive_actions]",
      `preflop = ${integer(
        "game.tree.max_aggressive_actions.preflop",
        draft.treeAggressionCapPreflop
      )}`,
      `flop = ${integer(
        "game.tree.max_aggressive_actions.flop",
        draft.treeAggressionCapFlop
      )}`,
      `turn = ${integer(
        "game.tree.max_aggressive_actions.turn",
        draft.treeAggressionCapTurn
      )}`,
      `river = ${integer(
        "game.tree.max_aggressive_actions.river",
        draft.treeAggressionCapRiver
      )}`
    )
  }
  if (draft.treeKind === "standard") {
    lines.push(...treeRuleLines)
  } else {
    if (draft.treeScriptParams.length > 0) {
      lines.push(
        "",
        "[game.tree.params]",
        ...draft.treeScriptParams.map(renderTreeScriptParam)
      )
    }
  }
  lines.push("", "[game.abstraction]", 'kind = "ehs2-percentile"')
  lines.push(
    "",
    "[game.abstraction.buckets]",
    `flop = ${flopBuckets}`,
    `turn = ${turnBuckets}`,
    `river = ${riverBuckets}`
  )
  lines.push("", "[game.information]", 'recall = "current-street"')

  if (draft.economics.kind === "cash") {
    lines.push("", "[economics]", 'kind = "cash"')
    if (draft.economics.cash.rakeEnabled) {
      const rakeLines = [
        "",
        "[economics.rake]",
        `rate = ${decimal(
          "economics.rake.rate",
          draft.economics.cash.rakeRate
        )}`,
      ]
      if (draft.economics.cash.rakeCapEnabled) {
        rakeLines.push(
          `cap_bb = ${decimal(
            "economics.rake.cap_bb",
            draft.economics.cash.rakeCapBb
          )}`
        )
      }
      rakeLines.push(
        `when = ${tomlString(draft.economics.cash.rakeWhen)}`,
        `allocation = ${tomlString(draft.economics.cash.rakeAllocation)}`,
        "rounding_unit_bb = 0.001",
        `rounding = ${tomlString(draft.economics.cash.rakeRounding)}`
      )
      lines.push(...rakeLines)
    }
  } else {
    const payouts = decimalList(
      "economics.payouts",
      draft.economics.tournamentIcm.payouts
    )
    if (payouts.length === 0) {
      throw new DraftTokenError(
        "economics.payouts",
        "Tournament ICMでは賞金を1件以上入力してください。"
      )
    }
    const outsideField = decimalList(
      "economics.outside_field_bb",
      draft.economics.tournamentIcm.outsideFieldBb
    )
    outsideField.forEach((value, index) => {
      if (/^0(?:\.0+)?$/.test(value)) {
        throw new DraftTokenError(
          `economics.outside_field_bb[${index}]`,
          "卓外stackは正のBB値で入力してください。"
        )
      }
    })
    const fieldPlayers = draft.seatCount + outsideField.length
    if (fieldPlayers > 10_000) {
      throw new DraftTokenError(
        "economics.outside_field_bb",
        "ICM fieldは卓内と卓外を合わせて10,000人以下にしてください。"
      )
    }
    if (payouts.length > fieldPlayers) {
      throw new DraftTokenError(
        "economics.payouts",
        `賞金はfield人数（${fieldPlayers}人）以下の件数にしてください。`
      )
    }

    lines.push(
      "",
      "[economics]",
      'kind = "tournament-icm"',
      `payouts = [${payouts.join(", ")}]`,
      `outside_field_bb = [${outsideField.join(", ")}]`
    )
    if (fieldPlayers > 15) {
      lines.push(
        `samples = ${integerAtLeast(
          "economics.samples",
          draft.economics.tournamentIcm.samples,
          2n
        )}`,
        `seed = ${integer(
          "economics.seed",
          draft.economics.tournamentIcm.seed
        )}`
      )
    }
  }

  if (
    draft.solverKind === "single-hand" &&
    draft.pruningKind === "regret-based"
  ) {
    throw new DraftTokenError(
      "solver.pruning.kind",
      "single-hand solverではregret-based pruningを使用できません。"
    )
  }
  lines.push(
    "",
    "[solver]",
    `kind = ${tomlString(draft.solverKind)}`,
    `seed = ${integer("solver.seed", draft.solverSeed)}`,
    `opponent_exploration = ${decimal(
      "solver.opponent_exploration",
      draft.opponentExploration
    )}`,
    `batch_sweeps = ${integerAtLeast(
      "solver.batch_sweeps",
      draft.batchSweeps,
      1n
    )}`,
    "",
    "[solver.discount]",
    `kind = ${tomlString(draft.discountKind)}`
  )
  if (draft.discountKind === "periodic") {
    lines.push(
      `every_sweeps = ${integerAtLeast(
        "solver.discount.every_sweeps",
        draft.discountEverySweeps,
        1n
      )}`,
      `until_sweeps = ${integer(
        "solver.discount.until_sweeps",
        draft.discountUntilSweeps
      )}`
    )
  }
  lines.push(
    "",
    "[solver.pruning]",
    `kind = ${tomlString(draft.pruningKind)}`,
    "",
    "[run]",
    `max_sweeps = ${maxSweeps}`
  )
  if (maxTime) {
    lines.push(`max_time = ${tomlString(maxTime)}`)
  }
  lines.push(
    "",
    "[run.resources]",
    `threads = ${threads}`,
    `memory = ${memory}`,
    "",
    "[run.stop]",
    `target = ${stopTarget}`,
    `check_every_sweeps = ${integerAtLeast(
      "run.stop.check_every_sweeps",
      draft.stopCheckEverySweeps,
      1n
    )}`,
    `confirmations = ${integerAtLeast(
      "run.stop.confirmations",
      draft.stopConfirmations,
      1n
    )}`,
    `evaluation_samples = ${integerAtLeast(
      "run.stop.evaluation_samples",
      draft.evaluationSamples,
      1n
    )}`,
    `deviator_traversals = ${integerAtLeast(
      "run.stop.deviator_traversals",
      draft.deviatorTraversals,
      1n
    )}`,
    "",
    "[run.checkpoint]",
    `interval = ${tomlString(checkpointInterval)}`,
    "",
    "[output]",
    `probability_encoding = ${tomlString(draft.probabilityEncoding)}`,
    ""
  )

  return lines.join("\n")
}
