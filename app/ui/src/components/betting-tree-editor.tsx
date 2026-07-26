import { useState } from "react"
import {
  IconArrowDown,
  IconArrowUp,
  IconCopy,
  IconFileCode,
  IconGitBranch,
  IconInfoCircle,
  IconPlus,
  IconTrash,
} from "@tabler/icons-react"

import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
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
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import {
  copyTreeRuleDraft,
  createDefaultPostflopTreeBuilder,
  createDefaultPreflopTreeBuilder,
  createPostflopTreeRules,
  createPreflopTreeRules,
  createTreeScriptParamDraft,
  createTreeRuleDraft,
  MAX_TREE_RULES,
  MAX_TREE_RULE_SIZES,
  type PostflopTreeBuilderDraft,
  type PreflopTreeBuilderDraft,
  type TreeRuleAction,
  type TreeRuleDraft,
  type TreeRuleEffect,
  type TreeRuleStreet,
  type TreeScriptParamDraft,
} from "@/lib/setup-config"

type BettingTreeEditorProps = {
  treeKind: "standard" | "script"
  scriptSource: string
  scriptParams: TreeScriptParamDraft[]
  allowLimp: "default" | "true" | "false"
  aggressionCapsEnabled: boolean
  aggressionCaps: {
    preflop: string
    flop: string
    turn: string
    river: string
  }
  reraiseJamEnabled: boolean
  reraiseJamNumerator: string
  reraiseJamDenominator: string
  rules: TreeRuleDraft[]
  onChange: (rules: TreeRuleDraft[]) => void
  onTreeConfigChange: (update: {
    treeKind?: "standard" | "script"
    treeScriptSource?: string
    treeScriptParams?: TreeScriptParamDraft[]
    treeAllowLimp?: "default" | "true" | "false"
    treeAggressionCapsEnabled?: boolean
    treeAggressionCapPreflop?: string
    treeAggressionCapFlop?: string
    treeAggressionCapTurn?: string
    treeAggressionCapRiver?: string
    treeReraiseJamEnabled?: boolean
    treeReraiseJamNumerator?: string
    treeReraiseJamDenominator?: string
  }) => void
  onPickScript: () => void
  disabled?: boolean
}

const SIGNED_INTEGER = /^-?(?:0|[1-9]\d*)$/

const streetOptions: Array<{ value: TreeRuleStreet; label: string }> = [
  { value: "preflop", label: "Preflop" },
  { value: "flop", label: "Flop" },
  { value: "turn", label: "Turn" },
  { value: "river", label: "River" },
  { value: "postflop", label: "Postflop全体" },
]

const effectOptions: Array<{ value: TreeRuleEffect; label: string }> = [
  { value: "add", label: "Add" },
  { value: "remove", label: "Remove" },
  { value: "replace", label: "Replace" },
  { value: "force", label: "Force" },
  { value: "checkdown", label: "Checkdown" },
]

const actionOptions: Array<{ value: TreeRuleAction; label: string }> = [
  { value: "fold", label: "Fold" },
  { value: "check", label: "Check" },
  { value: "call", label: "Call" },
  { value: "bet", label: "Bet" },
  { value: "raise", label: "Raise" },
]

function ruleErrors(rule: TreeRuleDraft) {
  const priority = SIGNED_INTEGER.test(rule.priority.trim())
    ? null
    : "priorityは符号付き整数で入力してください。"
  const condition = rule.condition.trim()
    ? null
    : "when条件を入力してください。"
  const action =
    rule.effect !== "checkdown" && rule.action === null
      ? "checkdown以外ではactionが必要です。"
      : null
  const sizes =
    rule.sizes.length > MAX_TREE_RULE_SIZES
      ? `sizesは${MAX_TREE_RULE_SIZES}件以下にしてください。`
      : rule.sizes.some((size) => !size.trim())
        ? "空のsize tokenがあります。"
        : rule.effect === "checkdown" &&
            (rule.action !== null || rule.sizes.length > 0)
          ? "checkdownではactionとsizesを指定できません。"
          : rule.effect === "remove" && rule.sizes.length > 0
            ? "removeはaction種別全体を削除するためsizesを指定できません。"
            : rule.action !== null &&
                rule.action !== "bet" &&
                rule.action !== "raise" &&
                rule.sizes.length > 0
              ? "sizesはbetまたはraiseでのみ指定できます。"
              : null
  return { priority, condition, action, sizes }
}

function ruleSummary(rule: TreeRuleDraft) {
  if (rule.effect === "checkdown") {
    return "checkdown"
  }
  return `${rule.effect} ${rule.action ?? "action未選択"}`
}

export function BettingTreeEditor({
  treeKind,
  scriptSource,
  scriptParams,
  allowLimp,
  aggressionCapsEnabled,
  aggressionCaps,
  reraiseJamEnabled,
  reraiseJamNumerator,
  reraiseJamDenominator,
  rules,
  onChange,
  onTreeConfigChange,
  onPickScript,
  disabled = false,
}: BettingTreeEditorProps) {
  const [openRuleIds, setOpenRuleIds] = useState<string[]>(() =>
    rules[0] ? [rules[0].id] : []
  )
  const [preflopBuilder, setPreflopBuilder] = useState<PreflopTreeBuilderDraft>(
    createDefaultPreflopTreeBuilder
  )
  const [postflopBuilder, setPostflopBuilder] =
    useState<PostflopTreeBuilderDraft>(createDefaultPostflopTreeBuilder)
  const [builderError, setBuilderError] = useState<string | null>(null)

  const updateBuilder = (patch: Partial<PreflopTreeBuilderDraft>) => {
    setPreflopBuilder((current) => ({ ...current, ...patch }))
    setBuilderError(null)
  }

  const applyPreflopBuilder = () => {
    try {
      const generated = createPreflopTreeRules(preflopBuilder)
      const preflopRules = generated.filter((rule) => rule.street === "preflop")
      const defaultCheckdown = generated.find(
        (rule) => rule.street === "postflop"
      )
      const existingPostflopRules = rules.filter(
        (rule) =>
          rule.street !== "preflop" &&
          !(
            rule.street === "postflop" &&
            rule.effect === "checkdown" &&
            rule.condition.trim() === "players >= 2"
          )
      )
      const next =
        preflopBuilder.postflop === "checkdown" && defaultCheckdown
          ? [...preflopRules, defaultCheckdown]
          : [...preflopRules, ...existingPostflopRules]
      onChange(next)
      setOpenRuleIds([])
      setBuilderError(null)
    } catch (error) {
      setBuilderError(
        error instanceof Error ? error.message : "Tree設定を適用できません。"
      )
    }
  }

  const updatePostflopStreet = (
    street: "flop" | "turn" | "river",
    field: "betSizes" | "raiseSizes",
    sizes: string[]
  ) => {
    setPostflopBuilder((current) => ({
      ...current,
      [street]: { ...current[street], [field]: sizes },
    }))
    setBuilderError(null)
  }

  const applyPostflopBuilder = () => {
    try {
      const generated = createPostflopTreeRules(postflopBuilder)
      const preflopRules = rules.filter((rule) => rule.street === "preflop")
      onChange([...preflopRules, ...generated])
      setPreflopBuilder((current) => ({
        ...current,
        postflop: "full-tree",
      }))
      setOpenRuleIds([])
      setBuilderError(null)
    } catch (error) {
      setBuilderError(
        error instanceof Error ? error.message : "Tree設定を適用できません。"
      )
    }
  }

  const patchRule = (index: number, patch: Partial<TreeRuleDraft>) => {
    onChange(
      rules.map((rule, ruleIndex) =>
        ruleIndex === index ? { ...rule, ...patch } : rule
      )
    )
  }

  const addRule = () => {
    if (rules.length >= MAX_TREE_RULES) {
      return
    }
    const rule = createTreeRuleDraft()
    onChange([...rules, rule])
    setOpenRuleIds((current) =>
      current.includes(rule.id) ? current : [...current, rule.id]
    )
  }

  const removeRule = (index: number) => {
    const removed = rules[index]
    onChange(rules.filter((_, ruleIndex) => ruleIndex !== index))
    if (removed) {
      setOpenRuleIds((current) =>
        current.filter((ruleId) => ruleId !== removed.id)
      )
    }
  }

  const duplicateRule = (index: number) => {
    const source = rules[index]
    if (!source || rules.length >= MAX_TREE_RULES) {
      return
    }
    const copy = copyTreeRuleDraft(source)
    const next = [...rules]
    next.splice(index + 1, 0, copy)
    onChange(next)
    setOpenRuleIds((current) => [...current, copy.id])
  }

  const moveRule = (index: number, offset: -1 | 1) => {
    const destination = index + offset
    if (destination < 0 || destination >= rules.length) {
      return
    }
    const next = [...rules]
    ;[next[index], next[destination]] = [next[destination], next[index]]
    onChange(next)
  }

  const changeEffect = (index: number, effect: TreeRuleEffect) => {
    const rule = rules[index]
    if (!rule) {
      return
    }
    if (effect === "checkdown") {
      patchRule(index, { effect, action: null, sizes: [] })
      return
    }
    if (effect === "remove") {
      patchRule(index, {
        effect,
        action: rule.action ?? "raise",
        sizes: [],
      })
      return
    }
    patchRule(index, { effect, action: rule.action ?? "raise" })
  }

  const changeAction = (index: number, action: TreeRuleAction) => {
    const rule = rules[index]
    if (!rule) {
      return
    }
    patchRule(index, {
      action,
      sizes:
        action === "bet" || action === "raise" ? rule.sizes : ([] as string[]),
    })
  }

  const addSize = (index: number) => {
    const rule = rules[index]
    if (
      !rule ||
      rule.sizes.length >= MAX_TREE_RULE_SIZES ||
      rule.effect === "remove" ||
      (rule.action !== "bet" && rule.action !== "raise")
    ) {
      return
    }
    patchRule(index, {
      sizes: [...rule.sizes, rule.action === "bet" ? "50%pot" : "2.5x"],
    })
  }

  const updateSize = (ruleIndex: number, sizeIndex: number, value: string) => {
    const rule = rules[ruleIndex]
    if (!rule) {
      return
    }
    patchRule(ruleIndex, {
      sizes: rule.sizes.map((size, index) =>
        index === sizeIndex ? value : size
      ),
    })
  }

  const removeSize = (ruleIndex: number, sizeIndex: number) => {
    const rule = rules[ruleIndex]
    if (!rule) {
      return
    }
    patchRule(ruleIndex, {
      sizes: rule.sizes.filter((_, index) => index !== sizeIndex),
    })
  }

  const patchScriptParam = (
    index: number,
    patch: Partial<TreeScriptParamDraft>
  ) => {
    onTreeConfigChange({
      treeScriptParams: scriptParams.map((param, paramIndex) =>
        paramIndex === index ? { ...param, ...patch } : param
      ),
    })
  }

  const patchAggressionCap = (
    street: "preflop" | "flop" | "turn" | "river",
    value: string
  ) => {
    switch (street) {
      case "preflop":
        onTreeConfigChange({ treeAggressionCapPreflop: value })
        break
      case "flop":
        onTreeConfigChange({ treeAggressionCapFlop: value })
        break
      case "turn":
        onTreeConfigChange({ treeAggressionCapTurn: value })
        break
      case "river":
        onTreeConfigChange({ treeAggressionCapRiver: value })
        break
    }
  }

  return (
    <Card>
      <CardHeader className="border-b">
        <h2 className="font-heading text-sm font-medium">ベッティングツリー</h2>
        <CardDescription>
          公開stateへ適用するtyped
          ruleを上から作成します。同じpriorityは表示順で適用されます。
        </CardDescription>
        <CardAction>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={disabled || rules.length >= MAX_TREE_RULES}
            onClick={addRule}
          >
            <IconPlus />
            ルールを追加
          </Button>
        </CardAction>
      </CardHeader>

      <CardContent className="space-y-3">
        <div className="grid items-end gap-3 rounded-lg border bg-muted/20 p-3 md:grid-cols-[240px_1fr]">
          <div className="field-stack">
            <Label>Tree frontend</Label>
            <Select
              value={treeKind}
              disabled={disabled}
              onValueChange={(value) =>
                onTreeConfigChange({
                  treeKind: value as "standard" | "script",
                })
              }
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="standard">Standard typed rules</SelectItem>
                <SelectItem value="script">.mwtree script</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <p className="text-xs text-muted-foreground">
            Standardは下のBuilder/ruleを使用。Scriptは選択した.mwtreeとscalar
            parameterをRust側でtyped ruleへ展開します。
          </p>
        </div>

        <div className="space-y-3 rounded-lg border p-3">
          <div>
            <Label>Production tree controls</Label>
            <p className="field-help">
              StandardとScriptに共通で適用され、effective
              configとfingerprintへ保持されます。
            </p>
          </div>
          <div className="grid gap-3 md:grid-cols-3">
            <div className="field-stack">
              <Label>Allow limp</Label>
              <Select
                value={allowLimp}
                disabled={disabled}
                onValueChange={(value) =>
                  onTreeConfigChange({
                    treeAllowLimp: value as "default" | "true" | "false",
                  })
                }
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="default">Generic default</SelectItem>
                  <SelectItem value="true">Allow</SelectItem>
                  <SelectItem value="false">Disallow</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="flex items-center justify-between gap-3 rounded-md border p-3">
              <div>
                <Label>Street aggression caps</Label>
                <p className="field-help">4 streetをまとめて指定</p>
              </div>
              <Switch
                checked={aggressionCapsEnabled}
                disabled={disabled}
                onCheckedChange={(checked) =>
                  onTreeConfigChange({
                    treeAggressionCapsEnabled: checked,
                  })
                }
              />
            </div>
            <div className="flex items-center justify-between gap-3 rounded-md border p-3">
              <div>
                <Label>Re-raise jam ratio</Label>
                <p className="field-help">actor開始stack比</p>
              </div>
              <Switch
                checked={reraiseJamEnabled}
                disabled={disabled}
                onCheckedChange={(checked) =>
                  onTreeConfigChange({
                    treeReraiseJamEnabled: checked,
                  })
                }
              />
            </div>
          </div>
          {aggressionCapsEnabled ? (
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
              {(
                [
                  ["preflop", aggressionCaps.preflop],
                  ["flop", aggressionCaps.flop],
                  ["turn", aggressionCaps.turn],
                  ["river", aggressionCaps.river],
                ] as const
              ).map(([street, value]) => (
                <div className="field-stack" key={street}>
                  <Label htmlFor={`tree-cap-${street}`}>
                    {street[0]?.toUpperCase()}
                    {street.slice(1)}
                  </Label>
                  <Input
                    id={`tree-cap-${street}`}
                    value={value}
                    inputMode="numeric"
                    disabled={disabled}
                    onChange={(event) =>
                      patchAggressionCap(street, event.target.value)
                    }
                  />
                </div>
              ))}
            </div>
          ) : null}
          {reraiseJamEnabled ? (
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="field-stack">
                <Label htmlFor="tree-jam-numerator">Numerator</Label>
                <Input
                  id="tree-jam-numerator"
                  value={reraiseJamNumerator}
                  inputMode="numeric"
                  disabled={disabled}
                  onChange={(event) =>
                    onTreeConfigChange({
                      treeReraiseJamNumerator: event.target.value,
                    })
                  }
                />
              </div>
              <div className="field-stack">
                <Label htmlFor="tree-jam-denominator">Denominator</Label>
                <Input
                  id="tree-jam-denominator"
                  value={reraiseJamDenominator}
                  inputMode="numeric"
                  disabled={disabled}
                  onChange={(event) =>
                    onTreeConfigChange({
                      treeReraiseJamDenominator: event.target.value,
                    })
                  }
                />
              </div>
            </div>
          ) : null}
        </div>

        {treeKind === "script" ? (
          <div className="space-y-3 rounded-lg border p-3">
            <div className="grid items-end gap-3 md:grid-cols-[1fr_auto]">
              <div className="field-stack">
                <Label htmlFor="tree-script-source">Script source</Label>
                <Input
                  id="tree-script-source"
                  value={scriptSource}
                  readOnly
                  placeholder=".mwtreeファイルを選択"
                />
              </div>
              <Button
                type="button"
                variant="outline"
                disabled={disabled}
                onClick={onPickScript}
              >
                <IconFileCode />
                .mwtreeを選択
              </Button>
            </div>
            <div className="flex items-center justify-between gap-3">
              <div>
                <Label>Script parameters</Label>
                <p className="field-help">
                  string / integer / finite float / booleanのみ
                </p>
              </div>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={disabled}
                onClick={() =>
                  onTreeConfigChange({
                    treeScriptParams: [
                      ...scriptParams,
                      createTreeScriptParamDraft(),
                    ],
                  })
                }
              >
                <IconPlus />
                Parameter
              </Button>
            </div>
            {scriptParams.map((param, index) => (
              <div
                className="grid gap-2 md:grid-cols-[minmax(120px,0.8fr)_150px_minmax(160px,1fr)_auto]"
                key={param.id}
              >
                <Input
                  value={param.key}
                  aria-label={`Script parameter ${index + 1} name`}
                  placeholder="open"
                  onChange={(event) =>
                    patchScriptParam(index, { key: event.target.value })
                  }
                />
                <Select
                  value={param.type}
                  onValueChange={(type) =>
                    patchScriptParam(index, {
                      type: type as TreeScriptParamDraft["type"],
                      value:
                        type === "boolean" &&
                        param.value !== "true" &&
                        param.value !== "false"
                          ? "false"
                          : param.value,
                    })
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="string">String</SelectItem>
                    <SelectItem value="integer">Integer</SelectItem>
                    <SelectItem value="float">Float</SelectItem>
                    <SelectItem value="boolean">Boolean</SelectItem>
                  </SelectContent>
                </Select>
                {param.type === "boolean" ? (
                  <Select
                    value={param.value}
                    onValueChange={(value) => {
                      if (value !== null) {
                        patchScriptParam(index, { value })
                      }
                    }}
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="true">true</SelectItem>
                      <SelectItem value="false">false</SelectItem>
                    </SelectContent>
                  </Select>
                ) : (
                  <Input
                    value={param.value}
                    className="font-mono"
                    aria-label={`Script parameter ${index + 1} value`}
                    onChange={(event) =>
                      patchScriptParam(index, { value: event.target.value })
                    }
                  />
                )}
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"
                  aria-label={`Script parameter ${index + 1}を削除`}
                  disabled={disabled}
                  onClick={() =>
                    onTreeConfigChange({
                      treeScriptParams: scriptParams.filter(
                        (_, paramIndex) => paramIndex !== index
                      ),
                    })
                  }
                >
                  <IconTrash />
                </Button>
              </div>
            ))}
          </div>
        ) : (
          <>
            <div className="rounded-lg border bg-muted/20 p-3">
              <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
                <div>
                  <p className="font-medium">Preflop Tree Builder</p>
                  <p className="text-xs text-muted-foreground">
                    open・limp後raise・re-raiseとaggression上限からtyped
                    ruleを生成します。
                  </p>
                </div>
                <Badge variant="outline">Preflop preset</Badge>
              </div>
              <div className="grid gap-3 md:grid-cols-3">
                {(
                  [
                    ["unopenedSizes", "Unopened open", "2.5x\nallin"],
                    ["limpedSizes", "Raise after limp", "2.5x\nallin"],
                    ["reraiseSizes", "Re-raise", "3x\nallin"],
                  ] as const
                ).map(([field, label, placeholder]) => (
                  <div className="field-stack" key={field}>
                    <Label htmlFor={`preflop-builder-${field}`}>{label}</Label>
                    <Textarea
                      id={`preflop-builder-${field}`}
                      className="min-h-16 resize-y font-mono text-xs"
                      value={preflopBuilder[field].join("\n")}
                      placeholder={placeholder}
                      spellCheck={false}
                      disabled={disabled}
                      onChange={(event) =>
                        updateBuilder({
                          [field]: event.target.value
                            .split(/\r?\n/)
                            .map((value) => value.trim())
                            .filter(Boolean),
                        })
                      }
                    />
                    <p className="field-help">1行に1 size token</p>
                  </div>
                ))}
              </div>
              <div className="mt-3 grid items-end gap-3 md:grid-cols-[180px_240px_1fr]">
                <div className="field-stack">
                  <Label htmlFor="preflop-builder-cap">
                    Max aggressive actions
                  </Label>
                  <Input
                    id="preflop-builder-cap"
                    inputMode="numeric"
                    value={preflopBuilder.maxAggressions}
                    disabled={disabled}
                    onChange={(event) =>
                      updateBuilder({ maxAggressions: event.target.value })
                    }
                  />
                </div>
                <div className="field-stack">
                  <Label>After Preflop</Label>
                  <Select
                    value={preflopBuilder.postflop}
                    disabled={disabled}
                    onValueChange={(postflop) =>
                      updateBuilder({
                        postflop:
                          postflop as PreflopTreeBuilderDraft["postflop"],
                      })
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="checkdown">
                        Checkdown（Preflop only）
                      </SelectItem>
                      <SelectItem value="full-tree">
                        Keep current / Standard postflop
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <Button
                  type="button"
                  className="justify-self-start md:justify-self-end"
                  disabled={disabled}
                  onClick={applyPreflopBuilder}
                >
                  <IconGitBranch />
                  Treeへ適用
                </Button>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                適用するとPreflop ruleを置き換えます。Standard
                postflopを選んだ場合、既存のPostflop詳細ruleは保持します。
              </p>
            </div>

            <div className="rounded-lg border bg-muted/20 p-3">
              <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
                <div>
                  <p className="font-medium">Postflop Tree Builder</p>
                  <p className="text-xs text-muted-foreground">
                    Flop・Turn・Riverごとのbet/raise
                    sizeとstreet内aggression上限を設定します。
                  </p>
                </div>
                <Badge variant="outline">All streets</Badge>
              </div>
              <div className="grid grid-cols-[72px_minmax(0,1fr)_minmax(0,1fr)] gap-2">
                <span />
                <Label>Bet sizes</Label>
                <Label>Raise sizes</Label>
                {(["flop", "turn", "river"] as const).map((street) => (
                  <div className="contents" key={street}>
                    <Label
                      className="self-center capitalize"
                      htmlFor={`postflop-${street}-bet`}
                    >
                      {street}
                    </Label>
                    <Textarea
                      id={`postflop-${street}-bet`}
                      className="min-h-14 resize-y font-mono text-xs"
                      value={postflopBuilder[street].betSizes.join("\n")}
                      spellCheck={false}
                      disabled={disabled}
                      aria-label={`${street} bet sizes`}
                      onChange={(event) =>
                        updatePostflopStreet(
                          street,
                          "betSizes",
                          event.target.value
                            .split(/\r?\n/)
                            .map((value) => value.trim())
                            .filter(Boolean)
                        )
                      }
                    />
                    <Textarea
                      id={`postflop-${street}-raise`}
                      className="min-h-14 resize-y font-mono text-xs"
                      value={postflopBuilder[street].raiseSizes.join("\n")}
                      spellCheck={false}
                      disabled={disabled}
                      aria-label={`${street} raise sizes`}
                      onChange={(event) =>
                        updatePostflopStreet(
                          street,
                          "raiseSizes",
                          event.target.value
                            .split(/\r?\n/)
                            .map((value) => value.trim())
                            .filter(Boolean)
                        )
                      }
                    />
                  </div>
                ))}
              </div>
              <div className="mt-3 flex flex-wrap items-end justify-between gap-3">
                <div className="field-stack w-52">
                  <Label htmlFor="postflop-builder-cap">
                    Max aggressive actions / street
                  </Label>
                  <Input
                    id="postflop-builder-cap"
                    inputMode="numeric"
                    value={postflopBuilder.maxAggressions}
                    disabled={disabled}
                    onChange={(event) => {
                      setPostflopBuilder((current) => ({
                        ...current,
                        maxAggressions: event.target.value,
                      }))
                      setBuilderError(null)
                    }}
                  />
                </div>
                <Button
                  type="button"
                  disabled={disabled}
                  onClick={applyPostflopBuilder}
                >
                  <IconGitBranch />
                  Postflop Treeへ適用
                </Button>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                適用するとPreflop
                ruleを残し、既存のFlop以降のruleを置き換えます。
              </p>
              {builderError ? (
                <p className="mt-2 text-xs text-destructive">{builderError}</p>
              ) : null}
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <Badge variant={rules.length > 0 ? "secondary" : "outline"}>
                {rules.length > 0
                  ? `カスタム ${rules.length} rules`
                  : "組み込み標準"}
              </Badge>
              <span className="text-xs text-muted-foreground">
                最大 {MAX_TREE_RULES} rules
              </span>
            </div>

            {rules.length === 0 ? (
              <Alert>
                <IconGitBranch />
                <AlertTitle>組み込み標準ツリーを使用します</AlertTitle>
                <AlertDescription>
                  ruleを追加しない場合は、Solverの標準open・raise・postflop
                  sizingとaggression capが使われます。
                </AlertDescription>
              </Alert>
            ) : (
              <Accordion
                multiple
                value={openRuleIds}
                onValueChange={setOpenRuleIds}
                disabled={disabled}
              >
                {rules.map((rule, index) => {
                  const errors = ruleErrors(rule)
                  const hasErrors = Object.values(errors).some(Boolean)
                  const idPrefix = `tree-rule-${rule.id}`
                  const canHaveSizes =
                    rule.effect !== "remove" &&
                    (rule.action === "bet" || rule.action === "raise")

                  return (
                    <AccordionItem value={rule.id} key={rule.id}>
                      <AccordionTrigger className="px-3 py-3 hover:no-underline">
                        <span className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
                          <span className="font-mono text-[0.65rem] text-muted-foreground">
                            #{index + 1} · P{rule.priority || "?"}
                          </span>
                          <Badge variant="outline">{rule.street}</Badge>
                          <span className="truncate">{ruleSummary(rule)}</span>
                          <span className="min-w-0 truncate font-normal text-muted-foreground">
                            when {rule.condition.trim() || "未入力"}
                          </span>
                          {hasErrors ? (
                            <Badge variant="destructive">要修正</Badge>
                          ) : null}
                        </span>
                      </AccordionTrigger>

                      <AccordionContent className="space-y-4 px-1">
                        <div className="grid gap-3 sm:grid-cols-2">
                          <div className="field-stack">
                            <Label htmlFor={`${idPrefix}-priority`}>
                              Priority
                            </Label>
                            <Input
                              id={`${idPrefix}-priority`}
                              value={rule.priority}
                              aria-invalid={Boolean(errors.priority)}
                              aria-describedby={`${idPrefix}-priority-help`}
                              onChange={(event) =>
                                patchRule(index, {
                                  priority: event.target.value,
                                })
                              }
                            />
                            <p
                              id={`${idPrefix}-priority-help`}
                              className={
                                errors.priority
                                  ? "text-xs text-destructive"
                                  : "field-help"
                              }
                            >
                              {errors.priority ??
                                "小さい値から適用。同値は表示順です。"}
                            </p>
                          </div>

                          <div className="field-stack">
                            <Label htmlFor={`${idPrefix}-street`}>Street</Label>
                            <Select
                              value={rule.street}
                              onValueChange={(street) =>
                                patchRule(index, {
                                  street: street as TreeRuleStreet,
                                })
                              }
                            >
                              <SelectTrigger
                                id={`${idPrefix}-street`}
                                className="w-full"
                              >
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                {streetOptions.map((option) => (
                                  <SelectItem
                                    value={option.value}
                                    key={option.value}
                                  >
                                    {option.label}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                          </div>

                          <div className="field-stack">
                            <Label htmlFor={`${idPrefix}-effect`}>Effect</Label>
                            <Select
                              value={rule.effect}
                              onValueChange={(effect) =>
                                changeEffect(index, effect as TreeRuleEffect)
                              }
                            >
                              <SelectTrigger
                                id={`${idPrefix}-effect`}
                                className="w-full"
                              >
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                {effectOptions.map((option) => (
                                  <SelectItem
                                    value={option.value}
                                    key={option.value}
                                  >
                                    {option.label}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                          </div>

                          {rule.effect !== "checkdown" ? (
                            <div className="field-stack">
                              <Label htmlFor={`${idPrefix}-action`}>
                                Action
                              </Label>
                              <Select
                                value={rule.action ?? undefined}
                                onValueChange={(action) =>
                                  changeAction(index, action as TreeRuleAction)
                                }
                              >
                                <SelectTrigger
                                  id={`${idPrefix}-action`}
                                  className="w-full"
                                  aria-invalid={Boolean(errors.action)}
                                >
                                  <SelectValue placeholder="Actionを選択" />
                                </SelectTrigger>
                                <SelectContent>
                                  {actionOptions.map((option) => (
                                    <SelectItem
                                      value={option.value}
                                      key={option.value}
                                    >
                                      {option.label}
                                    </SelectItem>
                                  ))}
                                </SelectContent>
                              </Select>
                              {errors.action ? (
                                <p className="text-xs text-destructive">
                                  {errors.action}
                                </p>
                              ) : null}
                            </div>
                          ) : (
                            <Alert>
                              <IconInfoCircle />
                              <AlertTitle>Checkdown</AlertTitle>
                              <AlertDescription>
                                actionとsizesを持たず、条件に一致したstreetをcheckのみにします。
                              </AlertDescription>
                            </Alert>
                          )}
                        </div>

                        <div className="field-stack">
                          <Label htmlFor={`${idPrefix}-condition`}>
                            When condition
                          </Label>
                          <Textarea
                            id={`${idPrefix}-condition`}
                            className="min-h-20 font-mono text-xs"
                            value={rule.condition}
                            spellCheck={false}
                            aria-invalid={Boolean(errors.condition)}
                            aria-describedby={`${idPrefix}-condition-help`}
                            onChange={(event) =>
                              patchRule(index, {
                                condition: event.target.value,
                              })
                            }
                          />
                          <p
                            id={`${idPrefix}-condition-help`}
                            className={
                              errors.condition
                                ? "text-xs text-destructive"
                                : "field-help"
                            }
                          >
                            {errors.condition ??
                              '例: unopened && position in ["CO", "BTN"]。position, in_position, players, limpers, flats, aggressions, squeeze, cbet, donk, sprを使用できます。'}
                          </p>
                        </div>

                        {rule.effect === "remove" ? (
                          <Alert>
                            <IconInfoCircle />
                            <AlertTitle>Removeはaction単位です</AlertTitle>
                            <AlertDescription>
                              条件に一致する選択済みaction種別をすべて除外します。個別sizeは指定しません。
                            </AlertDescription>
                          </Alert>
                        ) : canHaveSizes ? (
                          <div className="space-y-2">
                            <div className="flex flex-wrap items-center justify-between gap-2">
                              <div>
                                <Label>Sizes</Label>
                                <p className="field-help">
                                  1欄に1
                                  token。順序を保ったままTOMLへ出力します。
                                </p>
                              </div>
                              <Button
                                type="button"
                                size="sm"
                                variant="outline"
                                disabled={
                                  disabled ||
                                  rule.sizes.length >= MAX_TREE_RULE_SIZES
                                }
                                onClick={() => addSize(index)}
                              >
                                <IconPlus />
                                Sizeを追加
                              </Button>
                            </div>
                            {rule.sizes.map((size, sizeIndex) => (
                              <div
                                className="flex min-w-0 items-start gap-2"
                                key={`${rule.id}-size-${sizeIndex}`}
                              >
                                <div className="field-stack min-w-0 flex-1">
                                  <Label
                                    className="sr-only"
                                    htmlFor={`${idPrefix}-size-${sizeIndex}`}
                                  >
                                    Size {sizeIndex + 1}
                                  </Label>
                                  <Input
                                    id={`${idPrefix}-size-${sizeIndex}`}
                                    className="font-mono"
                                    value={size}
                                    aria-label={`Size ${sizeIndex + 1}`}
                                    aria-invalid={!size.trim()}
                                    placeholder="2.5x / 50%pot / allin"
                                    onChange={(event) =>
                                      updateSize(
                                        index,
                                        sizeIndex,
                                        event.target.value
                                      )
                                    }
                                  />
                                </div>
                                <Button
                                  type="button"
                                  size="icon"
                                  variant="ghost"
                                  aria-label={`Size ${sizeIndex + 1}を削除`}
                                  title={`Size ${sizeIndex + 1}を削除`}
                                  onClick={() => removeSize(index, sizeIndex)}
                                >
                                  <IconTrash />
                                </Button>
                              </div>
                            ))}
                            {rule.sizes.length === 0 ? (
                              <p className="rounded-md border border-dashed p-3 text-xs text-muted-foreground">
                                sizeは未指定です。「Sizeを追加」からbet/raise
                                targetを追加してください。
                              </p>
                            ) : null}
                            {errors.sizes ? (
                              <p className="text-xs text-destructive">
                                {errors.sizes}
                              </p>
                            ) : null}
                          </div>
                        ) : null}

                        <div className="flex flex-wrap items-center gap-2 border-t pt-3">
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            disabled={disabled || index === 0}
                            onClick={() => moveRule(index, -1)}
                          >
                            <IconArrowUp />
                            上へ
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            disabled={disabled || index === rules.length - 1}
                            onClick={() => moveRule(index, 1)}
                          >
                            <IconArrowDown />
                            下へ
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            disabled={
                              disabled || rules.length >= MAX_TREE_RULES
                            }
                            onClick={() => duplicateRule(index)}
                          >
                            <IconCopy />
                            複製
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            variant="destructive"
                            className="sm:ml-auto"
                            disabled={disabled}
                            onClick={() => removeRule(index)}
                          >
                            <IconTrash />
                            削除
                          </Button>
                        </div>
                      </AccordionContent>
                    </AccordionItem>
                  )
                })}
              </Accordion>
            )}
          </>
        )}
      </CardContent>
    </Card>
  )
}
