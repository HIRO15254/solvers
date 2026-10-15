/** Ranks high to low, as the input grammar spells them. */
export const RANKS = "AKQJT98765432";
/** Suits in the order the GUI lists them. */
export const SUITS = "shdc";

/** Tabler icon per suit, rendered filled. */
export const SUIT_ICON: Record<string, string> = { s: "spade", h: "heart", d: "diamonds", c: "clubs" };

const CARD = /^([AKQJT2-9])([shdc])$/;

/**
 * Splits a board written as `Ks 7h 2d` into cards. Returns `null` when any
 * token is not a card or a card repeats; the daemon reports the exact error.
 */
export function parseBoard(text: string): string[] | null {
  const cards = text.trim().split(/\s+/).filter(Boolean);
  if (cards.length === 0 || cards.some((card) => !CARD.test(card))) return null;
  return new Set(cards).size === cards.length ? cards : null;
}

export const STREET_LABEL: Record<string, string> = {
  preflop: "Preflop",
  flop: "Flop",
  turn: "Turn",
  river: "River",
};
