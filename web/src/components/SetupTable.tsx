import { useEffect, useRef, useState } from "react";
import type { TomlPrimitive } from "smol-toml";
import { api } from "../api/client";
import { Button, Card, Label, SegmentedControl, Select } from "../cryst";
import { fieldText, fieldValue, getAt, SCHEMA, setAt, writeDoc } from "../lib/config";
import { bb } from "../lib/format";
import { DraftInput } from "./common";
import { DocText, FieldIssue, useDoc, useIssue } from "./DocFields";
import { PayoutsDialog } from "./PayoutsDialog";

type EconomicsKind = "cash" | "tournament";
const ECONOMICS_OPTIONS: {
  value: EconomicsKind;
  label: string;
  icon: string;
}[] = [
  { value: "cash", label: "cash", icon: "coins" },
  { value: "tournament", label: "tournament", icon: "trophy" },
];
/** The economics keys only one kind has: rake exists only in cash. */
const KIND_KEYS: Record<EconomicsKind, string[]> = {
  cash: ["rake"],
  tournament: ["payouts", "outside_field_bb"],
};

type AnteKind = "none" | "all" | "bb";
const ANTE_OPTIONS: { value: AnteKind; label: string }[] = [
  { value: "none", label: "None" },
  { value: "all", label: "Everyone" },
  { value: "bb", label: "BB ante" },
];
const ANTE_KEY: Record<Exclude<AnteKind, "none">, string> = {
  all: "ante_bb",
  bb: "bb_ante_bb",
};

type TableMode = "normal" | "detail";
const TABLE_MODES: { value: TableMode; label: string }[] = [
  { value: "normal", label: "Same for all" },
  { value: "detail", label: "Per seat" },
];

/** Reads a comma-separated list of non-negative numbers; null when any item is not one. */
const numbers = (text: string): number[] | null => {
  const items = text.split(",").map((item) => item.trim());
  return items.every((item) => /^\d+(\.\d+)?$/.test(item)) ? items.map(Number) : null;
};

/**
 * The Table step: players, stacks, blinds, antes and the economics. The
 * table is set either the same for all (one stack, SB 0.5, an ante kind) or per seat
 * (per-player stacks, blinds and antes as comma lists in the preflop acting
 * order).
 */
export function SetupTable() {
  const { doc, setDoc, update } = useDoc();
  const [payoutsDialog, setPayoutsDialog] = useState(false);
  // The ante kind and table mode picked before the document shows them.
  const [anteChoice, setAnteChoice] = useState<AnteKind>("none");
  const [modeChoice, setModeChoice] = useState<TableMode>("normal");
  const table = (key: string) => getAt(doc, ["table", key]);
  const kind: EconomicsKind = getAt(doc, ["economics", "kind"]) === "tournament" ? "tournament" : "cash";
  const tournament = kind === "tournament";
  const payouts = (getAt(doc, ["economics", "payouts"]) as number[] | undefined) ?? [];

  // Switching kinds drops the other kind's keys; switching back brings them again.
  const dropped = useRef(new Map<string, TomlPrimitive | undefined>());
  const switchKind = (next: EconomicsKind) => {
    if (next === kind) return;
    for (const key of KIND_KEYS[kind])
      dropped.current.set(key, getAt(doc, ["economics", key]) as TomlPrimitive | undefined);
    setDoc((d) => {
      const cleared = KIND_KEYS[kind].reduce((n, key) => setAt(n, ["economics", key], undefined), d);
      const restored = KIND_KEYS[next].reduce(
        (n, key) => setAt(n, ["economics", key], dropped.current.get(key)),
        cleared,
      );
      return setAt(restored, ["economics", "kind"], next);
    });
  };

  // Preflop acting order of the table without straddles, from the daemon.
  const players = fieldText(table("players"));
  const [order, setOrder] = useState<string[]>([]);
  useEffect(() => {
    let live = true;
    api
      .line(
        writeDoc({
          schema: SCHEMA,
          table: { players: Number(players), stack_bb: 100 },
        }),
        [],
      )
      .then(
        (root) => {
          if (!live) return;
          const from = Math.max(
            0,
            root.seats.findIndex((seat) => seat.seat === root.next?.seat),
          );
          setOrder([...root.seats.slice(from), ...root.seats.slice(0, from)].map((seat) => seat.position));
        },
        () => live && setOrder([]),
      );
    return () => {
      live = false;
    };
  }, [players]);

  const positive = (key: string) => Number(table(key) ?? 0) > 0;
  const anteKind: AnteKind = positive("bb_ante_bb") ? "bb" : positive("ante_bb") ? "all" : anteChoice;
  const setAnteKind = (kind: AnteKind) => {
    setAnteChoice(kind);
    setDoc((d) => {
      const value = (getAt(d, ["table", ANTE_KEY[anteKind === "none" ? "all" : anteKind]]) ?? undefined) as
        TomlPrimitive | undefined;
      const moved = kind === "none" ? undefined : value;
      return setAt(
        setAt(d, ["table", "ante_bb"], kind === "all" ? moved : undefined),
        ["table", "bb_ante_bb"],
        kind === "bb" ? moved : undefined,
      );
    });
  };

  // A document that already needs per-seat values opens there; same-for-all drops what it cannot show.
  const straddles = (table("straddles_bb") as number[] | undefined) ?? [];
  const detailed = table("stacks_bb") !== undefined || (table("sb_bb") ?? 0.5) !== 0.5 || straddles.length > 0;
  const tableMode: TableMode = detailed ? "detail" : modeChoice;
  const setTableMode = (mode: TableMode) => {
    setModeChoice(mode);
    if (mode === "normal")
      setDoc((d) => {
        const first = order[0] === undefined ? undefined : getAt(d, ["table", "stacks_bb", order[0]]);
        const stack = (getAt(d, ["table", "stack_bb"]) ?? first) as TomlPrimitive | undefined;
        const cleared = ["stacks_bb", "sb_bb", "straddles_bb"].reduce(
          (next, key) => setAt(next, ["table", key], undefined),
          d,
        );
        return setAt(cleared, ["table", "stack_bb"], stack);
      });
  };

  const n = order.length;
  const amount = (value: unknown, fallback: number) => bb(typeof value === "number" ? value : fallback);
  const stacksText = order.map((p) => fieldText(getAt(doc, ["table", "stacks_bb", p]) ?? table("stack_bb"))).join(", ");
  const blindsText = order
    .map((_, i) => (i === n - 1 ? "1" : i === n - 2 ? amount(table("sb_bb"), 0.5) : amount(straddles[i], 0)))
    .join(", ");
  const antesText = order
    .map((_, i) => amount(Number(table("ante_bb") ?? 0) + (i === n - 1 ? Number(table("bb_ante_bb") ?? 0) : 0), 0))
    .join(", ");

  const commitStacks = (values: number[]) => {
    const equal = values.every((v) => v === values[0]);
    setDoc((d) =>
      order.reduce(
        (next, p, i) => (equal ? next : setAt(next, ["table", "stacks_bb", p], values[i])),
        setAt(setAt(d, ["table", "stacks_bb"], undefined), ["table", "stack_bb"], equal ? values[0] : undefined),
      ),
    );
    return null;
  };
  // The last two seats post SB and BB; the seats from the first actor on
  // that post more are the straddlers. The daemon judges the amounts.
  const commitBlinds = (values: number[]) => {
    if (values[n - 1] !== 1) return "The BB seat is 1 (amounts are in BB)";
    const before = values.slice(0, n - 2);
    const gap = before.indexOf(0);
    const placed = gap < 0 ? before : before.slice(0, gap);
    if (before.slice(placed.length).some((v) => v !== 0)) return `Place straddles in a row from ${order[0]}`;
    const sb = values[n - 2];
    setDoc((d) =>
      setAt(
        setAt(d, ["table", "sb_bb"], sb === 0.5 ? undefined : sb),
        ["table", "straddles_bb"],
        placed.length > 0 ? placed : undefined,
      ),
    );
    return null;
  };
  const commitAntes = (values: number[]) => {
    const all = values.every((v) => v === values[0]);
    const bbOnly = values.slice(0, n - 1).every((v) => v === 0);
    if (!all && !bbOnly) return "Antes are the same for everyone, or the BB's only (BB ante)";
    setDoc((d) =>
      setAt(
        setAt(d, ["table", "ante_bb"], all && values[0]! > 0 ? values[0] : undefined),
        ["table", "bb_ante_bb"],
        !all && bbOnly ? values[n - 1] : undefined,
      ),
    );
    return null;
  };

  const playersIssue = useIssue("table.players");
  const stacksIssue = useIssue("table.stacks_bb", "table.stack_bb");
  const blindsIssue = useIssue("table.sb_bb", "table.straddles_bb");
  const antesIssue = useIssue("table.ante_bb", "table.bb_ante_bb");

  return (
    <>
      <Card title="Table">
        <div className="form-sections">
          <div className="form-grid">
            <div className="field span-2">
              <Label>Stacks, blinds and antes</Label>
              <SegmentedControl
                label="Stacks, blinds and antes"
                options={TABLE_MODES}
                value={tableMode}
                onValueChange={(value) => setTableMode(value === "detail" ? "detail" : "normal")}
              />
            </div>
            <div className="field">
              <Select
                label="Players"
                className="mono"
                aria-invalid={playersIssue ? true : undefined}
                options={["2", "3", "4", "5", "6", "7", "8", "9"]}
                value={players}
                onValueChange={(value) => update(["table", "players"], fieldValue(value))}
              />
              <FieldIssue keys={["table.players"]} />
            </div>
            <DocText label="Chips per BB" path={["table", "bb_chips"]} placeholder="1000" />
            <div className="field">
              <Select
                label="Chip rounding"
                options={["down", "nearest", "up"]}
                value={fieldText(table("chip_rounding") ?? "nearest")}
                onValueChange={(value) => update(["table", "chip_rounding"], value)}
              />
              <FieldIssue keys={["table.chip_rounding"]} />
            </div>
            {tableMode === "normal" && (
              <>
                <DocText
                  label="Stack (BB)"
                  path={["table", "stack_bb"]}
                  hint="Everyone's starting stack. The SB is 0.5 BB"
                />
                <div className="field span-2">
                  <Label>Ante</Label>
                  <SegmentedControl
                    label="Ante"
                    options={ANTE_OPTIONS}
                    value={anteKind}
                    onValueChange={(value) => setAnteKind(ANTE_OPTIONS.find((o) => o.value === value)?.value ?? "none")}
                  />
                </div>
                {anteKind !== "none" && (
                  <DocText
                    label={anteKind === "all" ? "Ante (BB, everyone)" : "BB ante (BB)"}
                    path={["table", ANTE_KEY[anteKind]]}
                    placeholder="0"
                  />
                )}
              </>
            )}
          </div>
          {tableMode === "detail" && (
            <div className="stack" style={{ gap: 16 }}>
              {(
                [
                  ["Stacks (BB)", stacksText, commitStacks, stacksIssue],
                  ["Blinds and straddles (BB)", blindsText, commitBlinds, blindsIssue],
                  ["Antes (BB)", antesText, commitAntes, antesIssue],
                ] as const
              ).map(([label, value, commit, issue]) => (
                <DraftInput
                  key={label}
                  className="mono"
                  label={label}
                  hint={order.length > 0 ? `In the order ${order.join(", ")}` : undefined}
                  value={value}
                  spellCheck={false}
                  issue={issue?.message}
                  read={(text) => numbers(text)?.map(bb).join(", ") ?? ""}
                  onText={(text) => {
                    const values = numbers(text);
                    if (!values) return "Separate non-negative numbers with commas";
                    if (values.length !== order.length)
                      return `Write one per player: ${order.length} (now ${values.length})`;
                    return commit(values);
                  }}
                />
              ))}
            </div>
          )}
        </div>
      </Card>

      <Card title="Economics">
        <div className="form-sections">
          <div className="form-grid">
            <div className="field span-2">
              <Label>Kind</Label>
              <SegmentedControl
                label="Kind"
                options={ECONOMICS_OPTIONS}
                value={kind}
                onValueChange={(value) => switchKind(value === "tournament" ? "tournament" : "cash")}
              />
              <FieldIssue keys={["economics.kind"]} />
            </div>
          </div>
          <div className="form-grid">
            {tournament ? (
              <>
                <div className="field span-2">
                  <Label>Payouts</Label>
                  <div className="picker-field">
                    <Button
                      variant="outline"
                      icon="trophy"
                      aria-haspopup="dialog"
                      onClick={() => setPayoutsDialog(true)}
                    >
                      {payouts.length > 0 ? `${payouts.length} paid` : "Not set"}
                    </Button>
                    {payouts.length > 0 && (
                      <span className="mono muted meta">{payouts.map((p) => bb(p)).join(" / ")}</span>
                    )}
                  </div>
                  <FieldIssue keys={["economics.payouts"]} />
                </div>
                <DocText
                  label="Field off the table (BB)"
                  path={["economics", "outside_field_bb"]}
                  placeholder="none"
                  hint="Stacks of the players left who are not at this table. Comma-separated"
                  list
                />
              </>
            ) : (
              <>
                <DocText
                  label="Rake rate"
                  path={["economics", "rake", "rate"]}
                  placeholder="no rake"
                  hint="0 to 1. For example, 0.05 is 5%"
                />
                <DocText label="Rake cap (BB)" path={["economics", "rake", "cap_bb"]} placeholder="no cap" />
                <div className="span-2">
                  <DocText
                    label="Rake condition"
                    path={["economics", "rake", "when"]}
                    placeholder="flop_dealt"
                    hint="Which hands are raked. Default flop_dealt (no flop, no drop)"
                  />
                </div>
              </>
            )}
          </div>
        </div>
      </Card>

      {payoutsDialog && (
        <PayoutsDialog
          initial={payouts}
          onClose={() => setPayoutsDialog(false)}
          onApply={(next) => {
            update(["economics", "payouts"], next);
            setPayoutsDialog(false);
          }}
        />
      )}
    </>
  );
}
