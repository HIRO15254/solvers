import { useEffect, useRef, useState } from "react";
import { api, saveBlob } from "../api/client";
import type { LineCursor, PreviewTreeRow, ValidateResponse } from "../api/types";
import {
  Badge,
  Button,
  Card,
  CardAction,
  CardContent,
  CardHeader,
  CardTitle,
  EmptyState,
  Icon,
  IconButton,
  Input,
  Progress,
} from "../cryst";
import { STREET_LABEL } from "../lib/cards";
import { bb, bytes, integer } from "../lib/format";
import type { SizeRole } from "../lib/sizes";
import { IgnoredCard, KeyValue, Notice, PlayingCard, Section } from "./common";
import { PokerTable } from "./PokerTable";

/** Board cards dealt by the time each street starts. */
const STREET_CARDS: Record<string, number> = { preflop: 0, flop: 3, turn: 4, river: 5 };

export type Validation =
  { state: "pending" } | { state: "ok"; response: ValidateResponse } | { state: "error"; message: string };

/**
 * The table as the daemon replays the document's `[table]` and `spot.line`
 * (`POST /v1/line`): seats, wagers, pot, board and, once the line starts a
 * postflop street with two players, OOP/IP, the effective stack and SPR.
 * The spot's name sits above it. Pressing seats picks the two active
 * players. Validation results are not shown here.
 */
export function SpotCard({
  cursor,
  board,
  ignored,
  roles,
  name,
  autoName,
  nameError,
  selected,
  onName,
  onToggle,
  onBoardDialog,
}: {
  cursor: LineCursor | null;
  /** The board cards the line deals. */
  board: string[];
  /** Board cards past those, left out of the spot. */
  ignored: string[];
  /** OOP and IP once the line starts a postflop street with two players left. */
  roles: Record<SizeRole, string> | null;
  /** The name as written; empty uses `autoName`. */
  name: string;
  autoName: string;
  nameError?: string;
  selected: string[];
  onName: (name: string) => void;
  onToggle: (position: string) => void;
  onBoardDialog: () => void;
}) {
  const required = cursor ? (cursor.complete ? (cursor.boardCards ?? 0) : (STREET_CARDS[cursor.street] ?? 0)) : 0;
  const slots = Math.max(required, board.length);
  const street = cursor ? (STREET_LABEL[cursor.street] ?? cursor.street) : "";
  // The stack behind of the shorter of the two players left, as the replay reports it.
  const effective = roles
    ? Math.min(
        ...(cursor?.seats.filter((s) => s.position === roles.oop || s.position === roles.ip).map((s) => s.stack) ?? []),
      )
    : null;
  return (
    <Card>
      <CardHeader>
        <Input
          className="spot-name"
          icon="tag"
          aria-label="Spot name (also the run and TOML file name)"
          title="Names the spot, its run directory and the saved TOML. Empty uses the placeholder, which follows the spot."
          value={name}
          placeholder={autoName}
          spellCheck={false}
          error={nameError}
          onChange={(event) => onName(event.target.value)}
        />
      </CardHeader>
      <CardContent>
        {cursor ? (
          <PokerTable
            seats={cursor.seats}
            button={cursor.button}
            pot={cursor.pot}
            street={cursor.street}
            roles={roles}
            center={
              <>
                <span className="overline">{street}</span>
                <span className="mono table-pot">pot {bb(cursor.pot)}</span>
                {effective !== null && Number.isFinite(effective) && (
                  <span className="mono meta muted">
                    eff {bb(effective)} · SPR {cursor.pot > 0 ? (effective / cursor.pot).toFixed(2) : "—"}
                  </span>
                )}
                {slots + ignored.length > 0 && (
                  <button type="button" className="table-board" title="Choose the board" onClick={onBoardDialog}>
                    {Array.from({ length: slots }, (_, i) =>
                      board[i] ? (
                        <PlayingCard key={i} card={board[i]} small />
                      ) : (
                        <span key={i} className="slot-empty small" />
                      ),
                    )}
                    {ignored.map((card) => (
                      <IgnoredCard key={card} card={card} small />
                    ))}
                  </button>
                )}
              </>
            }
            selected={selected}
            onToggle={onToggle}
          />
        ) : (
          <div className="poker-table">
            <div className="felt" />
          </div>
        )}
      </CardContent>
    </Card>
  );
}

/**
 * The daemon's `validate --resources` of the document: the error with a way
 * to the step that holds its key, or the tree size, memory and warnings.
 * The start state (players, pot, effective stack, SPR) is on the table.
 */
export function ValidationCard({
  validation,
  storageAuto,
  errorStep,
  onRevalidate,
}: {
  validation: Validation;
  storageAuto: boolean;
  /** The step whose fields the error names, if any. */
  errorStep: { label: string; open: () => void } | null;
  onRevalidate: () => void;
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Validation</CardTitle>
        <CardAction>
          {validation.state === "pending" ? (
            <Icon name="loader-2" className="spin muted" />
          ) : (
            <IconButton icon="refresh" label="Validate again" size="sm" onClick={onRevalidate} />
          )}
        </CardAction>
      </CardHeader>
      <CardContent>
        {validation.state === "error" && (
          <Notice
            variant="destructive"
            title="Validation found a problem"
            description={validation.message}
            actions={
              errorStep && (
                <Button size="sm" variant="outline" iconRight="arrow-right" onClick={errorStep.open}>
                  Fix in {errorStep.label}
                </Button>
              )
            }
          />
        )}
        {validation.state === "ok" && <ValidationSummary response={validation.response} storageAuto={storageAuto} />}
      </CardContent>
    </Card>
  );
}

function ValidationSummary({ response, storageAuto }: { response: ValidateResponse; storageAuto: boolean }) {
  const { summary } = response;
  const resources = summary.resources;
  return (
    <div className="stack" style={{ gap: 20 }}>
      <div className="row wrap" style={{ gap: 8 }}>
        <Badge variant="success" icon="circle-check">
          {summary.status}
        </Badge>
        <Badge>{summary.schema}</Badge>
        <Badge>{summary.gameKind}</Badge>
      </div>
      {resources && (
        <Section title="Tree and resources">
          <KeyValue label="Nodes">{integer(resources.nodes)}</KeyValue>
          <KeyValue label="Terminals">{integer(resources.terminals)}</KeyValue>
          <KeyValue label="Storage">
            {resources.storage}
            {storageAuto && " (chosen by auto)"}
          </KeyValue>
          <KeyValue label="Memory estimate">
            <span style={{ color: resources.withinLimit ? undefined : "var(--destructive)" }}>
              {bytes(resources.memoryEstimateBytes)} / {bytes(resources.memoryLimitBytes)}
            </span>
          </KeyValue>
          <Progress
            label="Memory estimate"
            size="sm"
            variant={resources.withinLimit ? "default" : "destructive"}
            value={Math.min(resources.memoryEstimateBytes, resources.memoryLimitBytes)}
            max={resources.memoryLimitBytes}
          />
        </Section>
      )}
      {summary.warnings.length === 0 ? (
        <Notice variant="success" title="No warnings" />
      ) : (
        <Notice
          variant="warning"
          title={`${summary.warnings.length} warning${summary.warnings.length === 1 ? "" : "s"}`}
        >
          <ul className="notice-list">
            {summary.warnings.map((warning, i) => (
              <li key={i}>{warning}</li>
            ))}
          </ul>
        </Notice>
      )}
      <Button
        variant="outline"
        size="sm"
        icon="device-floppy"
        fullWidth
        onClick={() => saveBlob(new Blob([response.effectiveConfigToml], { type: "text/plain" }), "effective.toml")}
      >
        Save effective config
      </Button>
    </div>
  );
}

const selector = (history: string) => history || "root";

/** A step from the root: the node reached and the edge label that led to it. */
interface TreeStep {
  history: string;
  label: string;
}

/**
 * The game tree the document defines, before solving (`POST /v1/tree`):
 * one node at a time with its actions (or, at a chance node, its cards).
 * Pressing an edge walks down; the path above walks back. After an edit the
 * same node is read again, and the root when the new tree lacks it.
 */
export function TreeCard({
  toml,
  ready,
  roles,
}: {
  /** The document as validated. */
  toml: string;
  /** Validation passed, so the tree can be built. */
  ready: boolean;
  roles: Record<SizeRole, string> | null;
}) {
  const [path, setPath] = useState<TreeStep[]>([]);
  const [row, setRow] = useState<PreviewTreeRow | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const sequence = useRef(0);
  const history = path[path.length - 1]?.history ?? "";

  useEffect(() => {
    if (!ready) return;
    // The shown node is stale from here: its children stay disabled until the new one comes.
    const id = ++sequence.current;
    setLoading(true);
    const timer = setTimeout(async () => {
      try {
        let rows: PreviewTreeRow[];
        try {
          rows = await api.tree(toml, [selector(history)]);
        } catch (e) {
          if (history === "") throw e;
          // The edited tree has no such node: start again at the root.
          rows = await api.tree(toml);
          if (id === sequence.current) setPath([]);
        }
        if (id !== sequence.current) return;
        setRow(rows[0] ?? null);
        setError(rows[0] ? null : "The tree has no root node");
      } catch (e) {
        if (id === sequence.current) setError((e as Error).message);
      } finally {
        if (id === sequence.current) setLoading(false);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [toml, ready, history]);

  const actor = row?.actor
    ? roles
      ? `${row.actor.toUpperCase()} (${roles[row.actor]})`
      : row.actor.toUpperCase()
    : null;
  return (
    <Card>
      <CardHeader>
        <CardTitle>Tree</CardTitle>
        <CardAction>{loading && <Icon name="loader-2" className="spin muted" />}</CardAction>
      </CardHeader>
      <CardContent>
        {!ready ? (
          <EmptyState size="sm" icon="binary-tree" title="The tree shows once validation passes" />
        ) : error ? (
          <Notice variant="destructive" title="Cannot build the tree" description={error} />
        ) : row ? (
          <div className="stack" style={{ gap: 12 }}>
            <nav className="row wrap line-chips" aria-label="Path from the root">
              <button type="button" className="line-chip" disabled={path.length === 0} onClick={() => setPath([])}>
                root
              </button>
              {path.map((step, i) => (
                <button
                  key={step.history}
                  type="button"
                  className="line-chip"
                  disabled={i === path.length - 1}
                  onClick={() => setPath(path.slice(0, i + 1))}
                >
                  {step.label}
                </button>
              ))}
            </nav>
            <div className="tree-node">
              <span className="overline">
                {row.kind === "chance"
                  ? `${STREET_LABEL[row.street] ?? row.street} card`
                  : (STREET_LABEL[row.street] ?? row.street)}
              </span>
              {row.kind === "action" && (
                <span className="mono">
                  {actor} to act{row.pot !== null && ` · pot ${bb(row.pot)}`}
                </span>
              )}
              {row.kind === "terminal" && <span className="muted">Terminal: the hand ends here</span>}
            </div>
            {row.children.length > 0 && (
              <div
                className={`row wrap ${row.kind === "chance" ? "tree-cards" : "line-chips"}`}
                role="group"
                aria-label="Children"
              >
                {row.children.map((child, i) => {
                  const label = row.actions[i] ?? child;
                  const open = () => setPath([...path, { history: child, label }]);
                  return row.kind === "chance" ? (
                    <button
                      key={child}
                      type="button"
                      className="card-pick"
                      title={label}
                      disabled={loading}
                      onClick={open}
                    >
                      <PlayingCard card={label} small />
                    </button>
                  ) : (
                    <button
                      key={child}
                      type="button"
                      className="line-chip option"
                      title={row.child_kinds[i] === "terminal" ? "Ends the hand" : undefined}
                      disabled={loading}
                      onClick={open}
                    >
                      {label}
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}
