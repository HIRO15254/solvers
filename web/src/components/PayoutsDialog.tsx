import { useState } from "react";
import { Button, Dialog, IconButton, Input, Select } from "../cryst";
import { bb } from "../lib/format";

/**
 * Payout structures as shares of the prize pool, in place order. ICM only
 * depends on the ratios, so without a pool the shares themselves are written.
 */
const STRUCTURES: { id: string; label: string; shares: (places: number) => number[] }[] = [
  { id: "wta", label: "Winner takes all", shares: () => [100] },
  { id: "65-35", label: "Top 2: 65 / 35", shares: () => [65, 35] },
  { id: "50-30-20", label: "Top 3: 50 / 30 / 20 (SNG)", shares: () => [50, 30, 20] },
  { id: "40-30-20-10", label: "Top 4: 40 / 30 / 20 / 10", shares: () => [40, 30, 20, 10] },
  { id: "final-table", label: "Top 9 (final table)", shares: () => [30, 20, 14, 10, 8, 6.5, 5, 3.5, 3] },
  {
    id: "satellite",
    label: "Satellite (equal payouts to top N)",
    shares: (places) => Array.from({ length: places }, () => 100 / places),
  },
];

/**
 * Edits `economics.payouts` in place order. Presets fill the rows from a
 * structure and an optional prize pool; the rows stay editable. Whether the
 * list is valid (non-increasing, not all equal) is the daemon's to report.
 */
export function PayoutsDialog({
  initial,
  onClose,
  onApply,
}: {
  initial: number[];
  onClose: () => void;
  onApply: (payouts: number[] | undefined) => void;
}) {
  const [rows, setRows] = useState(initial.map((value) => bb(value)));
  const [pool, setPool] = useState("");
  const [structure, setStructure] = useState("");
  const [places, setPlaces] = useState("3");

  const values = rows.map(Number);
  const total = values.reduce((sum, value) => sum + (Number.isFinite(value) ? value : 0), 0);
  const fill = (id: string, seats: string) => {
    const preset = STRUCTURES.find((s) => s.id === id);
    if (!preset) return;
    const scale = Number(pool) > 0 ? Number(pool) / 100 : 1;
    setRows(preset.shares(Math.max(1, Math.round(Number(seats)) || 1)).map((share) => bb(share * scale)));
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && onClose()}
      title="Payout distribution"
      description="Payouts in finishing order. Fill from a preset, then edit individual rows."
      icon="trophy"
      closeLabel="Close"
      className="payouts-dialog"
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => onApply(rows.some((row) => row.trim() !== "") ? values : undefined)}>Apply</Button>
        </>
      }
    >
      <div className="stack" style={{ gap: 16 }}>
        <div className="popover-grid">
          <Select
            label="Preset"
            placeholder="Choose a structure…"
            options={STRUCTURES.map((s) => ({ value: s.id, label: s.label }))}
            value={structure}
            onValueChange={(id) => {
              setStructure(id);
              fill(id, places);
            }}
          />
          <Input
            label="Prize pool"
            className="mono"
            placeholder="Leave blank to keep proportions"
            value={pool}
            onChange={(e) => setPool(e.target.value)}
          />
          {structure === "satellite" && (
            <Input
              label="Equal payout places"
              className="mono"
              value={places}
              onChange={(e) => {
                setPlaces(e.target.value);
                fill(structure, e.target.value);
              }}
            />
          )}
          {structure && (
            <Button variant="ghost" size="sm" icon="refresh" onClick={() => fill(structure, places)}>
              Refill from prize pool
            </Button>
          )}
        </div>

        <div className="payout-rows">
          {rows.map((row, index) => (
            <div key={index} className="payout-row">
              <span className="mono muted">#{index + 1}</span>
              <Input
                size="sm"
                className="mono"
                aria-label={`Payout for place ${index + 1}`}
                value={row}
                onChange={(e) => setRows(rows.map((r, i) => (i === index ? e.target.value : r)))}
              />
              <span className="mono muted meta">{total > 0 ? `${bb((Number(row) / total) * 100)}%` : ""}</span>
              <IconButton
                icon="x"
                size="sm"
                variant="ghost"
                label={`Remove place ${index + 1}`}
                onClick={() => setRows(rows.filter((_, i) => i !== index))}
              />
            </div>
          ))}
        </div>
        <div className="row wrap" style={{ gap: 8 }}>
          <Button variant="outline" size="sm" icon="plus" onClick={() => setRows([...rows, rows.at(-1) ?? ""])}>
            Add place
          </Button>
          <Button
            variant="ghost"
            size="sm"
            icon="sort-descending"
            disabled={rows.length < 2}
            onClick={() => setRows([...rows].sort((x, y) => Number(y) - Number(x)))}
          >
            Sort descending
          </Button>
          <span className="grow" />
          <span className="mono meta">Total {bb(total)}</span>
        </div>
      </div>
    </Dialog>
  );
}
