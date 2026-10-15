import { useEffect, useMemo, useState } from "react";
import { api, saveBlob } from "../api/client";
import {
  isTerminal,
  type RunSummary,
  type Seat,
  type SolSummary,
  type StrategyRow,
  type TreeRow,
  type ValueRow,
} from "../api/types";
import { Board, InfoHint, KeyValue, Notice, PlayingCard } from "../components/common";
import { Button, Card, CardContent, EmptyState, Icon, Select, Tabs } from "../cryst";
import { actionColors } from "../lib/actions";
import { RANKS, STREET_LABEL, SUITS } from "../lib/cards";
import { bb, integer, percent } from "../lib/format";
import { ALL_CLASSES, classCombos, classOf } from "../lib/range";
import { runMeta, type RunMeta } from "../lib/runMeta";
import { navigate } from "../route";

/** What the GUI knows about one node of a solution. */
interface NodeData {
  row: TreeRow;
  /** Reach-weighted action frequencies (`export actions`). */
  frequencies?: number[];
  strategy?: StrategyRow[];
  ev?: ValueRow[];
}

const selector = (history: string) => history || "root";

async function loadNode(runId: string, history: string): Promise<NodeData> {
  const [row] = await api.solution<TreeRow[]>(runId, "tree", [selector(history)]);
  if (!row) throw new Error(`Node ${selector(history)} not found`);
  if (row.kind !== "action" || !row.stored) return { row };
  const [actions, strategy, ev] = await Promise.all([
    api.solution<{ action: string; frequency: number }[]>(runId, "actions", [selector(history)]),
    api.solution<StrategyRow[]>(runId, "strategy", [selector(history)]),
    api.solution<ValueRow[]>(runId, "ev", [selector(history)]),
  ]);
  return { row, frequencies: actions.map((a) => a.frequency), strategy, ev };
}

/** Per-class aggregates of a node's combo rows. */
interface ClassStat {
  weight: number;
  /** Σ weight × probability per action. */
  mass: number[];
}

function strategyByClass(rows: StrategyRow[], actions: number): Map<string, ClassStat> {
  const out = new Map<string, ClassStat>();
  for (const row of rows) {
    const name = classOf(row.combo);
    const stat = out.get(name) ?? { weight: 0, mass: new Array<number>(actions).fill(0) };
    stat.weight += row.weight;
    row.probabilities.forEach((p, a) => (stat.mass[a]! += row.weight * p));
    out.set(name, stat);
  }
  return out;
}

function valueByClass(rows: ValueRow[], seat: Seat): Map<string, { weight: number; ev: number }> {
  const sums = new Map<string, { weight: number; value: number }>();
  for (const row of rows) {
    if (row.seat !== seat) continue;
    const name = classOf(row.combo);
    const sum = sums.get(name) ?? { weight: 0, value: 0 };
    sum.weight += row.weight;
    sum.value += row.weight * row.ev;
    sums.set(name, sum);
  }
  return new Map(
    [...sums].map(([name, sum]) => [name, { weight: sum.weight, ev: sum.weight > 0 ? sum.value / sum.weight : 0 }]),
  );
}

function seatTotals(rows: ValueRow[], seat: Seat) {
  let weight = 0;
  let value = 0;
  for (const row of rows) {
    if (row.seat !== seat) continue;
    weight += row.weight;
    value += row.weight * row.ev;
  }
  return { weight, ev: weight > 0 ? value / weight : 0 };
}

type Tab = "strategy" | "ev" | "range";
type Side = "overview" | "hands";

const TABS: { id: Tab; label: string }[] = [
  { id: "strategy", label: "Strategy" },
  { id: "ev", label: "EV" },
  { id: "range", label: "Range" },
];

const SIDE_TABS: { id: Side; label: string }[] = [
  { id: "overview", label: "Overview" },
  { id: "hands", label: "Hands" },
];

const MODE_NOTE: Record<Tab, string> = {
  strategy: "Cell height shows reach weight (the largest class at this node is 100%); color proportions show strategy.",
  ev: "Acting player's EV by class, weighted by reach weight and measured from the start of the spot. Darker means a larger absolute value.",
  range: "Selected seat's reach weight by class (the largest class at this node is 100%).",
};

export function ResultsPage({ runId }: { runId?: string }) {
  const [runs, setRuns] = useState<RunSummary[] | null>(null);
  const [listError, setListError] = useState<string | null>(null);

  useEffect(() => {
    api
      .runs()
      .then((list) => setRuns(list.filter((run) => run.gameKind === "hu-postflop" && isTerminal(run.state)).reverse()))
      .catch((error) => setListError((error as Error).message));
  }, []);

  useEffect(() => {
    const first = runs?.find((run) => run.state === "completed") ?? runs?.[0];
    if (!runId && first) navigate({ page: "results", runId: first.runId });
  }, [runId, runs]);

  const run = runs?.find((r) => r.runId === runId);
  return run ? (
    <Solution key={run.runId} run={run} runs={runs ?? []} />
  ) : (
    <>
      <header className="page-header">
        <h1>Results</h1>
      </header>
      <main className="page-body padded">
        {listError ? (
          <Notice variant="destructive" title="Cannot load runs" description={listError} />
        ) : runs === null ? (
          <Icon name="loader-2" className="spin muted" />
        ) : (
          <EmptyState icon="chart-grid-dots" title="No finished HU Postflop runs" />
        )}
      </main>
    </>
  );
}

function Solution({ run, runs }: { run: RunSummary; runs: RunSummary[] }) {
  const [meta, setMeta] = useState<RunMeta | null>(null);
  const [summary, setSummary] = useState<SolSummary | null>(null);
  const [path, setPath] = useState<string[]>([""]);
  const [nodes, setNodes] = useState<Record<string, NodeData>>({});
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("strategy");
  const [rangeSeat, setRangeSeat] = useState<Seat>("oop");
  const [side, setSide] = useState<Side>("overview");
  const [selectedClass, setSelectedClass] = useState<string | null>(null);

  useEffect(() => {
    runMeta(run)
      .then(setMeta)
      .catch(() => undefined);
    api
      .solution<SolSummary>(run.runId, "summary")
      .then(setSummary)
      .catch((e) => setError((e as Error).message));
  }, [run]);

  const current = path.at(-1)!;
  useEffect(() => {
    for (const history of path) {
      if (nodes[history]) continue;
      loadNode(run.runId, history)
        .then((data) => setNodes((all) => ({ ...all, [history]: data })))
        .catch((e) => setError((e as Error).message));
    }
  }, [path, nodes, run.runId]);

  const node = nodes[current];
  const position = (seat: Seat | null) =>
    seat === null
      ? ""
      : ((seat === "oop" ? meta?.start?.oop.position : meta?.start?.ip.position) ?? seat.toUpperCase());
  const seatName = (seat: Seat) => `${position(seat)}（${seat.toUpperCase()}）`;

  const go = (index: number, child: string) => {
    setPath([...path.slice(0, index + 1), child]);
    setSelectedClass(null);
  };

  const exportCsv = async () => {
    const blob = await api.solutionCsv(run.runId, "strategy", [selector(current)]);
    saveBlob(blob, `${run.runId}-${selector(current).replace(/[^\w.-]+/g, "_")}-strategy.csv`);
  };

  return (
    <>
      <header className="page-header">
        <h1>Results</h1>
        <div style={{ flex: "0 1 220px", minWidth: 0, marginLeft: 8 }}>
          <Select
            aria-label="Run"
            size="sm"
            className="mono"
            options={runs.map((r) => ({
              value: r.runId,
              label: `${r.runId}${r.state !== "completed" ? ` (${r.state})` : ""}`,
            }))}
            value={run.runId}
            onValueChange={(value) => navigate({ page: "results", runId: value })}
          />
        </div>
        {meta && (
          <span className="mono meta muted">
            {meta.line} · {meta.economics}
          </span>
        )}
        <span className="grow" />
        <Button
          variant="outline"
          icon="download"
          disabled={node?.row.kind !== "action" || !node.row.stored}
          onClick={exportCsv}
        >
          Export
        </Button>
      </header>
      <main className="page-body padded stack" style={{ gap: 16 }}>
        {error && <Notice variant="destructive" title="Cannot load solution" description={error} />}
        <nav className="history" aria-label="Action history">
          <div className="history-card start">
            <div className="history-head">
              <span>{STREET_LABEL[nodes[""]?.row.street ?? ""] ?? "Start"}</span>
              <span className="mono meta muted">{summary ? `${bb(summary.pot)} BB` : ""}</span>
            </div>
            <Board cards={summary?.board.split(" ") ?? meta?.board ?? []} small />
          </div>
          {path.map((history, index) => {
            const data = nodes[history];
            const next = path[index + 1];
            if (!data) {
              return (
                <div key={history} className="history-card">
                  <Icon name="loader-2" className="spin muted" />
                </div>
              );
            }
            const { row } = data;
            if (row.kind === "chance") {
              const dealt = next ? row.actions[row.children.indexOf(next)] : undefined;
              return (
                <button
                  key={history}
                  type="button"
                  className="history-card"
                  aria-current={next ? undefined : "step"}
                  title={next ? "Choose another card" : undefined}
                  onClick={() => setPath(path.slice(0, index + 1))}
                  style={{ minWidth: 100, cursor: next ? "pointer" : "default" }}
                >
                  <div className="history-head">
                    <span>{STREET_LABEL[row.street] ?? row.street}</span>
                  </div>
                  {dealt ? (
                    <Board cards={[dealt.replace("*", "")]} small />
                  ) : (
                    <span className="muted">Choose card</span>
                  )}
                </button>
              );
            }
            if (row.kind === "terminal") {
              return (
                <div key={history} className="history-card" aria-current="step">
                  <div className="history-head">
                    <span>Terminal</span>
                  </div>
                </div>
              );
            }
            const colors = actionColors(row.actions);
            return (
              <div key={history} className="history-card node" aria-current={next ? undefined : "step"}>
                <div className="history-head">
                  <span>{position(row.actor)}</span>
                  <span className="mono meta muted">pot {row.pot !== null ? bb(row.pot) : ""} BB</span>
                </div>
                <div className="stack" style={{ gap: 2 }}>
                  {row.actions.map((label, a) => {
                    const child = row.children[a]!;
                    return (
                      <button
                        key={label}
                        type="button"
                        className="act-btn"
                        aria-pressed={child === next}
                        disabled={child.includes("?")}
                        onClick={() => go(index, child)}
                      >
                        <span className="swatch" style={{ background: colors[a] }} />
                        <span className="grow">{label}</span>
                        <span className="mono meta muted">
                          {data.frequencies ? percent(data.frequencies[a] ?? 0) : ""}
                        </span>
                      </button>
                    );
                  })}
                </div>
              </div>
            );
          })}
        </nav>

        {node?.row.kind === "chance" && <CardPicker row={node.row} onPick={(child) => go(path.length - 1, child)} />}
        {node?.row.kind === "terminal" && (
          <Card>
            <EmptyState size="sm" icon="flag" title="Terminal node (hand ended). Select an earlier node to go back." />
          </Card>
        )}
        {node?.row.kind === "action" && !node.row.stored && (
          <Notice
            variant="info"
            title="Strategy is not stored for this node"
            description={
              <>
                This run uses solution_streets = no-rivers. The CLI command{" "}
                <span className="mono">solvers inspect --sol</span> can re-solve and display unstored River nodes.
              </>
            }
          />
        )}
        {node?.row.kind === "action" && node.strategy && node.ev && (
          <div className="row wrap" style={{ alignItems: "flex-start", gap: 16 }}>
            <Card className="matrix-card" aria-label="Hand matrix">
              <CardContent className="stack" style={{ gap: 12 }}>
                <div className="row wrap" style={{ gap: 12 }}>
                  <Tabs
                    variant="pill"
                    items={TABS}
                    value={tab}
                    onValueChange={(value) => setTab(TABS.find((t) => t.id === value)?.id ?? "strategy")}
                  />
                  {tab === "range" && (
                    <Tabs
                      variant="pill"
                      items={(["oop", "ip"] as const).map((seat) => ({ id: seat, label: seatName(seat) }))}
                      value={rangeSeat}
                      onValueChange={(value) => setRangeSeat(value === "ip" ? "ip" : "oop")}
                    />
                  )}
                  <span className="grow" />
                  <span style={{ fontWeight: 500 }}>{seatName(node.row.actor!)} to act</span>
                  <span className="mono meta muted">
                    pot {bb(node.row.pot ?? 0)} BB ·{" "}
                    {integer(Math.round(seatTotals(node.ev, node.row.actor!).weight * 10) / 10)} combos
                  </span>
                </div>
                <NodeGrid
                  node={node}
                  tab={tab}
                  rangeSeat={rangeSeat}
                  selected={selectedClass}
                  onSelect={(name) => {
                    setSelectedClass(name);
                    setSide("hands");
                  }}
                />
              </CardContent>
            </Card>
            <SidePanel
              node={node}
              side={side}
              setSide={setSide}
              selectedClass={selectedClass}
              seatName={seatName}
              unit={meta?.utilityUnit ?? "BB"}
            />
          </div>
        )}
      </main>
    </>
  );
}

function CardPicker({ row, onPick }: { row: TreeRow; onPick: (child: string) => void }) {
  // label → child, with iso-merged representatives marked `*`.
  const children = new Map(row.actions.map((label, i) => [label.replace("*", ""), { label, child: row.children[i]! }]));
  return (
    <Card title={`${STREET_LABEL[row.street] ?? row.street} card`}>
      <div className="stack" style={{ gap: 4 }}>
        {SUITS.split("").map((suit) => (
          <div key={suit} className="card-row">
            {RANKS.split("").map((rank) => {
              const entry = children.get(rank + suit);
              return (
                <button
                  key={rank}
                  type="button"
                  className="card-pick"
                  disabled={!entry}
                  title={entry?.label.endsWith("*") ? "Representative card for a suit-isomorphic group" : undefined}
                  onClick={() => entry && onPick(entry.child)}
                >
                  <PlayingCard card={rank + suit} small />
                  {entry?.label.endsWith("*") && <span className="muted">*</span>}
                </button>
              );
            })}
          </div>
        ))}
        <p className="muted meta" style={{ margin: "8px 0 0" }}>
          * marks a representative card after suit-isomorphic merging. Faded cards are unavailable or merged into
          another equivalent card.
        </p>
      </div>
    </Card>
  );
}

/** EV tint: success/destructive mixed into muted, stronger with |EV|; `strong` fills take light text. */
function evTint(ev: number, low: number, high: number): { background: string; strong: boolean } {
  const span = Math.max(Math.abs(low), Math.abs(high), 1e-9);
  const share = Math.round(15 + 85 * Math.min(1, Math.abs(ev) / span));
  return {
    background: `color-mix(in oklab, var(${ev >= 0 ? "--success" : "--destructive"}) ${share}%, var(--muted))`,
    strong: share >= 60,
  };
}

/**
 * One span per action, as wide as its share of the sum: the shares need not
 * add up to 1 (class masses, rounded frequencies), and the spans still fill
 * the whole width.
 */
function Segments({ values, colors }: { values: number[]; colors: string[] }) {
  const total = values.reduce((sum, v) => sum + Math.max(v, 0), 0);
  if (total <= 0) return null;
  return values.map((v, a) =>
    v > 0 ? <span key={a} style={{ width: `${(v / total) * 100}%`, background: colors[a] }} /> : null,
  );
}

function MixBar({ shares, colors, label }: { shares: number[]; colors: string[]; label: string }) {
  return (
    <div className="mix-bar" role="img" aria-label={label}>
      <Segments values={shares} colors={colors} />
    </div>
  );
}

function Legend({ items }: { items: { color: string; label: string }[] }) {
  return (
    <>
      {items.map((item) => (
        <span key={item.label} className="legend-item">
          <span className="swatch" style={{ background: item.color }} />
          {item.label}
        </span>
      ))}
    </>
  );
}

function NodeGrid({
  node,
  tab,
  rangeSeat,
  selected,
  onSelect,
}: {
  node: NodeData;
  tab: Tab;
  rangeSeat: Seat;
  selected: string | null;
  onSelect: (name: string) => void;
}) {
  const { row } = node;
  const colors = actionColors(row.actions);
  const strategy = useMemo(() => strategyByClass(node.strategy!, row.actions.length), [node, row.actions.length]);
  const values = useMemo(
    () => valueByClass(node.ev!, tab === "range" ? rangeSeat : row.actor!),
    [node, tab, rangeSeat, row.actor],
  );
  // Fill heights are relative to the node's densest class, so a deep node's
  // small reach still reads as a shape.
  const density = (name: string, weight: number) => weight / classCombos(name).length;
  const source = tab === "strategy" ? strategy : values;
  const maxDensity = Math.max(1e-12, ...[...source].map(([name, stat]) => density(name, stat.weight)));
  const evs = [...values.values()].filter((v) => v.weight > 0).map((v) => v.ev);
  const low = Math.min(0, ...evs);
  const high = Math.max(0, ...evs);
  const frequencies = node.frequencies ?? [];

  const legend =
    tab === "strategy"
      ? row.actions.map((label, a) => ({ color: colors[a]!, label }))
      : tab === "ev"
        ? [
            { color: "var(--success)", label: "EV > 0" },
            { color: "var(--destructive)", label: "EV < 0" },
          ]
        : [{ color: "var(--primary)", label: "Reach weight at this node" }];

  return (
    <>
      {tab === "strategy" && (
        <MixBar
          shares={frequencies}
          colors={colors}
          label={row.actions.map((label, a) => `${label} ${percent(frequencies[a] ?? 0)}`).join(" · ")}
        />
      )}
      <div className="hand-grid large">
        {ALL_CLASSES.map((name) => {
          const stat = strategy.get(name);
          const value = values.get(name);
          const weight = tab === "strategy" ? (stat?.weight ?? 0) : (value?.weight ?? 0);
          const on = weight > 0;
          const height = `${(density(name, weight) / maxDensity) * 100}%`;
          const tint = tab === "ev" && value && on ? evTint(value.ev, low, high) : null;
          return (
            <button
              key={name}
              type="button"
              className={`hand-cell ${on ? "on" : ""} ${tint?.strong ? "strong" : ""}`}
              aria-pressed={selected === name}
              disabled={!on}
              onClick={() => onSelect(name)}
              style={tint ? { background: tint.background } : undefined}
              title={on ? `${name} · combo ${Number(weight.toFixed(2))}` : `${name} does not reach this node`}
            >
              {tab === "strategy" && stat && on && (
                <span className="fill stacked" style={{ height }}>
                  <Segments values={stat.mass} colors={colors} />
                </span>
              )}
              {tab === "range" && on && <span className="fill" style={{ height }} />}
              <span className="label">{name}</span>
              {tab === "ev" && value && on && <span className="value">{value.ev.toFixed(1)}</span>}
            </button>
          );
        })}
      </div>
      <div className="row wrap legend">
        <Legend items={legend} />
        <span className="grow" />
        <InfoHint label="Display" text={MODE_NOTE[tab]} align="end" />
      </div>
    </>
  );
}

function SidePanel({
  node,
  side,
  setSide,
  selectedClass,
  seatName,
  unit,
}: {
  node: NodeData;
  side: Side;
  setSide: (side: Side) => void;
  selectedClass: string | null;
  seatName: (seat: Seat) => string;
  unit: string;
}) {
  const { row } = node;
  const colors = actionColors(row.actions);
  const actorRows = node.strategy!;
  const totalWeight = actorRows.reduce((sum, r) => sum + r.weight, 0);
  const combosPerAction = row.actions.map((_, a) =>
    actorRows.reduce((sum, r) => sum + r.weight * (r.probabilities[a] ?? 0), 0),
  );
  const handRows = selectedClass ? actorRows.filter((r) => classOf(r.combo) === selectedClass) : [];
  const evByCombo = new Map(node.ev!.filter((v) => v.seat === row.actor).map((v) => [v.combo, v.ev]));
  const classWeight = handRows.reduce((sum, r) => sum + r.weight, 0);
  const classMix = row.actions.map((_, a) =>
    classWeight > 0 ? handRows.reduce((sum, r) => sum + r.weight * (r.probabilities[a] ?? 0), 0) / classWeight : 0,
  );

  return (
    <Card className="side-card" aria-label="Details">
      <CardContent className="stack" style={{ gap: 16 }}>
        <Tabs
          items={SIDE_TABS}
          value={side}
          onValueChange={(value) => setSide(SIDE_TABS.find((t) => t.id === value)?.id ?? "overview")}
        />
        {side === "overview" ? (
          <>
            <div className="data-list">
              <div className="data-head">
                <span className="grow">Action</span>
                <span className="num" style={{ width: 56 }}>
                  Frequency
                </span>
                <span className="num" style={{ width: 64 }}>
                  Combos
                </span>
              </div>
              {row.actions.map((label, a) => (
                <div key={label} className="data-row plain stack" style={{ gap: 6 }}>
                  <div className="row" style={{ gap: 12 }}>
                    <span className="grow row" style={{ gap: 8, fontWeight: 500 }}>
                      <span className="swatch" style={{ background: colors[a] }} />
                      {label}
                    </span>
                    <span className="mono num" style={{ width: 56, fontWeight: 600 }}>
                      {percent(node.frequencies?.[a] ?? 0)}
                    </span>
                    <span className="mono num muted" style={{ width: 64 }}>
                      {combosPerAction[a]!.toFixed(1)}
                    </span>
                  </div>
                  <span className="share-bar">
                    <span
                      style={{
                        width: `${(totalWeight > 0 ? combosPerAction[a]! / totalWeight : 0) * 100}%`,
                        background: colors[a],
                      }}
                    />
                  </span>
                </div>
              ))}
            </div>
            <div className="field-label">
              <h2 className="overline">Range at this node</h2>
              <InfoHint
                label="Range at this node"
                text="Combo is the sum of reach weights; EV is measured from the start of the spot (hu-postflop.jp.md §3)."
                align="end"
              />
            </div>
            <div className="range-stats">
              {(["oop", "ip"] as const).map((seat) => {
                const totals = seatTotals(node.ev!, seat);
                return (
                  <div key={seat} className="range-stat">
                    <span style={{ fontWeight: 600 }}>{seatName(seat)}</span>
                    <KeyValue label="EV">
                      {totals.ev.toFixed(3)} {unit}
                    </KeyValue>
                    <KeyValue label="Combos">{totals.weight.toFixed(1)}</KeyValue>
                  </div>
                );
              })}
            </div>
          </>
        ) : selectedClass === null ? (
          <EmptyState size="sm" icon="pointer" title="Select a class in the grid to view strategy by combo" />
        ) : (
          <>
            <div className="row" style={{ alignItems: "baseline", gap: 12 }}>
              <h2 className="mono class-title">{selectedClass}</h2>
              <span className="muted">{classWeight.toFixed(2)} combos</span>
            </div>
            {handRows.length === 0 ? (
              <EmptyState size="sm" icon="cards" title="No combos reach this node" />
            ) : (
              <>
                <MixBar
                  shares={classMix}
                  colors={colors}
                  label={row.actions.map((label, a) => `${label} ${percent(classMix[a] ?? 0)}`).join(" · ")}
                />
                <div className="row wrap legend">
                  <Legend items={row.actions.map((label, a) => ({ color: colors[a]!, label }))} />
                </div>
                <div className="data-list">
                  <div className="data-head">
                    <span style={{ width: 72 }}>combo</span>
                    <span className="grow">Strategy</span>
                    <span className="num" style={{ width: 52 }}>
                      EV
                    </span>
                  </div>
                  {handRows.map((hand) => (
                    <div key={hand.combo} className="data-row row" style={{ gap: 10, minHeight: 40 }}>
                      <span className="row" style={{ width: 72, gap: 2 }}>
                        <PlayingCard card={hand.combo.slice(0, 2)} small />
                        <PlayingCard card={hand.combo.slice(2, 4)} small />
                      </span>
                      <span className="grow stack" style={{ gap: 3, minWidth: 0 }}>
                        <MixBar
                          shares={hand.probabilities}
                          colors={colors}
                          label={row.actions
                            .map((label, a) => `${label} ${percent(hand.probabilities[a] ?? 0)}`)
                            .join(" · ")}
                        />
                        <span className="mono meta muted">
                          {hand.probabilities.map((p) => percent(p, 0)).join(" / ")} · w {hand.weight.toFixed(3)}
                        </span>
                      </span>
                      <span className="mono num" style={{ width: 52 }}>
                        {(evByCombo.get(hand.combo) ?? 0).toFixed(2)}
                      </span>
                    </div>
                  ))}
                </div>
              </>
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}
