"use client";

import type { MultiwaySeat } from "./preflop-config";

type SizingKey = "openSizesBb" | "raiseFactors" | "flop" | "turn" | "river";

interface Props {
  seats: MultiwaySeat[];
  onChange: (index: number, key: SizingKey, values: number[]) => void;
  onRemove: (index: number) => void;
}

const SIZING_FIELDS: Array<{
  key: SizingKey;
  label: string;
  note: string;
}> = [
  { key: "openSizesBb", label: "PREFLOP open / iso", note: "raise-to BB" },
  { key: "raiseFactors", label: "PREFLOP re-raise", note: "previous-bet multiple" },
  { key: "flop", label: "FLOP bet / raise", note: "pot-after-call" },
  { key: "turn", label: "TURN bet / raise", note: "pot-after-call" },
  { key: "river", label: "RIVER bet / raise", note: "pot-after-call" },
];

function parseSizes(value: string): number[] {
  return value
    .split(",")
    .map((entry) => Number(entry.trim()))
    .filter(Number.isFinite);
}

function sizesFor(seat: MultiwaySeat, key: SizingKey): number[] {
  if (!seat.betting) return [];
  if (key === "openSizesBb" || key === "raiseFactors") {
    return seat.betting[key];
  }
  return seat.betting.postflopBetSizes[key];
}

export default function MultiwaySeatOverrides({
  seats,
  onChange,
  onRemove,
}: Props) {
  const overrides = seats
    .map((seat, index) => ({ seat, index }))
    .filter(({ seat }) => seat.betting);

  if (overrides.length === 0) return null;

  return (
    <div className="seat-override-list" aria-label="Seat-specific sizing overrides">
      {overrides.map(({ seat, index }) => (
        <details className="seat-override" key={seat.id} open>
          <summary>
            <span><strong>{seat.position}</strong> actor-specific sizing</span>
            <small>table defaultを完全に置換</small>
          </summary>
          <div className="seat-override-grid">
            {SIZING_FIELDS.map((field) => (
              <label className="field" key={field.key}>
                <span className="field-label">
                  {field.label}
                  <span className="field-label-note">{field.note}</span>
                </span>
                <input
                  className="input"
                  inputMode="decimal"
                  value={sizesFor(seat, field.key).join(", ")}
                  onChange={(event) =>
                    onChange(index, field.key, parseSizes(event.target.value))
                  }
                  aria-label={`${seat.position} ${field.label}`}
                />
              </label>
            ))}
          </div>
          <button
            type="button"
            className="text-button"
            onClick={() => onRemove(index)}
          >
            個別設定を解除
          </button>
        </details>
      ))}
    </div>
  );
}
