// The tree-script vocabulary (docs/nlh-input-v1.jp.md §9) for highlighting,
// completion and hover. The daemon remains the parser of record; this list
// only has to name what it accepts. `vocabulary.test.ts` checks it against
// the Rust dialect so a new variable or keyword cannot be missed here.

export type VariableKind = "bool" | "number" | "text";

export interface Variable {
  name: string;
  kind: VariableKind;
  /** A board predicate: heads-up postflop only. */
  board: boolean;
  doc: string;
}

export const STREETS = ["preflop", "flop", "turn", "river"] as const;
export const ACTIONS = ["fold", "check", "call", "bet", "raise"] as const;
export const WAGER_ACTIONS = ["bet", "raise"] as const;
/** Effects that take an action and a `[...]` size list (`remove` takes no list). */
export const LIST_EFFECTS = ["add", "replace", "force"] as const;
export const EFFECTS = ["add", "remove", "replace", "force", "jam", "merge", "checkdown"] as const;
export const DECLARATIONS = ["param", "define"] as const;
export const CONTROL = ["when", "if", "else", "in", "within"] as const;
export const CONSTANTS = ["true", "false"] as const;
/** Size literals that are words rather than numbers. */
export const SIZE_WORDS = ["a", "e", "min"] as const;

/** Every reserved word but the variables. */
export const KEYWORDS: readonly string[] = [
  ...DECLARATIONS,
  ...CONTROL,
  ...STREETS,
  ...EFFECTS,
  ...ACTIONS,
  ...CONSTANTS,
  ...SIZE_WORDS,
];

/** Fixed position names (docs/nlh-input-v1.jp.md §5), the values of `position`-like text variables. */
export const POSITIONS = ["UTG", "UTG1", "UTG2", "LJ", "HJ", "CO", "BTN", "SB", "BB"] as const;
export const RANKS = ["A", "K", "Q", "J", "T", "9", "8", "7", "6", "5", "4", "3", "2"] as const;

const v = (name: string, kind: VariableKind, doc: string, board = false): Variable => ({ name, kind, board, doc });

export const VARIABLES: readonly Variable[] = [
  v("aggressions", "number", "Bets and raises on the current street, short all-ins included; forced bets are not."),
  v("raises", "number", "Same count as aggressions: bets and raises on the current street."),
  v("unopened", "bool", "aggressions == 0."),
  v("players", "number", "Players not folded, all-in players included. Always 2 in HU Postflop."),
  v("position", "text", "The actor's fixed position name, e.g. BTN."),
  v("in_position", "bool", "Preflop: the BTN. Postflop: the last unfolded seat in postflop order."),
  v("spr", "number", "The smaller of the actor's and the deepest opponent's remaining stack, over the pot."),
  v("pot", "number", "The current pot in BB, dead money and this street's wagers included."),
  v("to_call", "number", "The street's largest wager minus the actor's, in BB."),
  v("facing_pct", "number", "to_call / pot * 100; 0 when the pot is 0."),
  v("cbet", "bool", "Postflop, unopened, and the actor was the previous street's last aggressor."),
  v("donk", "bool", "Postflop, unopened, and another seat was the previous street's last aggressor."),
  v("limpers", "number", "Preflop calls before the first raise."),
  v("flats", "number", "Calls since the last preflop raise."),
  v("squeeze", "bool", "Preflop with aggressions > 0 && flats > 0."),
  v("open_cold_calls", "number", "With one preflop aggression, non-BB seats whose first voluntary action was a call."),
  v("preflop_participant", "bool", "The actor called, bet or raised preflop (forced posts do not count)."),
  v("in_position_to_last_aggressor", "bool", "The actor acts after the last preflop raiser postflop."),
  v("last_preflop_aggressor_position", "text", 'The last preflop raiser\'s position; "" with no raise.'),
  v("board_cards", "number", "Cards on the board (3, 4 or 5).", true),
  v("board_suits", "number", "Distinct suits on the board.", true),
  v("board_ranks", "number", "Distinct ranks on the board.", true),
  v("straight_ranks", "number", "Most distinct board ranks inside one five-rank straight window.", true),
  v("paired", "bool", "board_ranks < board_cards.", true),
  v("monotone", "bool", "board_suits == 1.", true),
  v("two_tone", "bool", "board_suits == 2.", true),
  v("rainbow", "bool", "board_suits == board_cards.", true),
  v("flush_possible", "bool", "Three or more cards of one suit.", true),
  v("straight_possible", "bool", "straight_ranks >= 3: two hole cards can make a straight.", true),
  v("high_card", "text", "The board's highest rank, A to 2.", true),
  v("low_card", "text", "The board's lowest rank (A counts high).", true),
];

export const VARIABLE_BY_NAME: ReadonlyMap<string, Variable> = new Map(VARIABLES.map((variable) => [variable.name, variable]));

/** Values a text variable compares with. */
export function textValues(name: string): readonly string[] {
  if (name === "position" || name === "last_preflop_aggressor_position") return POSITIONS;
  if (name === "high_card" || name === "low_card") return RANKS;
  return [];
}

export const KEYWORD_DOCS: Readonly<Record<string, string>> = {
  param: "param NAME = VALUE: a one-token value the GUI can override in [tree.params]. Numeric sizes are BB; use 33% for a pot percentage or 300c for chips. Comment lines directly above it become its description.",
  define: "define NAME = CONDITION: a named condition, usable after its declaration.",
  when: "when CONDITION { ... }: the body applies only where the condition holds; nested conditions AND together.",
  if: "if A { ... } else if B { ... } else { ... }: exclusive branches.",
  else: "else: the branch taken when no earlier if / else if matched.",
  in: "VARIABLE in [a, b]: true when the variable equals one of the literals.",
  within: "merge ACTION within DIFF: the width two kept sizes must exceed.",
  preflop: "Rules for preflop decisions. The heads-up postflop solver keeps them but they have no effect there.",
  flop: "Rules for flop decisions.",
  turn: "Rules for turn decisions.",
  river: "Rules for river decisions.",
  add: "add ACTION [sizes]: adds the legal candidates of that kind.",
  remove: "remove ACTION: removes every candidate of that kind.",
  replace: "replace ACTION [sizes]: removes that kind, then adds the given candidates.",
  force: "force ACTION [sizes]: the menu becomes only these candidates.",
  jam: "jam bet|raise >= SIZE (or >): candidates whose target reaches the threshold become all-in. Never adds all-in by itself.",
  merge: "merge bet|raise within DIFF: from the largest target down, removes candidates within DIFF of the last kept one.",
  checkdown: "checkdown: the menu becomes check (or fold when facing a bet).",
  fold: "fold: give up the hand.",
  check: "check: pass when nothing is owed.",
  call: "call: match the current wager.",
  bet: "bet: the first wager of a street.",
  raise: "raise: a wager over an existing one; preflop opens are raises.",
  true: "Boolean literal, for == / != against a bool variable.",
  false: "Boolean literal, for == / != against a bool variable.",
  a: "a: all-in (the actor's maximum reachable street amount M).",
  e: "e: the geometric size reaching all-in over the streets left; Ne over N streets.",
  min: "min: the minimum full bet or raise.",
};

export interface SizeExample {
  label: string;
  doc: string;
}

/** Common size literals, offered in size lists and jam thresholds. */
export const SIZE_EXAMPLES: readonly SizeExample[] = [
  { label: "33%", doc: "33% of the pot after calling." },
  { label: "50%", doc: "Half the pot after calling." },
  { label: "75%", doc: "75% of the pot after calling." },
  { label: "100%", doc: "The pot after calling." },
  { label: "150%", doc: "150% of the pot after calling." },
  { label: "a", doc: "All-in." },
  { label: "e", doc: "Geometric to all-in over the streets left." },
  { label: "2e", doc: "Geometric to all-in over two streets." },
  { label: "min", doc: "The minimum full bet or raise." },
  { label: "3x", doc: "Three times the street's largest wager." },
  { label: "2.5bb", doc: "A street target of 2.5 BB." },
  { label: "300c", doc: "A street target of 300 chips." },
  { label: "85%stack", doc: "85% of the actor's maximum reachable amount M." },
  { label: "80%effective", doc: "80% of the effective stack against unfolded opponents." },
];

/** Common merge widths: amounts, not targets. */
export const WIDTH_EXAMPLES: readonly SizeExample[] = [
  { label: "10%", doc: "10% of the pot after calling." },
  { label: "0.5bb", doc: "Half a big blind." },
  { label: "5%stack", doc: "5% of the actor's maximum reachable amount M." },
  { label: "5%effective", doc: "5% of the effective stack." },
];

const SIZE_LITERAL = /^(?:\d+(?:\.\d+)?(?:x|e|bb|c|%|%stack|%effective)?|a|e|min)$/;

/** Whether a bare token reads as a size literal. */
export function isSizeLiteral(word: string): boolean {
  return SIZE_LITERAL.test(word);
}

/** A one-line explanation of a size literal, or null. */
export function describeSize(word: string): string | null {
  if (!isSizeLiteral(word)) return null;
  if (word in KEYWORD_DOCS) return KEYWORD_DOCS[word] ?? null;
  const match = /^(\d+(?:\.\d+)?)(x|e|bb|c|%|%stack|%effective)?$/.exec(word);
  if (!match) return null;
  const [, number, suffix] = match;
  switch (suffix) {
    case "x":
      return `${number} times the street's largest wager.`;
    case "e":
      return `Geometric to all-in over ${number} streets.`;
    case "bb":
      return `A street target of ${number} BB (as a merge width: ${number} BB).`;
    case "c":
      return `A street target of ${number} chips (as a merge width: ${number} chips).`;
    case "%":
      return `${number}% of the pot after calling (as a merge width: that share of the pot).`;
    case "%stack":
      return `${number}% of the actor's maximum reachable amount M.`;
    case "%effective":
      return `${number}% of the effective stack against unfolded opponents.`;
    default:
      return `A street target of ${number} BB (as a merge width: ${number} BB).`;
  }
}
