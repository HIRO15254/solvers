import type { PreviewTreeRow } from "../api/types";
import { RANKS, SUITS } from "../lib/cards";
import { PlayingCard } from "./common";

/**
 * A chance node's cards as a deck (suit rows, rank columns); cards the node
 * cannot deal are faded. Iso-merged representatives carry a `*`.
 */
export function CardGrid({ row, onPick }: { row: PreviewTreeRow; onPick: (child: string) => void }) {
  // label → child, with iso-merged representatives marked `*`.
  const children = new Map(row.actions.map((label, i) => [label.replace("*", ""), { label, child: row.children[i]! }]));
  return (
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
        * marks a representative card after suit-isomorphic merging. Faded cards are unavailable or merged into another
        equivalent card.
      </p>
    </div>
  );
}
