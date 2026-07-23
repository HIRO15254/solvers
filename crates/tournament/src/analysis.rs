//! All-in EV ("luck") analysis: how many big blinds Hero has run above or
//! below equity expectation across every hand where Hero went all-in and
//! reached showdown.
//!
//! This is the standard poker-tracker "All-in Adjusted" metric. For the
//! common heads-up all-in the `equity × eligible-pot` comparison is exact;
//! for multiway all-ins with layered side pots (more than two players
//! contest the main pot Hero is eligible for) it is the usual
//! approximation trackers use, since equity is computed once against the
//! full multiway field rather than pot-by-pot against each side pot's
//! contestants.
//!
//! The core pieces — contribution reconstruction and exact-enumeration
//! equity — are exposed as free functions so they can be unit tested
//! directly against a real corpus hand (see the `tests` module below) and
//! reused by `examples/allin_ev.rs`, which is a thin loader/printer around
//! [`analyze`].

use std::collections::HashMap;

use cards::{ALL_CARDS, Card, HandRank, Street, rank_of};

use crate::TournamentSet;
use crate::model::{Action, GameType, Hand};

/// Hero's all-in luck for one qualifying hand (Hero all-in, reached
/// showdown with at least one opponent shown).
#[derive(Debug, Clone, PartialEq)]
pub struct AllInLuck {
    pub tournament_id: u64,
    pub tournament_name: String,
    pub hand_id: u64,
    pub game: GameType,
    /// Hero's hole cards, as shown at showdown.
    pub hero_cards: Vec<Card>,
    /// P(Hero wins), ties counted as `1/k`, over exact enumeration of the
    /// remaining board from Hero's all-in street.
    pub hero_equity: f64,
    /// `Σ_p min(contrib_p, hero_contrib)`: the main pot Hero contests.
    pub eligible_pot: u64,
    /// Sum of Hero's `Action::Collect` amounts in the hand (0 if Hero lost).
    pub actual_collected: u64,
    pub big_blind: u64,
    /// How many board cards were unknown at Hero's all-in moment: 5 for a
    /// preflop all-in down to 0 for a river all-in (deterministic).
    pub cards_to_come: usize,
    pub luck_chips: f64,
    pub luck_bb: f64,
}

/// Counts of hands that didn't qualify, plus the contribution-reconstruction
/// sanity check (should always be zero; a nonzero count means the
/// reconstruction algorithm and the loader's parsed pot disagree).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AllInEvStats {
    pub hands_scanned: usize,
    pub not_all_in_showdown: usize,
    pub excluded_bb_zero: usize,
    pub contribution_mismatches: usize,
}

/// Full analysis result: one [`AllInLuck`] per qualifying hand, plus scan
/// diagnostics.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AllInEvReport {
    pub hands: Vec<AllInLuck>,
    pub stats: AllInEvStats,
}

/// Runs the all-in EV analysis over every hand in `set`. See the module
/// docs for the metric definition.
pub fn analyze(set: &TournamentSet) -> AllInEvReport {
    let mut report = AllInEvReport::default();

    for tournament in &set.tournaments {
        for hand in &tournament.hands {
            report.stats.hands_scanned += 1;

            let shown = shown_hands(hand);
            let Some(all_in_street) = hero_all_in_street(hand) else {
                report.stats.not_all_in_showdown += 1;
                continue;
            };
            if !shown.contains_key("Hero") || shown.len() < 2 {
                report.stats.not_all_in_showdown += 1;
                continue;
            }
            if hand.big_blind == 0 {
                report.stats.excluded_bb_zero += 1;
                continue;
            }

            let total_contrib = reconstruct_contributions(hand);
            if total_contrib.values().sum::<u64>() != hand.pot.total {
                report.stats.contribution_mismatches += 1;
            }

            let hero_contrib = *total_contrib.get("Hero").unwrap_or(&0);
            let eligible = eligible_pot(&total_contrib, hero_contrib);

            let known_len = known_board_len(all_in_street);
            let known_board = &hand.board[..known_len];
            let cards_to_come = 5 - known_len;

            let equity = hero_equity(hand.game, &shown, known_board, cards_to_come);

            let actual: u64 = hand
                .actions
                .iter()
                .filter_map(|record| match &record.action {
                    Action::Collect { amount } if record.player == "Hero" => Some(*amount),
                    _ => None,
                })
                .sum();

            let expected = equity * eligible as f64;
            let luck_chips = actual as f64 - expected;
            let luck_bb = luck_chips / hand.big_blind as f64;

            report.hands.push(AllInLuck {
                tournament_id: tournament.id,
                tournament_name: tournament.name.clone(),
                hand_id: hand.id,
                game: hand.game,
                hero_cards: hand.hero_cards.clone(),
                hero_equity: equity,
                eligible_pot: eligible,
                actual_collected: actual,
                big_blind: hand.big_blind,
                cards_to_come,
                luck_chips,
                luck_bb,
            });
        }
    }

    report
}

/// The street of Hero's all-in action: the street of the first
/// `ActionRecord` for Hero whose action is a `Call`/`Bet`/`Raise` with
/// `all_in == true`. `None` if Hero never went all-in in this hand.
fn hero_all_in_street(hand: &Hand) -> Option<Street> {
    hand.actions.iter().find_map(|record| {
        if record.player != "Hero" {
            return None;
        }
        let all_in = match &record.action {
            Action::Call { all_in, .. }
            | Action::Bet { all_in, .. }
            | Action::Raise { all_in, .. } => *all_in,
            _ => false,
        };
        all_in.then_some(record.street)
    })
}

/// Builds `player -> shown hole cards` from every `Action::Show` record in
/// the hand.
fn shown_hands(hand: &Hand) -> HashMap<String, Vec<Card>> {
    let mut shown = HashMap::new();
    for record in &hand.actions {
        if let Action::Show { cards, .. } = &record.action {
            shown.insert(record.player.clone(), cards.clone());
        }
    }
    shown
}

/// Maps a street to the number of board cards known at that street's start
/// (i.e. the board-prefix length "known" at the moment a player commits on
/// that street).
fn known_board_len(street: Street) -> usize {
    match street {
        Street::Preflop => 0,
        Street::Flop => 3,
        Street::Turn => 4,
        Street::River => 5,
    }
}

/// Reconstructs each player's total chip contribution to the pot for
/// `hand`, by walking `hand.actions` in order and maintaining a per-street
/// "committed this street" counter that resets whenever the street changes.
/// Antes are tracked in the total only (raise math is over the blind
/// level, not antes). `Σ` of the result should always equal
/// `hand.pot.total`; callers that care should check this (see
/// [`analyze`]'s `contribution_mismatches` counter).
pub fn reconstruct_contributions(hand: &Hand) -> HashMap<String, u64> {
    let mut total_contrib: HashMap<String, u64> = HashMap::new();
    let mut street_commit: HashMap<String, u64> = HashMap::new();
    let mut current_street: Option<Street> = None;

    for record in &hand.actions {
        if current_street != Some(record.street) {
            street_commit.clear();
            current_street = Some(record.street);
        }

        match &record.action {
            Action::PostAnte(amount) => {
                *total_contrib.entry(record.player.clone()).or_insert(0) += *amount;
            }
            Action::PostSmallBlind(amount) | Action::PostBigBlind(amount) => {
                *total_contrib.entry(record.player.clone()).or_insert(0) += *amount;
                *street_commit.entry(record.player.clone()).or_insert(0) += *amount;
            }
            Action::Bet { amount, .. } | Action::Call { amount, .. } => {
                *total_contrib.entry(record.player.clone()).or_insert(0) += *amount;
                *street_commit.entry(record.player.clone()).or_insert(0) += *amount;
            }
            Action::Raise { to, .. } => {
                let commit = street_commit.entry(record.player.clone()).or_insert(0);
                let add = *to - *commit;
                *commit = *to;
                *total_contrib.entry(record.player.clone()).or_insert(0) += add;
            }
            Action::UncalledBetReturn { amount } => {
                if let Some(c) = total_contrib.get_mut(&record.player) {
                    *c -= *amount;
                }
                if let Some(c) = street_commit.get_mut(&record.player) {
                    *c = c.saturating_sub(*amount);
                }
            }
            Action::Fold | Action::Check | Action::Show { .. } | Action::Collect { .. } => {}
        }
    }

    total_contrib
}

/// `Σ_p min(contrib_p, hero_contrib)`: the pot Hero is eligible to win
/// (main pot, from Hero's point of view) given everyone's total
/// contributions.
fn eligible_pot(total_contrib: &HashMap<String, u64>, hero_contrib: u64) -> u64 {
    total_contrib.values().map(|&c| c.min(hero_contrib)).sum()
}

/// Every 2-card subset of a 4-card Omaha hand, as index pairs into that
/// hand's cards.
const OMAHA_HOLE_PAIRS: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

/// Every 3-card subset of a 5-card board, as index triples.
const BOARD_TRIPLES: [(usize, usize, usize); 10] = [
    (0, 1, 2),
    (0, 1, 3),
    (0, 1, 4),
    (0, 2, 3),
    (0, 2, 4),
    (0, 3, 4),
    (1, 2, 3),
    (1, 2, 4),
    (1, 3, 4),
    (2, 3, 4),
];

/// An Omaha player's best hand rank: the max over all `C(4,2) × C(5,3) =
/// 60` ways to use exactly 2 of the 4 hole cards and 3 of the 5 board
/// cards.
fn omaha_best_rank(hole: &[Card], board: &[Card]) -> HandRank {
    debug_assert_eq!(hole.len(), 4, "Omaha hole cards must be exactly 4");
    debug_assert_eq!(board.len(), 5, "board must be exactly 5 cards");
    OMAHA_HOLE_PAIRS
        .iter()
        .flat_map(|&(hi, hj)| {
            BOARD_TRIPLES.iter().map(move |&(bi, bj, bk)| {
                rank_of([hole[hi], hole[hj], board[bi], board[bj], board[bk]])
            })
        })
        .max()
        .expect("60 fixed combinations always yield at least one rank")
}

/// A showdown player's best hand rank on `board` (5 cards), by game type:
/// best-5-of-7 for Hold'em / AoF Hold'em, best-of-60 for Omaha.
fn showdown_rank(game: GameType, hole: &[Card], board: &[Card]) -> HandRank {
    match game {
        GameType::Omaha => omaha_best_rank(hole, board),
        GameType::Holdem | GameType::AofHoldem => {
            rank_of(hole.iter().copied().chain(board.iter().copied()))
        }
    }
}

/// Calls `f` once per `k`-combination of `deck` (as a slice reused across
/// calls, no per-combination allocation), in lexicographic order of
/// indices.
fn each_combination(
    deck: &[Card],
    k: usize,
    buf: &mut Vec<Card>,
    start: usize,
    f: &mut impl FnMut(&[Card]),
) {
    if buf.len() == k {
        f(buf);
        return;
    }
    for (i, &card) in deck.iter().enumerate().skip(start) {
        buf.push(card);
        each_combination(deck, k, buf, i + 1, f);
        buf.pop();
    }
}

/// Computes Hero's equity share of `hero_eligible_pot` (see [`analyze`]) by
/// exact enumeration of the `cards_to_come` remaining board cards from
/// `known_board` (Hero's all-in street's board prefix — later actual board
/// cards are ignored). `shown` maps every showdown player to their
/// revealed hole cards (2 for Hold'em/AoF, 4 for Omaha). Ties split `1/k`
/// among the tied winners. `cards_to_come == 0` enumerates exactly one
/// (empty) completion, i.e. the deterministic actual result.
pub fn hero_equity(
    game: GameType,
    shown: &HashMap<String, Vec<Card>>,
    known_board: &[Card],
    cards_to_come: usize,
) -> f64 {
    debug_assert_eq!(
        known_board.len() + cards_to_come,
        5,
        "known board + cards to come must always total 5"
    );

    let mut dead: Vec<Card> = shown.values().flatten().copied().collect();
    dead.extend_from_slice(known_board);
    let deck: Vec<Card> = ALL_CARDS
        .into_iter()
        .filter(|c| !dead.contains(c))
        .collect();

    let players: Vec<(&str, &[Card])> = shown
        .iter()
        .map(|(name, cards)| (name.as_str(), cards.as_slice()))
        .collect();

    let mut hero_share_sum = 0.0f64;
    let mut completions = 0u64;
    let mut buf: Vec<Card> = Vec::with_capacity(cards_to_come);

    each_combination(&deck, cards_to_come, &mut buf, 0, &mut |completion| {
        let mut full_board = [Card::from_index(0); 5];
        full_board[..known_board.len()].copy_from_slice(known_board);
        full_board[known_board.len()..].copy_from_slice(completion);

        let mut best_rank: Option<HandRank> = None;
        let mut best_count = 0usize;
        let mut hero_rank: Option<HandRank> = None;
        for &(name, hole) in &players {
            let rank = showdown_rank(game, hole, &full_board);
            if name == "Hero" {
                hero_rank = Some(rank);
            }
            match best_rank {
                Some(b) if rank < b => {}
                Some(b) if rank == b => best_count += 1,
                _ => {
                    best_rank = Some(rank);
                    best_count = 1;
                }
            }
        }

        if hero_rank == best_rank {
            hero_share_sum += 1.0 / best_count as f64;
        }
        completions += 1;
    });

    if completions == 0 {
        return 0.0;
    }
    hero_share_sum / completions as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn corpus_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/tournaments")
    }

    fn find_hand(set: &TournamentSet, hand_id: u64) -> Hand {
        set.tournaments
            .iter()
            .flat_map(|t| t.hands.iter())
            .find(|h| h.id == hand_id)
            .cloned()
            .unwrap_or_else(|| panic!("hand {hand_id} not found in corpus"))
    }

    /// The concrete validation hand from the spec: `TM6026970026`, a Daily
    /// Hyper hand where Hero calls all-in on the flop for its last 2,547
    /// chips, three-way to showdown against a flush and two pair.
    #[test]
    fn validation_hand_tm6026970026() {
        let set = crate::load_dir(&corpus_path()).expect("corpus should load");
        let hand = find_hand(&set, 6026970026);

        let total_contrib = reconstruct_contributions(&hand);
        let sum: u64 = total_contrib.values().sum();
        assert_eq!(sum, 47_971, "Σ total_contrib should equal pot.total");
        assert_eq!(sum, hand.pot.total);

        let hero_contrib = total_contrib["Hero"];
        assert_eq!(hero_contrib, 4_247);

        let eligible = eligible_pot(&total_contrib, hero_contrib);
        assert_eq!(eligible, 13_441);

        let all_in_street = hero_all_in_street(&hand).expect("Hero goes all-in on the flop");
        assert_eq!(all_in_street, Street::Flop);

        let shown = shown_hands(&hand);
        assert_eq!(shown.len(), 3, "3-way showdown: Hero, b8ca1cf6, 4b00a43");
        assert!(shown.contains_key("Hero"));

        let known_len = known_board_len(all_in_street);
        assert_eq!(known_len, 3);
        let known_board = &hand.board[..known_len];
        // [Js 3s 3c], the flop.
        assert_eq!(known_board.len(), 3);

        let equity = hero_equity(hand.game, &shown, known_board, 5 - known_len);
        assert!(
            (0.0..1.0).contains(&equity),
            "equity must be a fraction, got {equity}"
        );
        // NOTE (spec deviation): the spec's draft expected this hand's Hero
        // equity to be "roughly 15-30%" (< 0.5, an underdog). Exact
        // enumeration gives ~53.49%, which is also independently verified
        // by a standalone brute-force script against `cards::rank_of`
        // outside this crate. This is the game-theoretically correct
        // number: Hero's [5c Jc] on [Js 3s 3c] already holds two pair
        // (Jacks and Threes), which beats [Tc Ts]'s two pair (Tens and
        // Threes) outright; Hero's only live danger is [9s Qs]'s four-card
        // flush draw (~38% by the river), so Hero is correctly a solid
        // favourite, not an underdog. We assert the verified value here
        // rather than the spec's estimate.
        assert!(
            (0.53..0.54).contains(&equity),
            "Hero's 3-way flop equity should be ~53.49% (verified independently), got {equity}"
        );
        assert!((equity - 0.5348837209302325).abs() < 1e-9);
    }

    /// A minimal synthetic hand: heads-up, Hero shoves preflop and is
    /// called, both cards shown, no board dealt. Exercises the
    /// `cards_to_come == 5` path and a simple contribution reconstruction
    /// (blinds, a raise-to-allin, a call, no uncalled-bet return since it's
    /// a jam-call).
    fn heads_up_preflop_allin(hero_cards: &str, villain_cards: &str) -> Hand {
        let hero: Vec<Card> = hero_cards
            .split_whitespace()
            .map(|c| c.parse().unwrap())
            .collect();
        let villain: Vec<Card> = villain_cards
            .split_whitespace()
            .map(|c| c.parse().unwrap())
            .collect();

        Hand {
            id: 1,
            tournament_id: 1,
            tournament_name: "Test".to_string(),
            game: GameType::Holdem,
            level: 1,
            small_blind: 50,
            big_blind: 100,
            ante: 0,
            played_at: crate::model::DateTime {
                year: 2026,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
            },
            table: "1".to_string(),
            table_size: 2,
            button_seat: 1,
            seats: vec![
                crate::model::Seat {
                    seat: 1,
                    player: "Hero".to_string(),
                    chips: 1_000,
                },
                crate::model::Seat {
                    seat: 2,
                    player: "Villain".to_string(),
                    chips: 1_000,
                },
            ],
            hero_cards: hero.clone(),
            actions: vec![
                crate::model::ActionRecord {
                    street: Street::Preflop,
                    player: "Hero".to_string(),
                    action: Action::PostSmallBlind(50),
                },
                crate::model::ActionRecord {
                    street: Street::Preflop,
                    player: "Villain".to_string(),
                    action: Action::PostBigBlind(100),
                },
                crate::model::ActionRecord {
                    street: Street::Preflop,
                    player: "Hero".to_string(),
                    action: Action::Raise {
                        by: 950,
                        to: 1_000,
                        all_in: true,
                    },
                },
                crate::model::ActionRecord {
                    street: Street::Preflop,
                    player: "Villain".to_string(),
                    action: Action::Call {
                        amount: 900,
                        all_in: true,
                    },
                },
                crate::model::ActionRecord {
                    street: Street::Preflop,
                    player: "Hero".to_string(),
                    action: Action::Show {
                        cards: hero,
                        description: None,
                    },
                },
                crate::model::ActionRecord {
                    street: Street::Preflop,
                    player: "Villain".to_string(),
                    action: Action::Show {
                        cards: villain,
                        description: None,
                    },
                },
                crate::model::ActionRecord {
                    street: Street::Preflop,
                    player: "Hero".to_string(),
                    action: Action::Collect { amount: 2_000 },
                },
            ],
            board: vec![],
            pot: crate::model::PotSummary {
                total: 2_000,
                ..Default::default()
            },
            results: vec![],
        }
    }

    #[test]
    fn contribution_reconstruction_heads_up_allin() {
        let hand = heads_up_preflop_allin("As Ad", "Ks Kd");
        let total_contrib = reconstruct_contributions(&hand);
        assert_eq!(total_contrib["Hero"], 1_000);
        assert_eq!(total_contrib["Villain"], 1_000);
        let sum: u64 = total_contrib.values().sum();
        assert_eq!(sum, hand.pot.total);
    }

    #[test]
    fn heads_up_aces_vs_kings_equity_is_favourite() {
        let hand = heads_up_preflop_allin("As Ad", "Ks Kd");
        let shown = shown_hands(&hand);
        let equity = hero_equity(hand.game, &shown, &[], 5);
        // AA vs KK preflop is roughly 80-82% in the classic tables.
        assert!(
            (0.78..0.84).contains(&equity),
            "AA vs KK should be a big favourite, got {equity}"
        );
    }

    #[test]
    fn known_board_lengths() {
        assert_eq!(known_board_len(Street::Preflop), 0);
        assert_eq!(known_board_len(Street::Flop), 3);
        assert_eq!(known_board_len(Street::Turn), 4);
        assert_eq!(known_board_len(Street::River), 5);
    }

    /// Checks the contribution-reconstruction sanity invariant
    /// (`Σ total_contrib == pot.total`) across the whole corpus. This is
    /// deliberately decoupled from [`analyze`]'s equity enumeration (which
    /// is release-build-speed only, see `examples/allin_ev.rs`) so this
    /// test stays fast under `cargo test`'s debug build.
    #[test]
    fn contribution_reconstruction_balances_across_corpus() {
        let set = crate::load_dir(&corpus_path()).expect("corpus should load");
        let mut hands_scanned = 0usize;
        let mut mismatches = 0usize;
        for tournament in &set.tournaments {
            for hand in &tournament.hands {
                hands_scanned += 1;
                let total_contrib = reconstruct_contributions(hand);
                let sum: u64 = total_contrib.values().sum();
                if sum != hand.pot.total {
                    mismatches += 1;
                }
            }
        }
        assert_eq!(hands_scanned, 2_795);
        assert_eq!(
            mismatches, 0,
            "every hand's reconstructed contributions should sum to its pot total"
        );
    }
}
