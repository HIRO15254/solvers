import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

import { describe, expect, test } from "bun:test"

import {
  applyTreePreset,
  createDefaultFormDraft,
  createDefaultPostflopTreeBuilder,
  createDefaultPreflopTreeBuilder,
  createPostflopTreeRules,
  createPreflopTreeRules,
  createRecommendedSetupPreset,
  DraftTokenError,
  renderFormDraftToml,
  summarizeDecimalTokenList,
  type TreeRuleDraft,
  withButton,
  withSeatCount,
} from "../src/lib/setup-config"

const guiDefaultFixture = fileURLToPath(
  new URL(
    "../../../examples/preflop_multiway_v1_gui_default.toml",
    import.meta.url
  )
)
const guiFullSurfaceFixture = fileURLToPath(
  new URL(
    "../../../examples/preflop_multiway_v1_gui_full_surface.toml",
    import.meta.url
  )
)

describe("Solve設定フォーム", () => {
  test("GUI既定値とRust側preflight fixtureが一致する", () => {
    const draft = createDefaultFormDraft()
    expect(draft.seats.every((seat) => seat.range === "random")).toBe(true)
    expect(renderFormDraftToml(draft)).toBe(
      readFileSync(guiDefaultFixture, "utf8")
    )
  })

  test("mainのCash/Tournament暫定推奨構成をproduction TOMLへ展開する", () => {
    const cash = createRecommendedSetupPreset("cash-6max-100bb-k256")
    const cashToml = renderFormDraftToml(cash)
    expect(cash.name).toContain("K256")
    expect(cashToml).toContain("flop = 256")
    expect(cashToml).toContain("rate = 0.05")
    expect(cashToml).toContain('when = "unopened"')
    expect(cashToml).toContain('sizes = ["2.5bb"]')
    expect(cashToml).toContain(
      'when = "aggressions >= 1 && in_position_to_last_aggressor"'
    )
    expect(cashToml).toContain(
      "reraise_jam_above_actor_starting_stack = { numerator = 1, denominator = 3 }"
    )

    const tournament = createRecommendedSetupPreset("tournament-6max-50bb-k128")
    const tournamentToml = renderFormDraftToml(tournament)
    expect(tournament.name).toContain("K128")
    expect(tournament.seats.every((seat) => seat.anteBb === "0.125")).toBe(true)
    expect(tournamentToml).toContain('kind = "tournament-icm"')
    expect(tournamentToml).toContain("payouts = [50.0, 30.0, 20.0]")
    expect(tournamentToml).toContain("flop = 128")
    expect(tournamentToml).toContain(
      'when = "aggressions == 1 && open_cold_calls >= 2 && position != \\"BB\\""'
    )
  })

  test("main由来のサンプルTreeを他設定を保ったまま切り替える", () => {
    const draft = createDefaultFormDraft()
    draft.economics.kind = "tournament-icm"
    draft.flopBuckets = "91"

    const cash = applyTreePreset(draft, "cash-canonical")
    expect(cash.economics.kind).toBe("tournament-icm")
    expect(cash.flopBuckets).toBe("91")
    expect(cash.treeRules).toHaveLength(7)
    expect(cash.treeReraiseJamEnabled).toBe(true)

    const tournament = applyTreePreset(draft, "tournament-canonical")
    expect(tournament.treeRules).toHaveLength(7)
    expect(tournament.treeRules[0]?.sizes).toEqual(["2bb", "allin"])

    const pushFold = applyTreePreset(draft, "push-fold-10bb")
    expect(pushFold.treeAggressionCapPreflop).toBe("1")
    expect(pushFold.treeRules[0]?.sizes).toEqual(["allin"])
    expect(pushFold.treeRules[1]?.effect).toBe("checkdown")

    const smoke = applyTreePreset(draft, "production-smoke")
    expect(smoke.treeRules).toEqual([])
    expect(smoke.treeAggressionCapRiver).toBe("1")
  })

  test("Preflop Tree Builderがopen・limp後raise・re-raise・capをtyped rule化する", () => {
    const builder = createDefaultPreflopTreeBuilder()
    builder.unopenedSizes = ["2.2x", "allin"]
    builder.maxAggressions = "3"
    builder.postflop = "full-tree"

    const rules = createPreflopTreeRules(builder)
    expect(rules.map((rule) => rule.condition)).toEqual([
      "unopened",
      "limpers > 0 && aggressions == 0",
      "aggressions >= 1",
      "aggressions >= 3",
    ])
    expect(rules[0]?.sizes).toEqual(["2.2x", "allin"])
    expect(rules.at(-1)?.effect).toBe("remove")
  })

  test("Postflop Tree Builderが各streetのbet・raise・capをtyped rule化する", () => {
    const builder = createDefaultPostflopTreeBuilder()
    builder.flop.betSizes = ["33%pot", "75%pot", "allin"]
    builder.river.raiseSizes = ["min", "allin"]
    builder.maxAggressions = "2"

    const rules = createPostflopTreeRules(builder)
    expect(rules).toHaveLength(9)
    expect(rules[0]).toMatchObject({
      street: "flop",
      action: "bet",
      sizes: ["33%pot", "75%pot", "allin"],
    })
    expect(rules[7]).toMatchObject({
      street: "river",
      action: "raise",
      sizes: ["min", "allin"],
    })
    expect(rules[8]).toMatchObject({
      condition: "aggressions >= 2",
      effect: "remove",
      action: "raise",
    })
  })

  test("全v1 form controlをTOMLへlosslessに出力する", () => {
    const draft = createDefaultFormDraft()
    draft.standardBlinds = false
    draft.preflopFirstToAct = "4"
    draft.defaultStackBb = "80.000"
    draft.defaultRange = "QQ+"
    draft.treeAllowLimp = "true"
    draft.treeAggressionCapsEnabled = true
    draft.treeAggressionCapPreflop = "6"
    draft.treeAggressionCapFlop = "3"
    draft.treeAggressionCapTurn = "2"
    draft.treeAggressionCapRiver = "1"
    draft.treeReraiseJamEnabled = true
    draft.treeReraiseJamNumerator = "1"
    draft.treeReraiseJamDenominator = "3"
    draft.flopBuckets = "32"
    draft.turnBuckets = "48"
    draft.riverBuckets = "96"
    draft.solverKind = "single-hand"
    draft.solverSeed = "19"
    draft.opponentExploration = "0.125"
    draft.batchSweeps = "3"
    draft.discountKind = "none"
    draft.pruningKind = "none"
    draft.maxTime = "12h"
    draft.stopCheckEverySweeps = "2000"
    draft.stopConfirmations = "5"
    draft.evaluationSamples = "8192"
    draft.deviatorTraversals = "40000"
    draft.probabilityEncoding = "f32"

    const toml = renderFormDraftToml(draft)
    expect(toml).toBe(readFileSync(guiFullSurfaceFixture, "utf8"))
    expect(toml).toContain("standard_blinds = false")
    expect(toml).toContain("preflop_first_to_act = 4")
    expect(toml).toContain("stack_bb = 80.000")
    expect(toml).toContain('range = "QQ+"')
    expect(toml).toContain("allow_limp = true")
    expect(toml).toContain(
      "reraise_jam_above_actor_starting_stack = { numerator = 1, denominator = 3 }"
    )
    expect(toml).toContain("[game.tree.max_aggressive_actions]")
    expect(toml).toContain("river = 1")
    expect(toml).toContain('kind = "single-hand"')
    expect(toml).toContain("opponent_exploration = 0.125")
    expect(toml).toContain('max_time = "12h"')
    expect(toml).toContain("evaluation_samples = 8192")
    expect(toml).toContain('probability_encoding = "f32"')
    expect(toml).not.toContain("\nevery_sweeps =")
  })

  test("production abstractionはretired fieldを出力しない", () => {
    const draft = createDefaultFormDraft()

    const toml = renderFormDraftToml(draft)
    const abstraction = toml.slice(
      toml.indexOf("[game.abstraction]"),
      toml.indexOf("[game.information]")
    )
    expect(abstraction).toContain('kind = "ehs2-percentile"')
    expect(abstraction).not.toContain("rollouts_per_state")
    expect(abstraction).not.toContain("opponent_buckets")
    expect(toml).toContain('recall = "current-street"')
  })

  test("uncapped rake・condition・allocation・roundingを出力する", () => {
    const draft = createDefaultFormDraft()
    draft.economics.cash.rakeEnabled = true
    draft.economics.cash.rakeCapEnabled = false
    draft.economics.cash.rakeWhen = "players_saw_flop >= 3"
    draft.economics.cash.rakeAllocation = "proportional"
    draft.economics.cash.rakeRounding = "nearest"

    const toml = renderFormDraftToml(draft)
    expect(toml).not.toContain("cap_bb =")
    expect(toml).toContain('when = "players_saw_flop >= 3"')
    expect(toml).toContain('allocation = "proportional"')
    expect(toml).toContain('rounding = "nearest"')
  })

  test(".mwtree sourceと全scalar parameter型を出力する", () => {
    const draft = createDefaultFormDraft()
    draft.treeKind = "script"
    draft.treeScriptSource = "short-stack.mwtree"
    draft.treeScriptParams = [
      { id: "s", key: "open", type: "string", value: "2.2x" },
      { id: "i", key: "streets", type: "integer", value: "3" },
      { id: "f", key: "jam_spr", type: "float", value: "0.8" },
      { id: "b", key: "enabled", type: "boolean", value: "true" },
    ]

    const toml = renderFormDraftToml(draft)
    expect(toml).toContain('kind = "script"')
    expect(toml).toContain('source = "short-stack.mwtree"')
    expect(toml).toContain('"open" = "2.2x"')
    expect(toml).toContain('"streets" = 3')
    expect(toml).toContain('"jam_spr" = 0.8')
    expect(toml).toContain('"enabled" = true')
    expect(toml).not.toContain("[[game.tree.rules]]")
  })

  test("tree ruleの順序・符号付きpriority・size tokenをlosslessに出力する", () => {
    const draft = createDefaultFormDraft()
    const rules: TreeRuleDraft[] = [
      {
        id: "first",
        priority: "-10",
        street: "flop",
        condition: 'in_position && position == "BTN"',
        effect: "replace",
        action: "bet",
        sizes: ["geometric(allin, streets=3)", "allin"],
      },
      {
        id: "second",
        priority: "200",
        street: "postflop",
        condition: "players >= 4",
        effect: "checkdown",
        action: null,
        sizes: [],
      },
    ]

    const toml = renderFormDraftToml({ ...draft, treeRules: rules })
    const first = toml.indexOf('priority = -10\nstreet = "flop"')
    const second = toml.indexOf('priority = 200\nstreet = "postflop"')

    expect(first).toBeGreaterThan(-1)
    expect(second).toBeGreaterThan(first)
    expect(toml).toContain('sizes = ["geometric(allin, streets=3)", "allin"]')
    expect(
      toml.slice(second, toml.indexOf("[game.abstraction]"))
    ).not.toContain("action =")
  })

  test("remove ruleに個別sizeを指定した下書きは拒否する", () => {
    const draft = createDefaultFormDraft()
    draft.treeRules = [
      {
        id: "invalid-remove",
        priority: "100",
        street: "preflop",
        condition: "unopened",
        effect: "remove",
        action: "raise",
        sizes: ["2.5x"],
      },
    ]

    expect(() => renderFormDraftToml(draft)).toThrow(DraftTokenError)
  })

  test("raw byteメモリはTOML整数、単位付きメモリは文字列で出力する", () => {
    const draft = createDefaultFormDraft()
    expect(renderFormDraftToml({ ...draft, memory: "1073741824" })).toContain(
      "memory = 1073741824"
    )
    expect(renderFormDraftToml({ ...draft, memory: "1GiB" })).toContain(
      'memory = "1GiB"'
    )
    expect(() => renderFormDraftToml({ ...draft, memory: "7GiB" })).toThrow(
      DraftTokenError
    )
  })

  test("seat数とbutton変更時にstandard blindを再配置する", () => {
    const headsUp = withSeatCount(createDefaultFormDraft(), 2)
    expect(headsUp.seats.map((seat) => seat.blindBb)).toEqual([
      "0.500",
      "1.000",
    ])

    const rotated = withButton(createDefaultFormDraft(), 4)
    expect(rotated.seats.map((seat) => seat.blindBb)).toEqual([
      "1.000",
      "0.000",
      "0.000",
      "0.000",
      "0.000",
      "0.500",
    ])
  })

  test("6人exact ICMをrake・samples・seedなしで生成する", () => {
    const draft = createDefaultFormDraft()
    draft.economics.kind = "tournament-icm"
    draft.economics.cash.rakeEnabled = true
    draft.economics.tournamentIcm.payouts = "1000\n600, 400"
    draft.economics.tournamentIcm.outsideFieldBb = ""
    draft.stopTarget = "default"

    const toml = renderFormDraftToml(draft)
    const economics = toml.slice(
      toml.indexOf("[economics]"),
      toml.indexOf("\n[solver]")
    )

    expect(economics).toContain('kind = "tournament-icm"')
    expect(economics).toContain("payouts = [1000, 600, 400]")
    expect(economics).toContain("outside_field_bb = []")
    expect(economics).not.toContain("[economics.rake]")
    expect(economics).not.toContain("\nsamples =")
    expect(economics).not.toContain("\nseed =")
    expect(toml).toContain('target = "default"')
  })

  test("ICM fieldの15人exact / 16人sampled境界を正しく出力する", () => {
    const draft = createDefaultFormDraft()
    draft.economics.kind = "tournament-icm"
    draft.economics.tournamentIcm.payouts = "100 60 40"
    draft.economics.tournamentIcm.outsideFieldBb = Array.from(
      { length: 9 },
      (_, index) => String(index + 10)
    ).join("\n")

    const exact = renderFormDraftToml(draft)
    const exactEconomics = exact.slice(
      exact.indexOf("[economics]"),
      exact.indexOf("\n[solver]")
    )
    expect(exactEconomics).not.toContain("\nsamples =")
    expect(exactEconomics).not.toContain("\nseed =")

    draft.economics.tournamentIcm.outsideFieldBb += "\n30"
    draft.economics.tournamentIcm.samples = "250000"
    draft.economics.tournamentIcm.seed = "17"
    const sampled = renderFormDraftToml(draft)
    expect(sampled).toContain("samples = 250000")
    expect(sampled).toContain("seed = 17")
  })

  test("ICMの賞金件数超過とsample数1をフォーム段階で拒否する", () => {
    const tooManyPayouts = withSeatCount(createDefaultFormDraft(), 2)
    tooManyPayouts.economics.kind = "tournament-icm"
    tooManyPayouts.economics.tournamentIcm.payouts = "100 60 40"
    expect(() => renderFormDraftToml(tooManyPayouts)).toThrow(DraftTokenError)

    const sampled = createDefaultFormDraft()
    sampled.economics.kind = "tournament-icm"
    sampled.economics.tournamentIcm.outsideFieldBb = Array.from(
      { length: 10 },
      () => "20"
    ).join("\n")
    sampled.economics.tournamentIcm.samples = "1"
    expect(() => renderFormDraftToml(sampled)).toThrow(DraftTokenError)
  })

  test("ICM summaryは末尾0を有賞順位に数えず総賞金をlosslessに加算する", () => {
    expect(
      summarizeDecimalTokenList(
        "100000000000000000000.125, 60.875, 40, 0, 0.000"
      )
    ).toEqual({
      tokens: ["100000000000000000000.125", "60.875", "40", "0", "0.000"],
      valid: true,
      paidPlaces: 3,
      total: "100000000000000000101",
    })
    expect(summarizeDecimalTokenList("100, nope, 40")).toMatchObject({
      valid: false,
      paidPlaces: null,
      total: null,
    })
  })
})
