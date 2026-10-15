import type { ReactNode } from "react";
import type { LineSeat } from "../api/types";
import { Badge } from "../cryst";
import { STREET_LABEL } from "../lib/cards";
import { bb } from "../lib/format";
import type { SizeRole } from "../lib/sizes";

/**
 * The table as the line replay left it: seats clockwise around the felt, the
 * street wager in front of each seat, `center` (pot and board) in the middle,
 * and OOP/IP on the two players a postflop spot starts with. Every figure
 * comes from the daemon's replay; this only places them. A seat press
 * toggles it in `selected`.
 */
export function PokerTable({
  seats,
  button,
  pot,
  street,
  center,
  roles,
  selected,
  onToggle,
}: {
  seats: LineSeat[];
  button: number;
  pot: number;
  street: string;
  center: ReactNode;
  roles: Record<SizeRole, string> | null;
  selected: string[];
  onToggle: (position: string) => void;
}) {
  // Clockwise from the bottom of the ellipse, the button's seat first.
  const start = Math.max(
    0,
    seats.findIndex((s) => s.seat === button),
  );
  const angle = (index: number) =>
    Math.PI / 2 + (((index - start + seats.length) % seats.length) * 2 * Math.PI) / seats.length;
  const place = (index: number, reach: number) => ({
    left: `${50 + 44 * reach * Math.cos(angle(index))}%`,
    top: `${50 + 40 * reach * Math.sin(angle(index))}%`,
  });

  return (
    <div className="poker-table" aria-label={`${STREET_LABEL[street] ?? street}, pot ${bb(pot)} BB`}>
      <div className="felt" />
      <div className="table-center">{center}</div>
      {seats.map((seat, index) => (
        <div key={`chips-${seat.seat}`}>
          {seat.wager > 0 && (
            <span className="table-wager mono" style={place(index, 0.58)}>
              {bb(seat.wager)}
            </span>
          )}
          {seat.seat === button && (
            <span className="table-button" style={place(index - 0.32, 0.74)} aria-label="button">
              D
            </span>
          )}
        </div>
      ))}
      {seats.map((seat, index) => {
        const picked = selected.includes(seat.position);
        return (
          <div key={seat.seat} className="table-seat-slot" style={place(index, 1)}>
            <button
              type="button"
              className={`table-seat ${seat.status} ${picked ? "picked" : ""}`}
              aria-pressed={picked}
              title={
                picked ? `Unpick ${seat.position} as an active player` : `Pick ${seat.position} as an active player`
              }
              onClick={() => onToggle(seat.position)}
            >
              <span className="table-seat-name">
                {seat.position}
                {roles?.oop === seat.position && <span className="table-role">OOP</span>}
                {roles?.ip === seat.position && <span className="table-role">IP</span>}
              </span>
              {seat.status === "folded" ? (
                <span className="muted meta">fold</span>
              ) : seat.status === "allIn" ? (
                <Badge variant="warning">all-in</Badge>
              ) : (
                <span className="mono meta">{bb(seat.stack)}</span>
              )}
            </button>
          </div>
        );
      })}
    </div>
  );
}
