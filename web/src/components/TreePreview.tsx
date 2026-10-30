import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { api } from "../api/client";
import type { PreviewTreeRow, Seat, StartState } from "../api/types";
import { Button, Card, CardAction, CardContent, CardHeader, CardTitle, EmptyState, Icon, IconButton } from "../cryst";
import { actionColors } from "../lib/actions";
import { STREET_LABEL } from "../lib/cards";
import { displayAmount } from "../lib/format";
import type { SizeRole } from "../lib/sizes";
import { describeAction, outcome, rootStacks, sizePercent, splitStreets, type ActionInfo } from "../lib/tree";
import { CardGrid } from "./CardGrid";
import { Notice, PlayingCard } from "./common";

const selector = (history: string) => history || "root";
/** Decisions this deep (0 = the street's first) start open. */
const OPEN_DEPTH = 2;
/** The street a chance node after each street deals. */
const DEALT: Record<string, string> = { flop: "turn", turn: "river" };

/** One street of the walked line: where it starts and how it was reached. */
interface StreetStep {
  /** The street's first node: an action node, or the terminal an all-in runout ends at. */
  root: string;
  /** Cards dealt to reach it, as labelled (iso-merged representatives carry `*`). */
  cards: string[];
  /** The previous street's actions that closed it. */
  via: string[];
}

/** A deal in progress: the chance node shown, and the cards picked before it in a runout. */
interface Dealing {
  /** Index of the street the deal leaves. */
  from: number;
  via: string[];
  cards: string[];
  row: PreviewTreeRow;
}

function actionText(info: ActionInfo, bbChips: number): string {
  const amount = info.amount === null ? "" : ` ${displayAmount(info.amount, bbChips)}`;
  if (info.allIn && (info.kind === "bet" || info.kind === "raise")) return `All-in${amount}`;
  if (info.kind === "raise") return `Raise to${amount}`;
  const name = info.kind[0]!.toUpperCase() + info.kind.slice(1);
  return info.kind === "call" ? `${name}${amount}${info.allIn ? " (all-in)" : ""}` : `${name}${amount}`;
}

/** The size as the script's literals read it; an all-in reads from its label. */
function sizeText(info: ActionInfo): string {
  if (info.potPercent === null || info.allIn) return "";
  const percent = sizePercent(info.potPercent);
  return info.kind === "raise" && info.times !== null ? `${percent} · ${Number(info.times.toFixed(1))}x` : percent;
}

/** The tree items currently shown, in document order. */
function items(tree: HTMLElement): HTMLElement[] {
  return [...tree.querySelectorAll<HTMLElement>('[role="treeitem"]')];
}

/**
 * The game tree the document defines, before solving (`POST /v1/tree` with
 * `restOfStreet`): one street at a time as an outline of every decision on
 * it, each action sized as the script's literals read it. An action that
 * closes the street deals the next card; the streets walked stay as tabs
 * above. After an edit the same streets are read again, and the first street
 * alone when the new tree lacks one.
 */
export function TreeCard({
  toml,
  ready,
  roles,
  start,
  bbChips,
}: {
  /** The document as validated. */
  toml: string;
  /** Validation passed, so the tree can be built. */
  ready: boolean;
  roles: Record<SizeRole, string> | null;
  /** The validated start, for the stacks behind (all-in labels). */
  start: StartState | null;
  bbChips: number;
}) {
  const [streets, setStreets] = useState<StreetStep[]>([{ root: "", cards: [], via: [] }]);
  const [rows, setRows] = useState<PreviewTreeRow[][] | null>(null);
  const [viewing, setViewing] = useState(0);
  const [dealing, setDealing] = useState<Dealing | null>(null);
  /** Decisions opened or closed by hand, by the history of the node they lead to. */
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const [focused, setFocused] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const sequence = useRef(0);
  const treeRef = useRef<HTMLDivElement>(null);
  const roots = streets.map((street) => street.root);
  const rootsKey = roots.join("\n");
  const stacks = useMemo(() => rootStacks(start), [start]);

  useEffect(() => {
    if (!ready) return;
    const id = ++sequence.current;
    setLoading(true);
    const timer = setTimeout(async () => {
      try {
        const fetched = await api.tree(toml, roots.map(selector), true);
        if (id !== sequence.current) return;
        setRows(splitStreets(fetched, roots));
        setError(null);
      } catch (e) {
        if (id !== sequence.current) return;
        if (roots.length > 1) {
          // The edited tree lacks a walked street: start again from the first.
          setStreets([{ root: "", cards: [], via: [] }]);
          setViewing(0);
          setDealing(null);
        } else {
          setError((e as Error).message);
        }
      } finally {
        if (id === sequence.current) setLoading(false);
      }
    }, 300);
    return () => clearTimeout(timer);
    // `roots` is read through `rootsKey`.
  }, [toml, ready, rootsKey]);

  const seat = (actor: Seat | null) =>
    actor === null ? "" : roles ? `${roles[actor]} (${actor.toUpperCase()})` : actor.toUpperCase();
  const shown = Math.min(viewing, (rows?.length ?? 1) - 1);
  const streetRows = rows?.[shown] ?? [];
  const byHistory = useMemo(() => new Map(streetRows.map((row) => [row.history, row])), [streetRows]);
  const first = streetRows[0];

  const isOpen = (history: string, depth: number) => open[history] ?? depth < OPEN_DEPTH;
  const setAll = (value: boolean) =>
    setOpen((all) => {
      const next = { ...all };
      for (const row of streetRows) next[row.history] = value;
      return next;
    });

  const deal = async (from: number, chance: string, via: string[], cards: string[]) => {
    setLoading(true);
    try {
      const [row] = await api.tree(toml, [chance]);
      if (row) setDealing({ from, via, cards, row });
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  };

  const pick = (child: string) => {
    if (!dealing) return;
    const index = dealing.row.children.indexOf(child);
    const cards = [...dealing.cards, dealing.row.actions[index] ?? child];
    if (dealing.row.child_kinds[index] === "chance") {
      // An all-in runout: the next card comes straight away.
      void deal(dealing.from, child, dealing.via, cards);
      return;
    }
    setStreets([...streets.slice(0, dealing.from + 1), { root: child, cards, via: dealing.via }]);
    setViewing(dealing.from + 1);
    setDealing(null);
    setFocused(null);
  };

  /** The labels from the street's first node to `history`'s parent action, then `last`. */
  const lineTo = (row: PreviewTreeRow, last: string): string[] => {
    const label = (action: string) => action.replace(/^(bet|raise to) (\d+(?:\.\d+)?)$/, (original, kind: string, value: string) => {
      const amount = displayAmount(Number(value), bbChips);
      return amount.endsWith("c") ? `${kind} ${amount}` : original;
    });
    const labels = [label(last)];
    let history = row.history;
    while (history !== first?.history) {
      const parent = streetRows.find((candidate) => candidate.children.includes(history));
      if (!parent) break;
      labels.unshift(label(parent.actions[parent.children.indexOf(history)] ?? ""));
      history = parent.history;
    }
    return labels;
  };

  const activate = (row: PreviewTreeRow, index: number, depth: number) => {
    const child = row.children[index]!;
    const where = outcome(row, index);
    if (where === "decision") setOpen((all) => ({ ...all, [child]: !isOpen(child, depth) }));
    if (where === "deal") void deal(shown, child, lineTo(row, row.actions[index] ?? ""), []);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const tree = treeRef.current;
    const item = event.target instanceof HTMLElement ? event.target.closest<HTMLElement>('[role="treeitem"]') : null;
    if (!tree || !item) return;
    const list = items(tree);
    const at = list.indexOf(item);
    const go = (target: HTMLElement | undefined | null) => {
      if (!target) return;
      event.preventDefault();
      target.focus();
    };
    const expanded = item.getAttribute("aria-expanded");
    switch (event.key) {
      case "ArrowDown":
        go(list[at + 1]);
        break;
      case "ArrowUp":
        go(list[at - 1]);
        break;
      case "Home":
        go(list[0]);
        break;
      case "End":
        go(list.at(-1));
        break;
      case "ArrowRight":
        if (expanded === "false") {
          event.preventDefault();
          item.querySelector<HTMLElement>(".tree-row")?.click();
        } else if (expanded === "true") go(item.querySelector<HTMLElement>('[role="treeitem"]'));
        break;
      case "ArrowLeft":
        if (expanded === "true") {
          event.preventDefault();
          item.querySelector<HTMLElement>(".tree-row")?.click();
        } else go(item.parentElement?.closest<HTMLElement>('[role="treeitem"]'));
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        item.querySelector<HTMLElement>(".tree-row")?.click();
        break;
    }
  };

  const group = (row: PreviewTreeRow, depth: number) => {
    const colors = actionColors(row.actions);
    const pot = row.pot === null ? "" : `pot ${displayAmount(row.pot, bbChips)}`;
    return (
      <div role="group" className="tree-group" aria-label={`${seat(row.actor)} to act, ${pot}`}>
        <div className="tree-group-head" aria-hidden="true">
          <span>{seat(row.actor)}</span>
          <span className="mono muted">{pot}</span>
        </div>
        {row.children.map((child, index) => {
          const info = describeAction(row, index, stacks, bbChips);
          const where = outcome(row, index);
          const next = where === "decision" ? byHistory.get(child) : undefined;
          const expanded = next ? isOpen(child, depth) : undefined;
          const tabbable = focused === null ? row === first && index === 0 : focused === child;
          return (
            <div
              key={child}
              role="treeitem"
              className="tree-item"
              aria-level={depth + 1}
              aria-expanded={expanded}
              aria-setsize={row.actions.length}
              aria-posinset={index + 1}
              aria-label={`${actionText(info, bbChips)} ${sizeText(info)}`.trim()}
              tabIndex={tabbable ? 0 : -1}
              onFocus={(event) => {
                if (event.target === event.currentTarget) setFocused(child);
              }}
            >
              <div className="tree-row" onClick={() => activate(row, index, depth)}>
                <span className="tree-toggle" aria-hidden="true">
                  {next && <Icon name={expanded ? "chevron-down" : "chevron-right"} size={14} />}
                </span>
                <span className="swatch" style={{ background: colors[index] }} />
                <span className="tree-label">{actionText(info, bbChips)}</span>
                <span className="tree-size mono">{sizeText(info)}</span>
                <span className={`tree-outcome ${where}`}>
                  {where === "deal" && (
                    <>
                      Deal {DEALT[row.street] ?? "card"}
                      <Icon name="chevron-right" size={14} />
                    </>
                  )}
                  {where === "showdown" && "Showdown"}
                </span>
              </div>
              {next && expanded && group(next, depth + 1)}
            </div>
          );
        })}
      </div>
    );
  };

  const streetName = (index: number) =>
    STREET_LABEL[rows?.[index]?.[0]?.street ?? ""] ?? (index === 0 ? "Start" : "Next street");
  const dealStreet = dealing?.row.street;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Tree</CardTitle>
        <CardAction>
          <div className="row" style={{ gap: 2 }}>
            {loading && <Icon name="loader-2" className="spin muted" />}
            {ready && first?.kind === "action" && (
              <>
                <IconButton icon="chevrons-down" label="Expand all" size="sm" onClick={() => setAll(true)} />
                <IconButton icon="chevrons-up" label="Collapse all" size="sm" onClick={() => setAll(false)} />
              </>
            )}
          </div>
        </CardAction>
      </CardHeader>
      <CardContent>
        {!ready ? (
          <EmptyState size="sm" icon="binary-tree" title="The tree shows once validation passes" />
        ) : error ? (
          <Notice variant="destructive" title="Cannot build the tree" description={error} />
        ) : rows ? (
          <div className="stack" style={{ gap: 12 }}>
            <nav className="row wrap tree-streets" aria-label="Streets walked">
              {streets.map((step, index) => {
                // The first street shows the board it starts on; later ones, the cards dealt to reach them.
                const cards = index === 0 ? (start?.board ?? []) : step.cards;
                const pot = rows[index]?.[0]?.pot;
                return (
                  <button
                    key={step.root || "root"}
                    type="button"
                    className="history-card"
                    aria-current={index === shown ? "step" : undefined}
                    title={step.via.length ? `After ${step.via.join(", ")}` : undefined}
                    disabled={index >= rows.length}
                    onClick={() => {
                      setViewing(index);
                      setDealing(null);
                      setFocused(null);
                    }}
                  >
                    <span className="history-head">
                      <span>{streetName(index)}</span>
                      {pot != null && <span className="mono meta muted">{displayAmount(pot, bbChips)}</span>}
                    </span>
                    {cards.length > 0 && (
                      <span className="row" style={{ gap: 2 }}>
                        {cards.map((card) => (
                          <PlayingCard key={card} card={card.replace("*", "")} small />
                        ))}
                      </span>
                    )}
                  </button>
                );
              })}
            </nav>
            {streets[shown]!.via.length > 0 && (
              <p className="meta muted" style={{ margin: 0 }}>
                After <span className="mono">{streets[shown]!.via.join(" · ")}</span>
              </p>
            )}
            {first?.kind === "terminal" ? (
              <Notice variant="info" title="Showdown" description="The players are all-in; the board runs out to showdown." />
            ) : first ? (
              <div
                ref={treeRef}
                role="tree"
                aria-label={`${streetName(shown)} decisions`}
                className="tree-outline"
                onKeyDown={onKeyDown}
              >
                {group(first, 0)}
              </div>
            ) : null}
            {dealing && (
              <section className="tree-deal-panel stack" aria-label={`${STREET_LABEL[dealStreet ?? ""] ?? "Next"} card`}>
                <div className="row" style={{ gap: 8, alignItems: "baseline" }}>
                  <strong>{STREET_LABEL[dealStreet ?? ""] ?? "Next"} card</strong>
                  <span className="meta muted grow">
                    after <span className="mono">{dealing.via.join(" · ")}</span>
                    {dealing.cards.length > 0 && ` · ${dealing.cards.join(" ")}`}
                  </span>
                  <Button size="sm" variant="ghost" onClick={() => setDealing(null)}>
                    Cancel
                  </Button>
                </div>
                <CardGrid row={dealing.row} onPick={pick} />
              </section>
            )}
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}
