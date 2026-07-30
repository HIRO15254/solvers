import { Badge } from "@/components/ui/badge"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Table,
  TableBody,
  TableCaption,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  type FormSolveDraft,
  type SeatDraft,
  withButton,
  withSeatCount,
  withStandardBlindsSetting,
} from "@/lib/setup-config"

const positions: Record<number, string[]> = {
  2: ["BTN / SB", "BB"],
  3: ["BTN", "SB", "BB"],
  4: ["BTN", "SB", "BB", "CO"],
  5: ["BTN", "SB", "BB", "HJ", "CO"],
  6: ["BTN", "SB", "BB", "UTG", "HJ", "CO"],
  7: ["BTN", "SB", "BB", "UTG", "LJ", "HJ", "CO"],
  8: ["BTN", "SB", "BB", "UTG", "UTG1", "LJ", "HJ", "CO"],
  9: ["BTN", "SB", "BB", "UTG", "UTG1", "UTG2", "LJ", "HJ", "CO"],
}

function positionForSeat(seatCount: number, button: number, seat: number) {
  const offset = (seat - button + seatCount) % seatCount
  return positions[seatCount]?.[offset] ?? `Seat ${seat}`
}

type TableRangeEditorProps = {
  draft: FormSolveDraft
  onChange: (draft: FormSolveDraft) => void
}

export function TableRangeEditor({ draft, onChange }: TableRangeEditorProps) {
  const updateSeat = (
    seatIndex: number,
    field: keyof SeatDraft,
    value: string
  ) => {
    onChange({
      ...draft,
      seats: draft.seats.map((seat, index) =>
        index === seatIndex ? { ...seat, [field]: value } : seat
      ),
    })
  }

  return (
    <Card size="sm">
      <CardHeader className="border-b">
        <h2 className="font-heading text-sm font-medium">
          テーブル・スタートレンジ
        </h2>
        <CardDescription>
          全席を同時に編集します。Positionとstandard blindはButtonに追従します。
        </CardDescription>
        <CardAction>
          <Badge variant="secondary">{draft.seatCount} players</Badge>
        </CardAction>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="lineup-toolbar">
          <div className="field-stack">
            <Label htmlFor="seat-count">Players</Label>
            <Select
              value={String(draft.seatCount)}
              onValueChange={(value) =>
                onChange(withSeatCount(draft, Number(value)))
              }
            >
              <SelectTrigger id="seat-count" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Array.from({ length: 8 }, (_, index) => index + 2).map(
                  (count) => (
                    <SelectItem value={String(count)} key={count}>
                      {count} seats
                    </SelectItem>
                  )
                )}
              </SelectContent>
            </Select>
          </div>
          <div className="field-stack">
            <Label htmlFor="button-seat">Button</Label>
            <Select
              value={String(draft.button)}
              onValueChange={(value) =>
                onChange(withButton(draft, Number(value)))
              }
            >
              <SelectTrigger id="button-seat" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {draft.seats.map((_, seat) => (
                  <SelectItem value={String(seat)} key={seat}>
                    Seat {seat}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="field-stack">
            <Label htmlFor="common-ante">Common ante (BB)</Label>
            <Input
              id="common-ante"
              inputMode="decimal"
              value={draft.commonAnteBb}
              onChange={(event) =>
                onChange({ ...draft, commonAnteBb: event.target.value })
              }
            />
          </div>
          <div className="lineup-toolbar-note">
            <span>Blind preset</span>
            <div className="flex items-center gap-2">
              <Switch
                checked={draft.standardBlinds}
                onCheckedChange={(value) =>
                  onChange(withStandardBlindsSetting(draft, value))
                }
                aria-label="Standard blinds"
              />
              <strong>{draft.standardBlinds ? "Standard" : "Manual"}</strong>
            </div>
            <small>
              {draft.standardBlinds
                ? "Button変更時にSB / BBを再配置"
                : "全live blindをSeat列で明示"}
            </small>
          </div>
        </div>

        <div className="grid gap-3 rounded-lg border bg-muted/20 p-3 md:grid-cols-3">
          <div className="field-stack">
            <Label htmlFor="preflop-first-actor">Preflop first actor</Label>
            <Select
              value={draft.preflopFirstToAct}
              onValueChange={(preflopFirstToAct) => {
                if (preflopFirstToAct !== null) {
                  onChange({ ...draft, preflopFirstToAct })
                }
              }}
            >
              <SelectTrigger id="preflop-first-actor" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="utg">UTG（BBの次）</SelectItem>
                {draft.seats.map((_, seat) => (
                  <SelectItem value={String(seat)} key={seat}>
                    Seat {seat}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="field-stack">
            <Label htmlFor="default-stack">Default stack (BB)</Label>
            <Input
              id="default-stack"
              inputMode="decimal"
              value={draft.defaultStackBb}
              onChange={(event) => {
                const defaultStackBb = event.target.value
                onChange({
                  ...draft,
                  defaultStackBb,
                  seats: draft.seats.map((seat) => ({
                    ...seat,
                    stackBb: defaultStackBb,
                  })),
                })
              }}
            />
            <p className="field-help">
              変更時は全Seatへ適用。その後個別変更できます。
            </p>
          </div>
          <div className="field-stack">
            <Label htmlFor="default-range">Default range</Label>
            <Input
              id="default-range"
              className="font-mono"
              value={draft.defaultRange}
              spellCheck={false}
              onChange={(event) => {
                const defaultRange = event.target.value
                onChange({
                  ...draft,
                  defaultRange,
                  seats: draft.seats.map((seat) => ({
                    ...seat,
                    range: defaultRange,
                  })),
                })
              }}
            />
            <p className="field-help">
              変更時は全Seatへ適用。その後個別変更できます。
            </p>
          </div>
        </div>

        <Table className="min-w-[690px] table-fixed">
          <TableCaption className="sr-only">
            各SeatのPosition、Stack、Blind、Ante、スタートレンジ
          </TableCaption>
          <TableHeader>
            <TableRow>
              <TableHead className="h-8 w-16 px-1.5">Seat</TableHead>
              <TableHead className="h-8 w-20 px-1.5">Position</TableHead>
              <TableHead className="h-8 w-24 px-1.5 text-right">
                Stack
              </TableHead>
              <TableHead className="h-8 w-24 px-1.5 text-right">
                Blind
              </TableHead>
              <TableHead className="h-8 w-24 px-1.5 text-right">Ante</TableHead>
              <TableHead className="h-8 px-1.5">Range expression</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {draft.seats.map((seat, index) => {
              const position = positionForSeat(
                draft.seatCount,
                draft.button,
                index
              )
              return (
                <TableRow key={index}>
                  <TableCell className="px-1.5 py-1">
                    <strong>S{index}</strong>
                  </TableCell>
                  <TableCell className="px-1.5 py-1">
                    <Badge
                      variant={index === draft.button ? "default" : "outline"}
                    >
                      {position}
                    </Badge>
                  </TableCell>
                  <TableCell className="px-1.5 py-1">
                    <Input
                      className="h-7 text-right font-mono"
                      inputMode="decimal"
                      value={seat.stackBb}
                      onChange={(event) =>
                        updateSeat(index, "stackBb", event.target.value)
                      }
                      aria-label={`Seat ${index} stack BB`}
                    />
                  </TableCell>
                  <TableCell className="px-1.5 py-1">
                    <Input
                      className="h-7 text-right font-mono"
                      inputMode="decimal"
                      value={seat.blindBb}
                      onChange={(event) =>
                        updateSeat(index, "blindBb", event.target.value)
                      }
                      aria-label={`Seat ${index} live blind BB`}
                    />
                  </TableCell>
                  <TableCell className="px-1.5 py-1">
                    <Input
                      className="h-7 text-right font-mono"
                      inputMode="decimal"
                      value={seat.anteBb}
                      onChange={(event) =>
                        updateSeat(index, "anteBb", event.target.value)
                      }
                      aria-label={`Seat ${index} ante BB`}
                    />
                  </TableCell>
                  <TableCell className="px-1.5 py-1">
                    <Input
                      className="h-7 min-w-64 font-mono"
                      value={seat.range}
                      onChange={(event) =>
                        updateSeat(index, "range", event.target.value)
                      }
                      spellCheck={false}
                      aria-label={`Seat ${index} range expression`}
                    />
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
        <p className="field-help">
          <code>random</code> は全1,326
          comboをweight&nbsp;1で指定します（全席の既定値）。Rust側で正規化し、全席を同時に配れるか検証します。
        </p>
      </CardContent>
    </Card>
  )
}
