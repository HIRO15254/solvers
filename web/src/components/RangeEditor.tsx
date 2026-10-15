import { useId, useMemo, useRef, useState, type ReactNode } from "react";
import { Icon, Label, SegmentedControl, Textarea } from "../cryst";
import { Notice } from "./common";
import { RANKS } from "../lib/cards";
import { classCells, classCombos, className, formatRange, parseRange, rangeCounts } from "../lib/range";
import { integer } from "../lib/format";

const PAINT_WEIGHTS = [1, 0.75, 0.5, 0.25];

/**
 * A range string and its 13×13 picture. Painting a class on the grid
 * rewrites the string from the resulting weights. `toolbar` sits at the
 * start of the count row (the seat Tabs on Setup); `issue` is the
 * validator's reason for the string, shown under it.
 */
export function RangeEditor({
  text,
  board,
  toolbar,
  issue,
  onChange,
}: {
  text: string;
  board: string[];
  toolbar?: ReactNode;
  issue?: string;
  onChange: (text: string) => void;
}) {
  const id = useId();
  const parsed = useMemo(() => parseRange(text), [text]);
  const cells = useMemo(() => classCells(parsed.weights, board), [parsed, board]);
  const counts = useMemo(() => rangeCounts(parsed.weights, board), [parsed, board]);
  const [paint, setPaint] = useState(1);
  // While a drag is in progress: the weight being painted (0 erases).
  const stroke = useRef<number | null>(null);

  const apply = (name: string, weight: number) => {
    const weights = new Map(parseRange(text).weights);
    for (const combo of classCombos(name)) weights.set(combo, weight);
    onChange(formatRange(weights));
  };

  return (
    <div className="stack" style={{ gap: 16 }}>
      <div className="row wrap" style={{ gap: 12 }}>
        {toolbar}
        <span className="grow" />
        <span className="meta-item">
          <Icon name="layout-grid" />
          <span className="mono">{counts.classes}</span> {counts.classes === 1 ? "class" : "classes"}
        </span>
        <span className="meta-item">
          <Icon name="cards" />
          <span className="mono">{integer(Math.round(counts.combos * 10) / 10)}</span>{" "}
          {counts.combos === 1 ? "combo" : "combos"} (excluding board conflicts)
        </span>
      </div>
      <div className="row wrap" style={{ alignItems: "flex-start", gap: 20 }}>
        <div className="stack" style={{ flex: "1 1 240px", minWidth: 0, gap: 12 }}>
          <div className="field">
            <Label htmlFor={id}>Range</Label>
            <Textarea
              id={id}
              className="mono"
              rows={9}
              spellCheck={false}
              aria-invalid={issue ? true : undefined}
              value={text}
              onChange={(event) => onChange(event.target.value)}
            />
            {issue && (
              <span className="cr-hint cr-hint-error" role="alert">
                {issue}
              </span>
            )}
          </div>
          {parsed.errors.length > 0 && (
            <Notice variant="warning" title="Some entries could not be parsed">
              <ul className="notice-list">
                {parsed.errors.map((error, i) => (
                  <li key={i}>{error}</li>
                ))}
              </ul>
            </Notice>
          )}
          <div className="field">
            <Label>Paint weight</Label>
            <SegmentedControl
              label="Paint weight"
              size="sm"
              fit
              options={PAINT_WEIGHTS.map((weight) => ({ value: String(weight), label: `${weight * 100}%` }))}
              value={String(paint)}
              onValueChange={(value) => setPaint(Number(value))}
            />
          </div>
        </div>
        <div
          className="hand-grid"
          style={{ flex: "1 1 320px", maxWidth: 442 }}
          onPointerUp={() => (stroke.current = null)}
          onPointerLeave={() => (stroke.current = null)}
        >
          {RANKS.split("").flatMap((_, i) =>
            RANKS.split("").map((__, j) => {
              const name = className(i, j);
              const cell = cells.get(name)!;
              const on = cell.weight > 0;
              return (
                <div
                  key={name}
                  className={`hand-cell ${on ? "on" : ""}`}
                  title={`${name}  weight ${Number(cell.weight.toFixed(3))}${cell.blocked ? `  (blocked by board: ${cell.blocked})` : ""}`}
                  onPointerDown={(event) => {
                    event.preventDefault();
                    stroke.current = cell.weight >= paint && cell.uniform ? 0 : paint;
                    apply(name, stroke.current);
                  }}
                  onPointerEnter={() => {
                    if (stroke.current !== null) apply(name, stroke.current);
                  }}
                >
                  {on && <span className="fill" style={{ height: `${cell.weight * 100}%` }} />}
                  <span className="label">{name}</span>
                </div>
              );
            }),
          )}
        </div>
      </div>
    </div>
  );
}
