"use client";

import type { AriaAttributes } from "react";
import CommaListInput from "./CommaListInput";
import type { MultiwaySeat } from "./preflop-config";

type Street = "flop" | "turn" | "river";
type PreflopSizingKey = "openSizesBb" | "isolateSizesBb" | "raiseFactors";
type PostflopSizingKey = "betSizes" | "raiseSizes";
type PostflopOptionKey = "maxAggressiveActions" | "includeAllin";

interface Props {
  seats: MultiwaySeat[];
  onPreflopChange: (
    index: number,
    key: PreflopSizingKey,
    values: number[],
  ) => void;
  onPostflopSizesChange: (
    index: number,
    street: Street,
    key: PostflopSizingKey,
    values: number[],
  ) => void;
  onPostflopOptionChange: (
    index: number,
    street: Street,
    key: PostflopOptionKey,
    value: number | boolean,
  ) => void;
  onRemove: (index: number) => void;
  validationProps: (
    field: string,
  ) => Pick<AriaAttributes, "aria-invalid" | "aria-errormessage">;
}

const PREFLOP_FIELDS: Array<{
  key: PreflopSizingKey;
  label: string;
  note: string;
}> = [
  { key: "openSizesBb", label: "PREFLOP open", note: "raise-to BB" },
  { key: "isolateSizesBb", label: "PREFLOP isolate", note: "raise-to BB after limp" },
  { key: "raiseFactors", label: "PREFLOP re-raise", note: "previous-bet multiple" },
];
const STREETS: Street[] = ["flop", "turn", "river"];

export default function MultiwaySeatOverrides({
  seats,
  onPreflopChange,
  onPostflopSizesChange,
  onPostflopOptionChange,
  onRemove,
  validationProps,
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
            {PREFLOP_FIELDS.map((field) => (
              <label className="field" key={field.key}>
                <span className="field-label">
                  {field.label}
                  <span className="field-label-note">{field.note}</span>
                </span>
                <CommaListInput
                  className="input"
                  inputMode="decimal"
                  values={seat.betting?.[field.key] ?? []}
                  onCommit={(values) =>
                    onPreflopChange(
                      index,
                      field.key,
                      values,
                    )
                  }
                  aria-label={`${seat.position} ${field.label}`}
                  {...validationProps(`seat-${index}-override-${field.key}`)}
                />
              </label>
            ))}
          </div>
          {STREETS.map((street) => {
            const streetBetting = seat.betting?.postflop[street];
            if (!streetBetting) return null;
            return (
              <fieldset className="reveal-panel" key={street}>
                <legend className="field-label">{street.toUpperCase()}</legend>
                <div className="seat-override-grid">
                  {(["betSizes", "raiseSizes"] as const).map((key) => (
                    <label className="field" key={key}>
                      <span className="field-label">
                        {key === "betSizes" ? "Bet sizes" : "Raise sizes"}
                        <span className="field-label-note">pot-after-call</span>
                      </span>
                      <CommaListInput
                        className="input"
                        inputMode="decimal"
                        values={streetBetting[key]}
                        onCommit={(values) =>
                          onPostflopSizesChange(
                            index,
                            street,
                            key,
                            values,
                          )
                        }
                        aria-label={`${seat.position} ${street} ${key}`}
                        {...validationProps(`seat-${index}-override-${street}-${key}`)}
                      />
                    </label>
                  ))}
                  <label className="field">
                    <span className="field-label">Aggressive-action cap</span>
                    <input
                      className="input"
                      type="number"
                      min="0"
                      max="16"
                      step="1"
                      value={streetBetting.maxAggressiveActions}
                      {...validationProps(`seat-${index}-override-${street}-cap`)}
                      onChange={(event) =>
                        onPostflopOptionChange(
                          index,
                          street,
                          "maxAggressiveActions",
                          Math.trunc(Number(event.target.value)),
                        )
                      }
                    />
                  </label>
                  <button
                    type="button"
                    className="toggle"
                    aria-pressed={streetBetting.includeAllin}
                    onClick={() =>
                      onPostflopOptionChange(
                        index,
                        street,
                        "includeAllin",
                        !streetBetting.includeAllin,
                      )
                    }
                  >
                    <span className="toggle-copy">
                      <strong>All-inを追加</strong>
                      <small>{street.toUpperCase()}の別分岐</small>
                    </span>
                    <span className="toggle-track" aria-hidden="true" />
                  </button>
                </div>
              </fieldset>
            );
          })}
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
