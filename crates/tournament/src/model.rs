//! Data model shared by the hand-history and summary parsers: game/street
//! metadata, a dependency-free `DateTime`, and the `Hand` / `TournamentSummary`
//! structs themselves.

use cards::Card;
use cards::Street;

/// A GGPoker game variant, as it appears in hand-history headers and
/// summary files. Matched as a suffix of the "name + game" segment, in the
/// order `AofHoldem` (`"AoF Hold'em No Limit"`) before `Holdem`
/// (`"Hold'em No Limit"`) before `Omaha` (`"Omaha No Limit"`), since the AoF
/// variant's text is itself a superstring ending in the plain Hold'em text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameType {
    Holdem,
    AofHoldem,
    Omaha,
}

impl GameType {
    /// The known game-type suffix strings, in match-priority order (see
    /// [`GameType`] docs for why the order matters).
    const SUFFIXES: [(&'static str, GameType); 3] = [
        ("AoF Hold'em No Limit", GameType::AofHoldem),
        ("Hold'em No Limit", GameType::Holdem),
        ("Omaha No Limit", GameType::Omaha),
    ];

    /// Matches `s` exactly against one of the known game strings (used for
    /// the summary file's already-isolated game field).
    pub(crate) fn from_exact(s: &str) -> Option<GameType> {
        Self::SUFFIXES
            .iter()
            .find(|(suffix, _)| *suffix == s)
            .map(|(_, game)| *game)
    }

    /// Splits `s` into `(name, game)` by matching a known game string as a
    /// suffix, trying the suffixes in priority order. Returns the name part
    /// trimmed of trailing whitespace.
    pub(crate) fn split_suffix(s: &str) -> Option<(GameType, &str)> {
        Self::SUFFIXES
            .iter()
            .find_map(|(suffix, game)| s.strip_suffix(suffix).map(|name| (*game, name.trim_end())))
    }
}

/// A timestamp as it appears in GGPoker files: no timezone, second
/// resolution. Field declaration order (year, month, day, hour, minute,
/// second) is also the derived `Ord` comparison order, so `DateTime` sorts
/// chronologically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl std::fmt::Display for DateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:04}/{:02}/{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// One seat at the table, as listed before hole cards are dealt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub seat: u8,
    pub player: String,
    pub chips: u64,
}

/// A single player action (or forced post / pot payout), in the order it
/// occurred in the hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    PostAnte(u64),
    PostSmallBlind(u64),
    PostBigBlind(u64),
    Fold,
    Check,
    Call {
        amount: u64,
        all_in: bool,
    },
    Bet {
        amount: u64,
        all_in: bool,
    },
    Raise {
        by: u64,
        to: u64,
        all_in: bool,
    },
    Show {
        cards: Vec<Card>,
        description: Option<String>,
    },
    UncalledBetReturn {
        amount: u64,
    },
    Collect {
        amount: u64,
    },
}

/// One entry in a hand's flat action log. `player` is the acting player for
/// `Fold`/`Check`/.../`Show`, the returned-to player for
/// `UncalledBetReturn`, and the collecting player for `Collect`. Posts are
/// recorded at `Street::Preflop`; collects are recorded at whatever street
/// play reached (the last street marker seen, or `Preflop` if the hand
/// never saw a flop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRecord {
    pub street: Street,
    pub player: String,
    pub action: Action,
}

/// Total pot breakdown from the `*** SUMMARY ***` section. In this corpus
/// `rake`/`jackpot`/`bingo`/`fortune`/`tax` are always 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PotSummary {
    pub total: u64,
    pub rake: u64,
    pub jackpot: u64,
    pub bingo: u64,
    pub fortune: u64,
    pub tax: u64,
}

/// How a seat's hand ended, from its `Seat <n>: ...` summary line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeatOutcome {
    /// `folded before Flop` / `folded on the <Street>`. `street` is the
    /// last street *seen* when the fold happened (`Preflop` for "before
    /// Flop"). `didnt_bet` is set for the `(didn't bet)` suffix.
    Folded { street: Street, didnt_bet: bool },
    /// `won (n)` or `collected (n)`, optionally preceded by
    /// `showed [..] and collected (n)` (cards `Some` in that case).
    Collected {
        amount: u64,
        cards: Option<Vec<Card>>,
    },
    /// `showed [..] and won (n)`, optionally followed by ` with <desc>`.
    ShowedWon {
        amount: u64,
        cards: Vec<Card>,
        description: Option<String>,
    },
    /// `showed [..] and lost with <desc>`.
    ShowedLost {
        cards: Vec<Card>,
        description: String,
    },
}

/// One seat's summary-line result: position annotations plus its outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatResult {
    pub seat: u8,
    pub player: String,
    pub is_button: bool,
    pub is_small_blind: bool,
    pub is_big_blind: bool,
    pub outcome: SeatOutcome,
}

/// A fully parsed hand from a hand-history file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hand {
    pub id: u64,
    pub tournament_id: u64,
    pub tournament_name: String,
    pub game: GameType,
    pub level: u32,
    pub small_blind: u64,
    pub big_blind: u64,
    pub ante: u64,
    pub played_at: DateTime,
    pub table: String,
    pub table_size: u8,
    pub button_seat: u8,
    pub seats: Vec<Seat>,
    /// Hero's hole cards: 2 for Hold'em/AoF Hold'em, 4 for Omaha. Other
    /// seats' hole cards are never visible pre-showdown in this corpus.
    pub hero_cards: Vec<Card>,
    pub actions: Vec<ActionRecord>,
    /// Community cards dealt so far; length 0, 3, 4, or 5.
    pub board: Vec<Card>,
    pub pot: PotSummary,
    pub results: Vec<SeatResult>,
}

/// A cash-like prize amount, tagged by the currency/kind it was denominated
/// in. Dollar-shaped amounts (`Dollars`, `Coin`, `TournamentDollars`,
/// `TicketEntry`) are stored as integer cents; `Chips` is a raw chip count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Winnings {
    /// `$<x>`.
    Dollars(u64),
    /// `C$<x>` (GGPoker's play-money "coin" currency).
    Coin(u64),
    /// `T$<x>` (tournament dollars).
    TournamentDollars(u64),
    /// `$<x> Entry` (a ticket into another tournament).
    TicketEntry(u64),
    /// `<n> chips`.
    Chips(u64),
}

/// A fully parsed tournament summary file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentSummary {
    pub id: u64,
    pub name: String,
    pub game: GameType,
    pub buy_in_cents: u64,
    pub fee_cents: u64,
    pub players: u32,
    pub prize_pool_cents: u64,
    pub started_at: DateTime,
    pub hero_place: u32,
    pub hero_prize: Winnings,
    pub re_entries: u32,
    pub total_received: Winnings,
}

/// A line-level parse error: which line, its content, and what went wrong.
/// Both `parse_hand_history` and `parse_summary` report errors this way.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message} (line content: {content:?})")]
pub struct ParseError {
    /// 1-based line number within the parsed text.
    pub line: usize,
    /// The offending line's full content.
    pub content: String,
    /// Human-readable description of the problem.
    pub message: String,
}

impl ParseError {
    pub(crate) fn at(line: usize, content: &str, message: impl Into<String>) -> Self {
        ParseError {
            line,
            content: content.to_string(),
            message: message.into(),
        }
    }
}
