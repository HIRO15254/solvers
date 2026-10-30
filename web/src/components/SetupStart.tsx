import { useEffect, useState } from "react";
import { api } from "../api/client";
import type { LineCursor, LineOption, LineStep } from "../api/types";
import { Button, Card, Input, Popover, PopoverContent, PopoverTrigger } from "../cryst";
import { STREET_LABEL } from "../lib/cards";
import { fieldText, getAt } from "../lib/config";
import { displayAmount, lineToken, normalizedWager, writtenLine } from "../lib/format";
import { Board } from "./common";
import { DocText, useDoc } from "./DocFields";
import { LinePresets } from "./LinePresets";

const OPTION_LABEL: Record<LineOption["kind"], string> = {
  fold: "fold",
  check: "check",
  call: "call",
  bet: "bet",
  raise: "raise",
  allIn: "all-in",
};

/** An option badge's text: the action and, from the replay, its amount or range (BB). */
function optionText(option: LineOption, bbChips: number): string {
  if ("min" in option) return `${OPTION_LABEL[option.kind]} ${displayAmount(option.min, bbChips)}–${displayAmount(option.max, bbChips)}`;
  if ("amount" in option) return `${OPTION_LABEL[option.kind]} ${displayAmount(option.amount, bbChips)}`;
  return OPTION_LABEL[option.kind];
}

/** Spells a typed line with the grammar's separators: `, ` between actions, ` / ` between streets. */
const lineText = (text: string) =>
  text
    .replace(/\s+/g, " ")
    .replace(/ ?, ?/g, ", ")
    .replace(/ ?\/ ?/g, " / ")
    .trim();


/** A player who can still act on the street, with the replay at their turn and the passes that lead to it. */
interface Turn {
  position: string;
  cursor: LineCursor;
  /** The moves added before this player's turn, as `UTG f`. */
  passes: string[];
}

/**
 * The players who can act on the current street, in turn order: the next
 * player, then each one the replay reaches when everyone before them checks
 * or, without a check, folds. Options come from the daemon's replay, so the
 * GUI decides no turn order or legality of its own.
 */
async function streetTurns(toml: string, cursor: LineCursor): Promise<Turn[]> {
  const street = cursor.next?.street;
  const turns: Turn[] = [];
  const passes: string[] = [];
  let at = cursor;
  while (at.next && at.next.street === street && !turns.some((turn) => turn.position === at.next!.position)) {
    turns.push({ position: at.next.position, cursor: at, passes: [...passes] });
    const pass = at.next.options.find((o) => o.kind === "check") ?? at.next.options.find((o) => o.kind === "fold");
    if (!pass) break;
    passes.push(`${at.next.position} ${pass.token}`);
    at = await api.line(toml, [...at.moves, pass.token]);
  }
  return turns;
}

/** A player's badge on the street row; it opens their actions in a Popover. */
function TurnPicker({
  turn,
  open,
  busy,
  onOpenChange,
  onPick,
}: {
  turn: Turn;
  open: boolean;
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (moves: string[]) => void;
}) {
  const [sizing, setSizing] = useState<Extract<LineOption, { min: number }> | null>(null);
  const [amount, setAmount] = useState("");
  const [error, setError] = useState<string | null>(null);
  const options = turn.cursor.next?.options ?? [];
  const moves = turn.cursor.moves;
  const sized = () => {
    const value = Number(amount);
    if (!sizing) return;
    if (!amount.trim() || !Number.isFinite(value)) {
      setError(`Write the street total in BB (${displayAmount(sizing.min, turn.cursor.bbChips)} to ${displayAmount(sizing.max, turn.cursor.bbChips)})`);
      return;
    }
    onPick([...moves, `${sizing.token}${normalizedWager(amount)}`]);
  };
  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        onOpenChange(next);
        setSizing(null);
        setAmount("");
        setError(null);
      }}
    >
      <PopoverTrigger>
        <button
          type="button"
          className="line-chip option"
          disabled={busy}
          aria-expanded={open}
          aria-haspopup="dialog"
          title={
            turn.passes.length > 0
              ? `Choose ${turn.position}'s action (fills in ${turn.passes.join(", ")})`
              : `Choose ${turn.position}'s action`
          }
        >
          {turn.position}
        </button>
      </PopoverTrigger>
      <PopoverContent className="line-pick" aria-label={`${turn.position}'s action`}>
        <span className="field-label">{turn.position}'s action</span>
        {turn.passes.length > 0 && <span className="muted meta">Fills in: {turn.passes.join(", ")}</span>}
        <div className="row wrap line-chips">
          {options.map((option) => (
            <button
              key={option.kind}
              type="button"
              className="line-chip option"
              disabled={busy}
              aria-pressed={"min" in option ? sizing?.kind === option.kind : undefined}
              onClick={() => {
                setError(null);
                if ("min" in option) setSizing(sizing?.kind === option.kind ? null : option);
                else onPick([...moves, option.token]);
              }}
            >
              {optionText(option, turn.cursor.bbChips)}
            </button>
          ))}
        </div>
        {sizing && (
          <form
            className="line-amount"
            onSubmit={(event) => {
              event.preventDefault();
              sized();
            }}
          >
            <Input
              size="sm"
              className="mono"
              autoFocus
              aria-label={`${OPTION_LABEL[sizing.kind]} amount (BB, street total)`}
              placeholder={`${displayAmount(sizing.min, turn.cursor.bbChips)}–${displayAmount(sizing.max, turn.cursor.bbChips)}`}
              type="number"
              step="any"
              inputMode="decimal"
              value={amount}
              spellCheck={false}
              error={error ?? undefined}
              onChange={(event) => setAmount(event.target.value)}
            />
            <Button size="sm" type="submit" icon="plus" disabled={busy}>
              {OPTION_LABEL[sizing.kind]}
            </Button>
          </form>
        )}
      </PopoverContent>
    </Popover>
  );
}

/**
 * The Start step: the history before the spot and its board. A preset
 * for the two players picked on the table writes the line; the line itself
 * stays editable as text, and its replay (`POST /v1/line`) is listed by
 * street with the folds the line leaves out. After the current street's
 * actions come the players who can still act on it: a badge opens that
 * player's actions, and the players before them check or fold. Pressing a
 * replayed badge takes the line back to before that action.
 */
export function SetupStart({
  toml,
  cursor,
  building,
  selected,
  board,
  ignored,
  onBoardDialog,
  onBuilt,
}: {
  toml: string;
  cursor: LineCursor | null;
  /** The cursor replays moves picked here, not `spot.line` as written. */
  building: boolean;
  /** The two active players picked on the table. */
  selected: string[];
  /** The board cards the line deals. */
  board: string[];
  /** Board cards past those, shown dimmed and left out of the spot. */
  ignored: string[];
  onBoardDialog: () => void;
  /** A replay of picked moves, to become `spot.line`. */
  onBuilt: (cursor: LineCursor) => void;
}) {
  const { doc, update } = useDoc();
  const line = fieldText(getAt(doc, ["spot", "line"]));
  const [buildError, setBuildError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [opened, setOpened] = useState<string | null>(null);
  const streets: [string, { step: LineStep; index: number }[]][] = [];
  for (const [index, step] of (cursor?.actions ?? []).entries()) {
    const last = streets[streets.length - 1];
    if (last && last[0] === step.street) last[1].push({ step, index });
    else streets.push([step.street, [{ step, index }]]);
  }
  const nextStreet = cursor?.next?.street;
  if (nextStreet && streets[streets.length - 1]?.[0] !== nextStreet) streets.push([nextStreet, []]);

  // Each replay finds who can still act on its street; a newer replay drops an older search.
  useEffect(() => {
    let current = true;
    setTurns([]);
    setOpened(null);
    if (cursor?.next)
      streetTurns(toml, cursor).then(
        (found) => current && setTurns(found),
        (error) => current && setBuildError((error as Error).message),
      );
    return () => {
      current = false;
    };
    // The search reads only `[table]` and the replay's moves.
  }, [cursor]);

  const build = async (moves: string[]) => {
    setBusy(true);
    try {
      const result = await api.line(toml, moves);
      // Keep the written BB amount so validation can report rounding.
      onBuilt({ ...result, line: writtenLine(result.line, cursor?.line === line ? cursor.line : "", moves, result.moves) });
      setBuildError(null);
    } catch (error) {
      setBuildError((error as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const moves = cursor?.moves ?? [];

  return (
    <>
      <Card
        title="Pick a history"
        description="Press two seats on the table to pick a preflop history that leaves those two in. Amounts are fixed; edit the line to change them."
      >
        <LinePresets
          doc={doc}
          toml={toml}
          line={line}
          players={selected}
          onLine={(next, boardCards) => {
            update(["spot", "line"], next || undefined);
            if (board.length + ignored.length < boardCards) onBoardDialog();
          }}
        />
      </Card>

      <Card title="Line and board">
        <div className="form-sections">
          <div className="stack" style={{ gap: 12 }}>
            <DocText
              label="Line"
              path={["spot", "line"]}
              placeholder="BTN r2.5, BB c"
              hint="The actions from the forced bets to the spot. Folds are not written. Spaces around commas and / are normalized. Example: BTN r2.5, BB c / BB x, BTN b1.8, BB c"
              normalize={lineText}
            />
            {cursor && streets.length > 0 && (
              <div className="line-streets" aria-label="Line replay and next actions">
                {streets.map(([street, steps]) => (
                  <div key={street} className="line-street">
                    <span className="overline">{STREET_LABEL[street] ?? street}</span>
                    <div className="row wrap line-chips">
                      {steps.map(({ step, index }) => (
                        <button
                          key={index}
                          type="button"
                          className={`line-chip ${step.implicit ? "implicit" : ""}`}
                          disabled={busy}
                          title={`${step.implicit ? "An implicit fold the line leaves out. " : ""}Press to go back to before this action`}
                          onClick={() => void build(moves.slice(0, index))}
                        >
                          {step.position} {lineToken(step, cursor.bbChips)}
                        </button>
                      ))}
                      {street === nextStreet &&
                        turns.map((turn) => (
                          <TurnPicker
                            key={turn.position}
                            turn={turn}
                            open={opened === turn.position}
                            busy={busy}
                            onOpenChange={(open) => setOpened(open ? turn.position : null)}
                            onPick={(picked) => {
                              setOpened(null);
                              void build(picked);
                            }}
                          />
                        ))}
                    </div>
                  </div>
                ))}
                {buildError && (
                  <span className="cr-hint cr-hint-error" role="alert">
                    {buildError}
                  </span>
                )}
                {building && cursor && !cursor.complete && !cursor.terminal && (
                  <p className="muted meta" style={{ margin: 0 }}>
                    The line is not finished. Add actions until a postflop street starts.
                  </p>
                )}
              </div>
            )}
          </div>
          <div className="picker-field">
            <div className="picker-input">
              <DocText label="Board" path={["spot", "board"]} placeholder="Ks 7h 2d" />
            </div>
            <Button variant="outline" icon="cards" aria-haspopup="dialog" onClick={onBoardDialog}>
              Choose
            </Button>
            {board.length + ignored.length > 0 && <Board cards={board} ignored={ignored} />}
          </div>
        </div>
      </Card>
    </>
  );
}
