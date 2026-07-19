//! Integration test against the real GGPoker corpus committed at
//! `data/tournaments/` (126 hand-history files / 2,795 hands, 124 summary
//! files). Not `#[ignore]`d: parsing ~5 MB of text is fast enough to run
//! on every `cargo test`.
//!
//! The exact counts here were derived by exhaustively enumerating the
//! corpus once (see the crate's design spec); if the corpus ever changes,
//! these should be recomputed rather than loosened.

use std::collections::HashSet;
use std::path::PathBuf;

use tournament::{Action, GameType, load_dir};

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/tournaments")
}

#[test]
fn corpus_loads_and_satisfies_invariants() {
    let set = load_dir(&corpus_path()).expect("the full corpus should load with no leniency hacks");

    let total_hands: usize = set.tournaments.iter().map(|t| t.hands.len()).sum();
    let total_summaries = set
        .tournaments
        .iter()
        .filter(|t| t.summary.is_some())
        .count();

    assert_eq!(total_hands, 2_795, "total hand count across the corpus");
    assert_eq!(
        total_summaries, 124,
        "total summary count across the corpus"
    );
    // Hard-coded distinct-tournament count, observed once by exhaustively
    // enumerating the corpus: every tournament here has both a summary and
    // at least one hand-history file, so this also equals both of the
    // per-id-set sizes computed independently from each subdirectory.
    assert_eq!(
        set.tournaments.len(),
        124,
        "distinct tournament count across the corpus"
    );

    for tournament in &set.tournaments {
        // Hands are sorted strictly ascending by (played_at, id) and all
        // share this tournament's id.
        for pair in tournament.hands.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            assert!(
                (a.played_at, a.id) < (b.played_at, b.id),
                "hands not strictly ascending by (played_at, id) in tournament #{}: {:?} then {:?}",
                tournament.id,
                (a.played_at, a.id),
                (b.played_at, b.id)
            );
        }

        for hand in &tournament.hands {
            assert_eq!(
                hand.tournament_id, tournament.id,
                "hand {} filed under the wrong tournament",
                hand.id
            );

            let expected_hero_cards = match hand.game {
                GameType::Holdem | GameType::AofHoldem => 2,
                GameType::Omaha => 4,
            };
            assert_eq!(
                hand.hero_cards.len(),
                expected_hero_cards,
                "wrong hero_cards length for {:?} in hand {}",
                hand.game,
                hand.id
            );

            assert!(
                matches!(hand.board.len(), 0 | 3 | 4 | 5),
                "unexpected board length {} in hand {}",
                hand.board.len(),
                hand.id
            );

            assert!(
                hand.seats.iter().any(|s| s.seat == hand.button_seat),
                "button seat {} not in seat list for hand {}",
                hand.button_seat,
                hand.id
            );

            let seated: HashSet<&str> = hand.seats.iter().map(|s| s.player.as_str()).collect();

            for action in &hand.actions {
                assert!(
                    seated.contains(action.player.as_str()),
                    "action by unseated player {:?} in hand {}",
                    action.player,
                    hand.id
                );
            }

            let result_players: HashSet<&str> =
                hand.results.iter().map(|r| r.player.as_str()).collect();
            assert_eq!(
                result_players, seated,
                "seat results don't cover exactly the seated players in hand {}",
                hand.id
            );

            let collected: u64 = hand
                .actions
                .iter()
                .filter_map(|a| match a.action {
                    Action::Collect { amount } => Some(amount),
                    _ => None,
                })
                .sum();
            let extras =
                hand.pot.rake + hand.pot.jackpot + hand.pot.bingo + hand.pot.fortune + hand.pot.tax;
            assert_eq!(
                collected + extras,
                hand.pot.total,
                "pot doesn't balance in hand {}: collected={collected} extras={extras} total={}",
                hand.id,
                hand.pot.total
            );
        }
    }
}
