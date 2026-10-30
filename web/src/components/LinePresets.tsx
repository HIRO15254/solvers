import { useEffect, useRef, useState } from "react";
import { api } from "../api/client";
import type { LineCursor } from "../api/types";
import { Button, Label, SegmentedControl, Select } from "../cryst";
import { STREET_LABEL } from "../lib/cards";
import { getAt, setAt, writeDoc, type Doc } from "../lib/config";
import { chipDisplay } from "../lib/format";
import { Notice } from "./common";

type Flow = "xx" | "ip" | "oop";
type Street = "flop" | "turn" | "river";
type Flows = Record<"flop" | "turn", Flow>;

interface Preset {
  id: string;
  label: string;
  line: string;
}

/** Raise-to amounts (BB) a preset writes; other sizes are edited in the line itself. */
const SIZE = { open: 2.5, three: 8, four: 20, five: 45, squeeze: 12, iso: 4 };

/** A postflop bet a preset writes, as % of the pot at the start of the street. */
const BET_PCT: Record<"flop" | "turn", number> = { flop: 33, turn: 75 };

/** Apply the table's chip rounding to preset BB amounts before spelling a line token. */
function presetAmount(value: number, bbChips: number, mode: unknown): string {
  const chips = value * bbChips;
  const rounded = mode === "down" ? Math.floor(chips) : mode === "up" ? Math.ceil(chips) : Math.round(chips);
  return chipDisplay(BigInt(rounded), bbChips);
}

const START_OPTIONS: { value: Street; label: string }[] = [
  { value: "flop", label: "Flop" },
  { value: "turn", label: "Turn" },
  { value: "river", label: "River" },
];

const FLOW_OPTIONS: { value: Flow; label: string }[] = [
  { value: "xx", label: "check - check" },
  { value: "ip", label: "OOP check - IP bet - OOP call" },
  { value: "oop", label: "OOP bet - IP call" },
];

/** The postflop streets played before each starting street. */
const PLAYED: Record<Street, ("flop" | "turn")[]> = { flop: [], turn: ["flop"], river: ["flop", "turn"] };

/**
 * The preflop histories that leave exactly `a` and `b` in the hand, `a`
 * acting first preflop. `order` is the preflop acting order; a third player
 * who limps, opens or calls is the nearest seat that can.
 */
function presets(order: string[], a: string, b: string): Preset[] {
  const i = order.indexOf(a);
  const j = order.indexOf(b);
  const before = i > 0 ? order[i - 1] : undefined;
  const between = j - i > 1 ? order[i + 1] : undefined;
  const list: Preset[] = [
    { id: "2bet", label: "2bet", line: `${a} r${SIZE.open}, ${b} c` },
    { id: "3bet", label: "3bet", line: `${a} r${SIZE.open}, ${b} r${SIZE.three}, ${a} c` },
    { id: "4bet", label: "4bet", line: `${a} r${SIZE.open}, ${b} r${SIZE.three}, ${a} r${SIZE.four}, ${b} c` },
    {
      id: "5bet",
      label: "5bet",
      line: `${a} r${SIZE.open}, ${b} r${SIZE.three}, ${a} r${SIZE.four}, ${b} r${SIZE.five}, ${a} c`,
    },
  ];
  // b squeezes over a's open and a caller between them.
  if (between)
    list.push({
      id: "squeeze-opener",
      label: "squeeze vs open",
      line: `${a} r${SIZE.open}, ${between} c, ${b} r${SIZE.squeeze}, ${a} c`,
    });
  // b squeezes over an earlier open that a called.
  if (before)
    list.push({
      id: "squeeze-caller",
      label: "squeeze vs call",
      line: `${before} r${SIZE.open}, ${a} c, ${b} r${SIZE.squeeze}, ${a} c`,
    });
  // Only the player who closes the preflop action can check behind a limp.
  if (j === order.length - 1) list.push({ id: "limp", label: "limp", line: `${a} c, ${b} x` });
  // a isolates an earlier limper and b calls.
  if (before) list.push({ id: "iso", label: "iso", line: `${before} c, ${a} r${SIZE.iso}, ${b} c` });
  // a limps, b isolates and a calls.
  list.push({ id: "limp-call", label: "limp-iso", line: `${a} c, ${b} r${SIZE.iso}, ${a} c` });
  return list;
}

/**
 * Common lines for the two active players picked on the table, as in a
 * drill. Picking a preset, or changing its starting street or postflop
 * history, writes the line at once; amounts are edited in the line itself.
 * Seat order, the postflop actor and the pot come from the daemon's replay
 * (`POST /v1/line`); this only spells the line, and the daemon judges it.
 */
export function LinePresets({
  doc,
  toml,
  line,
  players,
  onLine,
}: {
  doc: Doc;
  toml: string;
  line: string;
  /** The two active players picked on the table, in any order. */
  players: string[];
  onLine: (line: string, boardCards: number) => void;
}) {
  const [root, setRoot] = useState<LineCursor | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [start, setStart] = useState<Street>("flop");
  const [flows, setFlows] = useState<Flows>({ flop: "ip", turn: "ip" });
  const [error, setError] = useState<string | null>(null);
  const sequence = useRef(0);
  /** The line this component wrote last; a different line means the user took over. */
  const written = useRef<string | null>(null);

  const tableKey = JSON.stringify(getAt(doc, ["table"]) ?? null);
  useEffect(() => {
    let live = true;
    api.line(toml, []).then(
      (cursor) => live && setRoot(cursor),
      () => live && setRoot(null),
    );
    return () => {
      live = false;
    };
  }, [tableKey]);

  // Preflop order: clockwise from the root's first actor.
  const seats = root?.seats ?? [];
  const from = Math.max(
    0,
    seats.findIndex((seat) => seat.seat === root?.next?.seat),
  );
  const order = [...seats.slice(from), ...seats.slice(0, from)].map((seat) => seat.position);
  const pair = players.filter((p) => order.includes(p)).sort((x, y) => order.indexOf(x) - order.indexOf(y));
  const list = pair.length === 2 ? presets(order, pair[0]!, pair[1]!) : [];
  const active = chosen !== null && line === written.current ? list.find((p) => p.id === chosen) : undefined;

  const apply = async (preset: Preset, next: { start: Street; flows: Flows }) => {
    const id = ++sequence.current;
    setError(null);
    const replay = (text: string) => api.line(writeDoc(setAt(doc, ["spot", "line"], text)));
    try {
      if (!root) throw new Error("Table is not ready");
      const amount = (value: number) => presetAmount(value, root.bbChips, getAt(doc, ["table", "chip_rounding"]));
      let text = preset.line.replace(/\br(\d+(?:\.\d+)?)/g, (_, value: string) => `r${amount(Number(value))}`);
      for (const street of PLAYED[next.start]) {
        const cursor = await replay(text);
        const oop = cursor.next?.position;
        const ip = cursor.seats.find((seat) => seat.status !== "folded" && seat.position !== oop)?.position;
        if (!oop || !ip) throw new Error(`No two players can act on the ${STREET_LABEL[street]}`);
        const flow = next.flows[street];
        const bet = amount((cursor.pot * BET_PCT[street]) / 100);
        text +=
          " / " +
          (flow === "xx"
            ? `${oop} x, ${ip} x`
            : flow === "ip"
              ? `${oop} x, ${ip} b${bet}, ${oop} c`
              : `${oop} b${bet}, ${ip} c`);
      }
      const cursor = await replay(text);
      if (id !== sequence.current) return;
      if (!cursor.complete || cursor.boardCards === null) throw new Error(`${text} does not start a postflop street`);
      written.current = cursor.line;
      setChosen(preset.id);
      onLine(cursor.line, cursor.boardCards);
    } catch (e) {
      if (id === sequence.current) setError((e as Error).message);
    }
  };

  // The picked players changed: the chosen history follows them when it still exists.
  const pairKey = pair.join(",");
  useEffect(() => {
    const preset = chosen === null ? undefined : list.find((p) => p.id === chosen);
    if (preset && written.current === line) void apply(preset, { start, flows });
    // Only a change of players re-applies here; the other inputs apply in their handlers.
  }, [pairKey]);

  /** Applies a changed input when a preset is in effect. */
  const change = (next: { start: Street; flows: Flows }) => {
    setStart(next.start);
    setFlows(next.flows);
    if (active) void apply(active, next);
  };
  const current = { start, flows };

  if (pair.length !== 2)
    return (
      <span className="muted meta">
        Press two seats on the table to pick a line that leaves them in (picked: {pair.join(", ") || "none"})
      </span>
    );

  return (
    <div className="stack" style={{ gap: 16 }}>
      <div className="preset-list" role="group" aria-label="Preflop histories">
        {list.map((preset) => (
          <Button
            key={preset.id}
            size="sm"
            variant={active?.id === preset.id ? "secondary" : "outline"}
            aria-pressed={active?.id === preset.id}
            onClick={() => void apply(preset, current)}
          >
            {preset.label}
          </Button>
        ))}
      </div>
      <div className="form-grid">
        <div className="field span-2">
          <Label>Starting street</Label>
          <SegmentedControl
            label="Starting street"
            options={START_OPTIONS}
            value={start}
            onValueChange={(value) =>
              change({ ...current, start: START_OPTIONS.find((o) => o.value === value)?.value ?? "flop" })
            }
          />
        </div>
        {PLAYED[start].map((street) => (
          <div key={street} className="span-2">
            <Select
              label={`${STREET_LABEL[street]} (bet ${BET_PCT[street]}% pot)`}
              options={FLOW_OPTIONS}
              value={flows[street]}
              onValueChange={(value) =>
                change({
                  ...current,
                  flows: { ...flows, [street]: FLOW_OPTIONS.find((o) => o.value === value)?.value ?? "xx" },
                })
              }
            />
          </div>
        ))}
      </div>
      {error && <Notice variant="destructive" title="Cannot build this line" description={error} />}
    </div>
  );
}
