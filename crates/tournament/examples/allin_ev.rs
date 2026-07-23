//! Prints the all-in EV ("luck") report for the corpus at `data/tournaments/`.
//!
//! For every hand where Hero was all-in and reached showdown, compares
//! what Hero actually collected against Hero's exact-enumeration equity
//! share of the pot at the moment of the all-in. See
//! [`tournament::analysis`] for the metric definition and the reusable
//! core logic; this binary is a thin loader + printer around it.
//!
//! Usage: `cargo run --release --example allin_ev`. The heaviest case
//! (preflop all-ins, up to `C(48,5) ≈ 1.71M` board completions per hand)
//! makes this a release-only-speed workload; tens of seconds for the full
//! corpus is expected and fine for a one-shot analysis.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cards::Card;
use tournament::GameType;
use tournament::analysis::{AllInLuck, analyze};
use tournament::load_dir;

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/tournaments")
}

fn fmt_bb(bb: f64) -> String {
    if bb >= 0.0 {
        format!("+{bb:.2} BB")
    } else {
        format!("{bb:.2} BB")
    }
}

fn fmt_cards(cards: &[Card]) -> String {
    cards
        .iter()
        .map(Card::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

fn print_subtotal(label: &str, hands: &[&AllInLuck]) {
    let n = hands.len();
    let total: f64 = hands.iter().map(|h| h.luck_bb).sum();
    println!("  {label}: n={n}  total luck = {}", fmt_bb(total));
}

fn print_hand_line(h: &AllInLuck) {
    let equity_pct = h.hero_equity * 100.0;
    println!(
        "  {} hand #{}: Hero [{}]  equity={equity_pct:.1}%  eligible={}  actual={}  luck={}",
        h.tournament_name,
        h.hand_id,
        fmt_cards(&h.hero_cards),
        h.eligible_pot,
        h.actual_collected,
        fmt_bb(h.luck_bb)
    );
}

fn main() {
    let set = load_dir(&corpus_path()).expect("the committed data/tournaments corpus should load");
    let report = analyze(&set);

    println!("=== All-in EV (luck) analysis ===");
    println!(
        "luck_chips(hand) = actual_collected_by_hero - hero_equity * hero_eligible_pot;  \
         luck_bb = luck_chips / big_blind."
    );
    println!(
        "hero_equity: P(Hero has the best hand), ties as 1/k, by EXACT enumeration of the \
         remaining board from Hero's all-in street."
    );
    println!(
        "hero_eligible_pot: sum of min(contrib_p, hero_contrib) over all players (the main \
         pot Hero contests)."
    );
    println!(
        "NOTE: this is the standard poker-tracker \"All-in Adjusted\" metric. It is exact for \
         the common heads-up all-in; for multiway all-ins with layered side pots it is the \
         usual equity x eligible-pot approximation, not a pot-by-pot decomposition."
    );
    println!();

    let n = report.hands.len();
    let total_luck_bb: f64 = report.hands.iter().map(|h| h.luck_bb).sum();
    let avg_luck_bb = if n > 0 { total_luck_bb / n as f64 } else { 0.0 };
    let total_expected_chips: f64 = report
        .hands
        .iter()
        .map(|h| h.hero_equity * h.eligible_pot as f64)
        .sum();
    let total_actual_chips: u64 = report.hands.iter().map(|h| h.actual_collected).sum();

    println!("--- Overall ---");
    println!("qualifying all-in showdown hands: {n}");
    println!("total luck: {}", fmt_bb(total_luck_bb));
    println!("average luck per all-in: {}", fmt_bb(avg_luck_bb));
    println!(
        "expected (equity x eligible) chips: {total_expected_chips:.1}   actual collected chips: {total_actual_chips}"
    );
    println!();

    let (variance, deterministic): (Vec<&AllInLuck>, Vec<&AllInLuck>) =
        report.hands.iter().partition(|h| h.cards_to_come > 0);
    println!("--- Variance-only subtotal ---");
    print_subtotal("variance (cards_to_come > 0, luck was possible)", &variance);
    print_subtotal(
        "deterministic (river all-in, cards_to_come == 0)",
        &deterministic,
    );
    println!();

    println!("--- By game type ---");
    for game in [GameType::Holdem, GameType::AofHoldem, GameType::Omaha] {
        let subset: Vec<&AllInLuck> = report.hands.iter().filter(|h| h.game == game).collect();
        print_subtotal(&format!("{game:?}"), &subset);
    }
    println!();

    println!("--- By cards to come (5=preflop, 2=flop, 1=turn, 0=river) ---");
    for cards_to_come in [5usize, 2, 1, 0] {
        let subset: Vec<&AllInLuck> = report
            .hands
            .iter()
            .filter(|h| h.cards_to_come == cards_to_come)
            .collect();
        print_subtotal(&format!("cards_to_come={cards_to_come}"), &subset);
    }
    println!();

    // Group by "series": the tournament name with any trailing " $<amount>"
    // buy-in suffix folded away, so "Daily Hyper $1" and "Daily Hyper $3"
    // aggregate into one "Daily Hyper" row. Sorted by total luck ascending
    // (most unlucky series first). Both BB and raw-chip diffs are shown,
    // because BB-normalization (dividing each hand by its big blind) and raw
    // chips can disagree in sign when big pots happen at different stack
    // depths than small ones.
    let mut by_series: BTreeMap<String, (usize, f64, f64)> = BTreeMap::new();
    for h in &report.hands {
        let series = h
            .tournament_name
            .split(" $")
            .next()
            .unwrap_or(&h.tournament_name)
            .to_string();
        let entry = by_series.entry(series).or_insert((0, 0.0, 0.0));
        entry.0 += 1;
        entry.1 += h.luck_bb;
        entry.2 += h.actual_collected as f64 - h.hero_equity * h.eligible_pot as f64;
    }
    let mut series_rows: Vec<(String, usize, f64, f64)> = by_series
        .into_iter()
        .map(|(name, (n, bb, chips))| (name, n, bb, chips))
        .collect();
    series_rows.sort_by(|a, b| a.2.partial_cmp(&b.2).expect("luck_bb is always finite"));
    println!("--- By tournament series (trailing buy-in folded) ---");
    for (name, n, bb, chips) in &series_rows {
        println!(
            "  {name:36} n={n:2}  total luck = {:>10}   (chips {:+.0})",
            fmt_bb(*bb),
            chips
        );
    }
    println!();

    let mut sorted: Vec<&AllInLuck> = report.hands.iter().collect();
    sorted.sort_by(|a, b| {
        a.luck_bb
            .partial_cmp(&b.luck_bb)
            .expect("luck_bb is always finite")
    });

    println!("--- 5 luckiest hands ---");
    for h in sorted.iter().rev().take(5) {
        print_hand_line(h);
    }
    println!();
    println!("--- 5 unluckiest hands ---");
    for h in sorted.iter().take(5) {
        print_hand_line(h);
    }
    println!();

    println!("--- Excluded ---");
    println!("total hands scanned: {}", report.stats.hands_scanned);
    println!(
        "not an all-in showdown: {}",
        report.stats.not_all_in_showdown
    );
    println!(
        "excluded (BB=0, Flipout) all-in showdowns: {}",
        report.stats.excluded_bb_zero
    );
    println!(
        "contribution-sanity mismatches (should be 0): {}",
        report.stats.contribution_mismatches
    );
}
