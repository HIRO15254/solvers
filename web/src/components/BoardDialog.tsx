import { useState } from "react";
import { Button, Dialog, Icon } from "../cryst";
import { RANKS, SUIT_ICON } from "../lib/cards";
import { PlayingCard } from "./common";

/** Flop, turn and river: the order cards are dealt to the board. */
const SLOTS = ["Flop", "Flop", "Flop", "Turn", "River"] as const;

/**
 * Picks the board from the deck in dealing order. Mounted only while open, so
 * each opening starts from the board the form holds. How many cards a spot
 * needs is the daemon's to check; the picker only stops at five.
 */
export function BoardDialog({
  initial,
  onClose,
  onApply,
}: {
  initial: string[];
  onClose: () => void;
  onApply: (cards: string[]) => void;
}) {
  const [cards, setCards] = useState(initial);
  const full = cards.length >= SLOTS.length;
  const toggle = (card: string) =>
    setCards((current) =>
      current.includes(card)
        ? current.filter((c) => c !== card)
        : current.length < SLOTS.length
          ? [...current, card]
          : current,
    );

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && onClose()}
      title="Choose board"
      description="Choose cards in Flop, Turn, River order. Click a selected card again to remove it."
      icon="cards"
      closeLabel="Close"
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => onApply(cards)}>Apply</Button>
        </>
      }
    >
      <div className="stack" style={{ gap: 16 }}>
        <div className="row" style={{ gap: 8 }}>
          <div className="board-slots">
            {SLOTS.map((street, i) => (
              <div key={i} className="board-slot">
                <span className="overline">{street}</span>
                {cards[i] ? <PlayingCard card={cards[i]} /> : <span className="slot-empty" aria-hidden="true" />}
              </div>
            ))}
          </div>
          <span className="grow" />
          <Button variant="ghost" size="sm" icon="eraser" disabled={cards.length === 0} onClick={() => setCards([])}>
            Clear
          </Button>
        </div>
        <div className="deck" role="group" aria-label="deck">
          {Object.entries(SUIT_ICON).map(([suit, icon]) =>
            RANKS.split("").map((rank) => {
              const card = rank + suit;
              const picked = cards.includes(card);
              return (
                <button
                  key={card}
                  type="button"
                  className={`deck-card suit-${suit}`}
                  aria-label={card}
                  aria-pressed={picked}
                  disabled={full && !picked}
                  onClick={() => toggle(card)}
                >
                  {rank}
                  <Icon name={icon} filled size={11} />
                </button>
              );
            }),
          )}
        </div>
      </div>
    </Dialog>
  );
}
