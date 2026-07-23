//! GGPoker tournament hand-history and summary parsing / loading.
//!
//! Two independent line-oriented parsers:
//! - [`parse_hand_history`] reads a `*.txt` hand-history file (a sequence
//!   of hand blocks separated by blank lines) into a `Vec<Hand>`.
//! - [`parse_summary`] reads a `*.txt` tournament-summary file into one
//!   [`TournamentSummary`].
//!
//! Both parsers are strict: any line that doesn't match the known GGPoker
//! shapes is a [`ParseError`] naming the line number and content, rather
//! than being silently skipped.
//!
//! [`load_dir`] / [`load_files`] walk a directory of such files and merge
//! them by tournament id into a [`TournamentSet`], deduplicating hands and
//! preferring the summary's name/game where both are present.

pub mod analysis;
mod hand_history;
mod load;
mod model;
mod summary;
mod util;

pub use hand_history::parse_hand_history;
pub use load::{LoadError, Tournament, TournamentSet, load_dir, load_files};
pub use model::{
    Action, ActionRecord, DateTime, GameType, Hand, ParseError, PotSummary, Seat, SeatOutcome,
    SeatResult, TournamentSummary, Winnings,
};
pub use summary::parse_summary;
