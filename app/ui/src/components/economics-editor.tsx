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
  type EconomicsKind,
  type FormSolveDraft,
  summarizeDecimalTokenList,
} from "@/lib/setup-config"

type EconomicsEditorProps = {
  draft: FormSolveDraft
  onChange: (draft: FormSolveDraft) => void
}

export function EconomicsEditor({ draft, onChange }: EconomicsEditorProps) {
  const payoutSummary = summarizeDecimalTokenList(
    draft.economics.tournamentIcm.payouts
  )
  const outsideSummary = summarizeDecimalTokenList(
    draft.economics.tournamentIcm.outsideFieldBb
  )
  const payoutTokenError =
    payoutSummary.tokens.length === 0
      ? "賞金を1件以上入力してください。"
      : payoutSummary.valid
        ? null
        : "賞金は0以上の有限10進数で入力してください。"
  const outsideError = outsideSummary.valid
    ? null
    : "卓外stackは正の有限10進数で入力してください。"
  const outsideHasZero =
    outsideSummary.valid &&
    outsideSummary.tokens.some((token) => /^0(?:\.0+)?$/.test(token))
  const parsedFieldPlayers = draft.seatCount + outsideSummary.tokens.length
  const effectiveOutsideError =
    (outsideHasZero ? "卓外stackに0は指定できません。" : outsideError) ??
    (parsedFieldPlayers > 10_000
      ? "ICM fieldは卓内と卓外を合わせて10,000人以下にしてください。"
      : null)
  const fieldPlayers =
    effectiveOutsideError === null ? parsedFieldPlayers : null
  const payoutsError =
    payoutTokenError ??
    (fieldPlayers !== null && payoutSummary.tokens.length > fieldPlayers
      ? `賞金はfield人数（${fieldPlayers}人）以下の件数にしてください。`
      : null)
  const icmInputError = payoutsError ?? effectiveOutsideError
  const sampledByCount = parsedFieldPlayers > 15
  const sampled = fieldPlayers !== null && fieldPlayers > 15

  const setKind = (kind: EconomicsKind) => {
    if (kind === draft.economics.kind) {
      return
    }
    onChange({
      ...draft,
      stopTarget: "default",
      economics: { ...draft.economics, kind },
    })
  }

  const updateCash = (update: Partial<FormSolveDraft["economics"]["cash"]>) => {
    onChange({
      ...draft,
      economics: {
        ...draft.economics,
        cash: { ...draft.economics.cash, ...update },
      },
    })
  }

  const updateIcm = (
    update: Partial<FormSolveDraft["economics"]["tournamentIcm"]>
  ) => {
    onChange({
      ...draft,
      economics: {
        ...draft.economics,
        tournamentIcm: {
          ...draft.economics.tournamentIcm,
          ...update,
        },
      },
    })
  }

  return (
    <Card size="sm">
      <CardHeader className="border-b">
        <h2 className="font-heading text-sm font-medium">経済モデル</h2>
        <CardDescription>
          CashのchipEV/rake、またはTournament ICMの賞金・卓外fieldを設定します。
        </CardDescription>
        <CardAction>
          <Badge variant="secondary">
            {draft.economics.kind === "cash" ? "ChipEV" : "ICM"}
          </Badge>
        </CardAction>
      </CardHeader>
      <CardContent className="space-y-4">
        <div
          className="economics-mode-switch"
          role="group"
          aria-label="経済モデル"
        >
          <Button
            type="button"
            size="sm"
            variant={draft.economics.kind === "cash" ? "default" : "outline"}
            aria-pressed={draft.economics.kind === "cash"}
            onClick={() => setKind("cash")}
          >
            Cash / ChipEV
          </Button>
          <Button
            type="button"
            size="sm"
            variant={
              draft.economics.kind === "tournament-icm" ? "default" : "outline"
            }
            aria-pressed={draft.economics.kind === "tournament-icm"}
            onClick={() => setKind("tournament-icm")}
          >
            Tournament / ICM
          </Button>
        </div>

        {draft.economics.kind === "cash" ? (
          <div className="economics-cash-grid">
            <div className="flex items-center justify-between gap-3 rounded-lg border p-3">
              <div>
                <p className="font-medium">Rake</p>
                <p className="text-xs text-muted-foreground">
                  無効時はrake tableを出力しません
                </p>
              </div>
              <Switch
                checked={draft.economics.cash.rakeEnabled}
                onCheckedChange={(rakeEnabled) => updateCash({ rakeEnabled })}
                aria-label="Rakeを有効化"
              />
            </div>
            {draft.economics.cash.rakeEnabled ? (
              <>
                <div className="field-stack">
                  <Label htmlFor="rake-rate">Rate (0..1)</Label>
                  <Input
                    id="rake-rate"
                    inputMode="decimal"
                    value={draft.economics.cash.rakeRate}
                    onChange={(event) =>
                      updateCash({ rakeRate: event.target.value })
                    }
                  />
                </div>
                <div className="field-stack rounded-lg border p-2.5">
                  <div className="flex items-center justify-between gap-2">
                    <Label htmlFor="rake-cap-enabled">Cap</Label>
                    <Switch
                      id="rake-cap-enabled"
                      checked={draft.economics.cash.rakeCapEnabled}
                      onCheckedChange={(rakeCapEnabled) =>
                        updateCash({ rakeCapEnabled })
                      }
                    />
                  </div>
                  {draft.economics.cash.rakeCapEnabled ? (
                    <Input
                      id="rake-cap"
                      inputMode="decimal"
                      aria-label="Rake cap BB"
                      value={draft.economics.cash.rakeCapBb}
                      onChange={(event) =>
                        updateCash({ rakeCapBb: event.target.value })
                      }
                    />
                  ) : (
                    <p className="field-help">Uncapped rake</p>
                  )}
                </div>
                <div className="field-stack md:col-span-2">
                  <Label htmlFor="rake-when">Rake when</Label>
                  <Input
                    id="rake-when"
                    className="font-mono"
                    value={draft.economics.cash.rakeWhen}
                    onChange={(event) =>
                      updateCash({ rakeWhen: event.target.value })
                    }
                  />
                  <p className="field-help">
                    例: flop_dealt / showdown / players_saw_flop &gt;= 3
                  </p>
                </div>
                <div className="field-stack">
                  <Label>Allocation</Label>
                  <Select
                    value={draft.economics.cash.rakeAllocation}
                    onValueChange={(rakeAllocation) =>
                      updateCash({
                        rakeAllocation:
                          rakeAllocation as FormSolveDraft["economics"]["cash"]["rakeAllocation"],
                      })
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="main-first">Main pot first</SelectItem>
                      <SelectItem value="proportional">Proportional</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <div className="field-stack">
                  <Label>Rounding</Label>
                  <Select
                    value={draft.economics.cash.rakeRounding}
                    onValueChange={(rakeRounding) =>
                      updateCash({
                        rakeRounding:
                          rakeRounding as FormSolveDraft["economics"]["cash"]["rakeRounding"],
                      })
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="down">Down</SelectItem>
                      <SelectItem value="nearest">Nearest</SelectItem>
                      <SelectItem value="up">Up</SelectItem>
                    </SelectContent>
                  </Select>
                  <p className="field-help">Unitはv1固定の0.001 BB</p>
                </div>
              </>
            ) : (
              <div className="economics-empty-note">
                PotはrakeなしのchipEVで評価されます。
              </div>
            )}
          </div>
        ) : (
          <div className="space-y-4">
            <div className="icm-summary-strip">
              <div>
                <span>Table</span>
                <strong>{draft.seatCount}</strong>
              </div>
              <div>
                <span>Outside</span>
                <strong>
                  {effectiveOutsideError ? "—" : outsideSummary.tokens.length}
                </strong>
              </div>
              <div>
                <span>Field</span>
                <strong>{fieldPlayers ?? "—"}</strong>
              </div>
              <div>
                <span>Paid places</span>
                <strong>{payoutsError ? "—" : payoutSummary.paidPlaces}</strong>
              </div>
              <div>
                <span>Total prize</span>
                <strong>{payoutsError ? "—" : payoutSummary.total}</strong>
              </div>
              <Badge
                variant={
                  icmInputError
                    ? "destructive"
                    : sampled
                      ? "secondary"
                      : "outline"
                }
              >
                {icmInputError
                  ? "要修正"
                  : sampled
                    ? "Deterministic MC"
                    : "Exact ICM"}
              </Badge>
            </div>

            <div className="icm-editor-grid">
              <div className="field-stack">
                <Label htmlFor="icm-payouts">Payouts</Label>
                <Textarea
                  id="icm-payouts"
                  className="icm-bulk-input font-mono text-xs"
                  value={draft.economics.tournamentIcm.payouts}
                  onChange={(event) =>
                    updateIcm({ payouts: event.target.value })
                  }
                  placeholder={"1000\n600\n400"}
                  spellCheck={false}
                  aria-invalid={payoutsError !== null}
                  aria-describedby="icm-payouts-help"
                />
                <p
                  id="icm-payouts-help"
                  className={
                    payoutsError ? "field-help text-destructive" : "field-help"
                  }
                >
                  {payoutsError ??
                    "1位から順に改行・空白・カンマ区切りで貼り付けます。末尾の0は有賞順位に数えません。"}
                </p>
              </div>
              <div className="field-stack">
                <Label htmlFor="icm-outside-stacks">
                  Outside field stacks (BB)
                </Label>
                <Textarea
                  id="icm-outside-stacks"
                  className="icm-bulk-input font-mono text-xs"
                  value={draft.economics.tournamentIcm.outsideFieldBb}
                  onChange={(event) =>
                    updateIcm({ outsideFieldBb: event.target.value })
                  }
                  placeholder={"18\n26\n11.5"}
                  spellCheck={false}
                  aria-invalid={effectiveOutsideError !== null}
                  aria-describedby="icm-outside-stacks-help"
                />
                <p
                  id="icm-outside-stacks-help"
                  className={
                    effectiveOutsideError
                      ? "field-help text-destructive"
                      : "field-help"
                  }
                >
                  {effectiveOutsideError ??
                    "卓外の各playerを1値ずつ入力します。卓内stackは上のtableを使います。"}
                </p>
              </div>
            </div>

            {sampledByCount ? (
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="field-stack">
                  <Label htmlFor="icm-samples">ICM samples</Label>
                  <Input
                    id="icm-samples"
                    inputMode="numeric"
                    value={draft.economics.tournamentIcm.samples}
                    onChange={(event) =>
                      updateIcm({ samples: event.target.value })
                    }
                  />
                </div>
                <div className="field-stack">
                  <Label htmlFor="icm-seed">ICM seed</Label>
                  <Input
                    id="icm-seed"
                    inputMode="numeric"
                    value={draft.economics.tournamentIcm.seed}
                    onChange={(event) =>
                      updateIcm({ seed: event.target.value })
                    }
                  />
                </div>
              </div>
            ) : (
              <p className="icm-mode-note">
                15人以下はexact
                ICMです。samplesとseedは設定ファイルへ出力しません。
              </p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  )
}
