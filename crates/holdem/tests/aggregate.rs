//! Range aggregation and range-vs-range equity correctness: combo-to-class
//! mapping goldens, class-level weight/average summaries, and showdown
//! equity goldens at the river, turn, and flop.

use cards::{Card, NUM_COMBOS, PerPlayer, Player, Range, combo_index};
use holdem::{class_average, class_of_combo, class_weights, range_equity};

fn parse_board(s: &str) -> Vec<Card> {
    s.split_whitespace().map(|c| c.parse().unwrap()).collect()
}

fn combo(a: &str, b: &str) -> usize {
    combo_index(a.parse().unwrap(), b.parse().unwrap())
}

#[test]
fn class_mapping_goldens() {
    // Cell indices anchored by `cards::range`'s own `class_index_layout`
    // golden test: AA is the top-left diagonal cell (0), AKs the cell just
    // right of it (1), AKo its mirror below the diagonal (13).
    assert_eq!(
        class_of_combo(combo("Ah", "Ad")),
        0,
        "AhAd should land in the AA cell"
    );
    assert_eq!(
        class_of_combo(combo("Ah", "Kh")),
        1,
        "AhKh should land in the AKs cell"
    );
    assert_eq!(
        class_of_combo(combo("Ah", "Kd")),
        13,
        "AhKd should land in the AKo cell"
    );
}

#[test]
fn class_weights_matches_range_total() {
    let range: Range = "AA".parse().unwrap();
    let weights = class_weights(range.weights());
    for (class, &w) in weights.iter().enumerate() {
        if class == 0 {
            assert!(
                (w - 6.0).abs() < 1e-9,
                "AA class weight should be 6.0, got {w}"
            );
        } else {
            assert_eq!(w, 0.0, "class {class} should have zero weight, got {w}");
        }
    }
}

#[test]
fn class_average_weighted_by_range() {
    let combined: Range = "AA,KK".parse().unwrap();
    let weights = combined.weights();

    let aa_range: Range = "AA".parse().unwrap();
    let per_combo: Vec<f32> = (0..NUM_COMBOS)
        .map(|c| if aa_range.weight(c) > 0.0 { 1.0 } else { 0.0 })
        .collect();

    let avg = class_average(weights, &per_combo);

    let aa_class = 0; // see class_mapping_goldens
    let kk_class = class_of_combo(combo("Kh", "Kd"));

    assert!(
        (avg[aa_class] - 1.0).abs() < 1e-9,
        "AA class average should be 1.0, got {}",
        avg[aa_class]
    );
    assert!(
        avg[kk_class].abs() < 1e-9,
        "KK class average should be 0.0, got {}",
        avg[kk_class]
    );
}

/// Board "Ks Kh Kd 2c 2d" is itself a full house (Kings full of deuces), so
/// every hero hand's true 5-card best hand is "Kings full of <hero's own
/// pair, if higher than deuces>" — Aces beat Queens beat Threes beat the
/// board's bare deuces. AA (Aces full) always beats QQ (Queens full); 33
/// (Threes full) always loses to it. No hero combo shares a rank with the
/// board, so there's no card-removal asymmetry across combos within a
/// class: every AA combo, every 33 combo, and every QQ combo sees the exact
/// same opposing compat structure.
#[test]
fn river_equity_golden_full_house_kicker() {
    let board = parse_board("Ks Kh Kd 2c 2d");
    let p0: Range = "AA,33".parse().unwrap();
    let p1: Range = "QQ".parse().unwrap();
    let ranges = PerPlayer::new(p0.weights().to_vec(), p1.weights().to_vec());

    let equity = range_equity(&board, &ranges);

    let aa: Range = "AA".parse().unwrap();
    let threes: Range = "33".parse().unwrap();
    let qq: Range = "QQ".parse().unwrap();

    for c in 0..NUM_COMBOS {
        if aa.weight(c) > 0.0 {
            assert!(
                (equity[Player::P0][c] - 1.0).abs() < 1e-6,
                "AA combo {c} equity should be 1.0, got {}",
                equity[Player::P0][c]
            );
        }
        if threes.weight(c) > 0.0 {
            assert!(
                equity[Player::P0][c].abs() < 1e-6,
                "33 combo {c} equity should be 0.0, got {}",
                equity[Player::P0][c]
            );
        }
        if qq.weight(c) > 0.0 {
            assert!(
                (equity[Player::P1][c] - 0.5).abs() < 1e-6,
                "QQ combo {c} equity vs AA,33 should be 0.5, got {}",
                equity[Player::P1][c]
            );
        }
    }
}

/// Turn smoke test: KK is a big favorite over both AA and QQ preflop, but
/// postflop on this dry board KK is drawing thin against AA (needs to spike
/// a King) and is a solid favorite over QQ (needs a Queen to catch up).
/// `range_equity` should reflect that qualitatively even with only one card
/// left to come: AA stays a big favorite, QQ stays a big underdog.
#[test]
fn turn_equity_smoke_aa_qq_vs_kk() {
    let board = parse_board("2c 7d 9h Js");
    let p0: Range = "AA,QQ".parse().unwrap();
    let p1: Range = "KK".parse().unwrap();
    let ranges = PerPlayer::new(p0.weights().to_vec(), p1.weights().to_vec());

    let equity = range_equity(&board, &ranges);

    let aa: Range = "AA".parse().unwrap();
    let qq: Range = "QQ".parse().unwrap();
    for c in 0..NUM_COMBOS {
        if aa.weight(c) > 0.0 {
            assert!(
                equity[Player::P0][c] > 0.8,
                "AA combo {c} equity too low vs KK: {}",
                equity[Player::P0][c]
            );
        }
        if qq.weight(c) > 0.0 {
            assert!(
                equity[Player::P0][c] < 0.4,
                "QQ combo {c} equity too high vs KK: {}",
                equity[Player::P0][c]
            );
        }
    }
}

/// Flop sanity + consistency check. AA vs KK on a dry, disconnected
/// rainbow flop is a big (but not overwhelming) favorite: KK's only outs are
/// running a set or quads.
///
/// The consistency check spot-verifies the two directions of `range_equity`
/// agree, via the simpler variant the spec allows: isolate one AA combo and
/// one disjoint KK combo into singleton ranges (weight 1 at that combo, 0
/// elsewhere) and recompute. With only one combo on each side, every
/// mutually-compatible runout contributes exactly 1.0 split between the two
/// hero-side equities (win+lose = 1, tie 0.5+0.5 = 1), so the two isolated
/// equities must sum to 1.0 exactly, up to f32 accumulation noise.
#[test]
#[ignore = "~1,176 flop runouts; CI runs it in release"]
fn flop_equity_sanity_aa_vs_kk() {
    let board = parse_board("2c 7d 9h");
    let aa: Range = "AA".parse().unwrap();
    let kk: Range = "KK".parse().unwrap();
    let ranges = PerPlayer::new(aa.weights().to_vec(), kk.weights().to_vec());

    let equity = range_equity(&board, &ranges);
    for c in 0..NUM_COMBOS {
        if aa.weight(c) > 0.0 {
            let e = equity[Player::P0][c];
            assert!(
                (0.85..0.95).contains(&e),
                "AA combo {c} equity vs KK out of range: {e}"
            );
        }
    }

    let spot_checks = [("Ah", "Ac", "Kd", "Kh"), ("As", "Ad", "Ks", "Kc")];
    for (a1, a2, k1, k2) in spot_checks {
        let mut p0 = vec![0.0f32; NUM_COMBOS];
        let mut p1 = vec![0.0f32; NUM_COMBOS];
        let a_combo = combo(a1, a2);
        let k_combo = combo(k1, k2);
        p0[a_combo] = 1.0;
        p1[k_combo] = 1.0;

        let iso = range_equity(&board, &PerPlayer::new(p0, p1));
        let sum = iso[Player::P0][a_combo] as f64 + iso[Player::P1][k_combo] as f64;
        assert!(
            (sum - 1.0).abs() < 1e-3,
            "isolated both-ways equity should sum to 1.0, got {sum} for {a1}{a2} vs {k1}{k2}"
        );
    }
}
