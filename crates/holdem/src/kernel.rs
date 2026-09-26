//! Terminal evaluation kernels shared by every postflop terminal (fold and
//! showdown, at any street). Extracted verbatim from the original
//! river-only evaluator so the multi-street builder and the river shim run
//! the exact same math.
//!
//! Invariant relied on by both kernels: by the time a reach vector reaches
//! a terminal, the engine's chance-node masks have already zeroed the
//! opponent's reach on every combo blocked by a board card dealt on the
//! path to that terminal. A kernel may therefore be handed a `sorted` combo
//! list that is a *superset* of the combos actually live at this specific
//! terminal — `fold_kernel` in particular is handed one list per game
//! (disjoint only from the game's starting board) and reused for fold
//! terminals at every street and every runout, rather than a separate exact
//! list per board — and the inclusion-exclusion sums below stay exactly
//! correct, because every "dead" combo the superset drags in contributes
//! zero.

use cards::{HandRank, NUM_COMBOS, Player, combo_cards};

use crate::hands::PostflopHands;

/// Bridge seat-local vectors to the historical global-combo kernels. Rank
/// tables retain global combo ids and their ordering, so the floating-point
/// sums below are unchanged. The two full-width buffers live only on the
/// stack for this call; tree storage and evaluator state remain compact.
fn with_global_combos(
    hands: &PostflopHands,
    player: Player,
    opp_reach: &[f32],
    out: &mut [f32],
    evaluate: impl FnOnce(&[f32], &mut [f32]),
) {
    assert_eq!(opp_reach.len(), hands.len(player.opponent()));
    assert_eq!(out.len(), hands.len(player));
    let mut global_reach = [0.0; NUM_COMBOS];
    for (&combo, &reach) in hands.combos(player.opponent()).iter().zip(opp_reach) {
        global_reach[combo as usize] = reach;
    }
    let mut global_out = [0.0; NUM_COMBOS];
    evaluate(&global_reach, &mut global_out);
    for (&combo, value) in hands.combos(player).iter().zip(out) {
        *value = global_out[combo as usize];
    }
}

/// Showdown evaluation with seat-local reach and CFV indices.
pub(crate) fn showdown_kernel_compact(
    sorted: &[(HandRank, u32)],
    utilities: [f64; 3],
    hands: &PostflopHands,
    player: Player,
    opp_reach: &[f32],
    out: &mut [f32],
) {
    with_global_combos(hands, player, opp_reach, out, |reach, values| {
        showdown_kernel(
            sorted,
            utilities[0],
            utilities[1],
            utilities[2],
            reach,
            values,
        );
    });
}

/// Fold evaluation with seat-local reach and CFV indices.
pub(crate) fn fold_kernel_compact(
    sorted: &[(HandRank, u32)],
    utility: f64,
    hands: &PostflopHands,
    player: Player,
    opp_reach: &[f32],
    out: &mut [f32],
) {
    with_global_combos(hands, player, opp_reach, out, |reach, values| {
        fold_kernel(sorted, utility, reach, values);
    });
}

/// Sum of `opp_reach` over combos disjoint from hand `h`, via
/// inclusion-exclusion on h's two cards.
pub(crate) fn compat_sums(sorted: &[(HandRank, u32)], opp_reach: &[f32]) -> (f64, [f64; 52]) {
    let mut total = 0.0f64;
    let mut per_card = [0.0f64; 52];
    for &(_, combo) in sorted {
        let r = opp_reach[combo as usize] as f64;
        if r != 0.0 {
            let (c1, c2) = combo_cards(combo as usize);
            total += r;
            per_card[c1.index()] += r;
            per_card[c2.index()] += r;
        }
    }
    (total, per_card)
}

/// Exact showdown evaluation of every live hand against the full opponent
/// reach vector: an O(n + m) sweep over ascending equal-rank groups, keeping
/// strictly-below prefix sums; ties are handled with the group's own sums.
///
/// `out` is not zeroed here — the caller fills it once per `eval` call (see
/// `postflop::PostflopEvaluator::eval`) and both kernels only ever write the
/// entries named in `sorted`.
pub(crate) fn showdown_kernel(
    sorted: &[(HandRank, u32)],
    u_win: f64,
    u_tie: f64,
    u_lose: f64,
    opp_reach: &[f32],
    out: &mut [f32],
) {
    let (all_total, all_card) = compat_sums(sorted, opp_reach);

    let mut below_total = 0.0f64;
    let mut below_card = [0.0f64; 52];
    let mut group_start = 0;
    while group_start < sorted.len() {
        let rank = sorted[group_start].0;
        let mut group_end = group_start;
        while group_end < sorted.len() && sorted[group_end].0 == rank {
            group_end += 1;
        }
        let group = &sorted[group_start..group_end];

        let mut group_total = 0.0f64;
        let mut group_card = [0.0f64; 52];
        for &(_, combo) in group {
            let r = opp_reach[combo as usize] as f64;
            if r != 0.0 {
                let (c1, c2) = combo_cards(combo as usize);
                group_total += r;
                group_card[c1.index()] += r;
                group_card[c2.index()] += r;
            }
        }

        for &(_, combo) in group {
            let idx = combo as usize;
            let (c1, c2) = combo_cards(idx);
            let (i1, i2) = (c1.index(), c2.index());
            // Inclusion-exclusion: the o == h combo is subtracted
            // twice by the per-card sums, so groups containing h
            // (tie, all) add its reach back once.
            let win = below_total - below_card[i1] - below_card[i2];
            let tie = group_total - group_card[i1] - group_card[i2] + opp_reach[idx] as f64;
            let compat = all_total - all_card[i1] - all_card[i2] + opp_reach[idx] as f64;
            let lose = compat - win - tie;
            out[idx] = (u_win * win + u_tie * tie + u_lose * lose) as f32;
        }

        below_total += group_total;
        for &(_, combo) in group {
            let r = opp_reach[combo as usize] as f64;
            if r != 0.0 {
                let (c1, c2) = combo_cards(combo as usize);
                below_card[c1.index()] += r;
                below_card[c2.index()] += r;
            }
        }
        group_start = group_end;
    }
}

/// O(n) inclusion-exclusion fold kernel: every live hand in `sorted` wins
/// the same outcome `u`, scaled by the opponent reach compatible with it.
///
/// `sorted`'s combo list need not be filtered down to this terminal's exact
/// board — see the module-level invariant above.
pub(crate) fn fold_kernel(sorted: &[(HandRank, u32)], u: f64, opp_reach: &[f32], out: &mut [f32]) {
    let (all_total, all_card) = compat_sums(sorted, opp_reach);
    for &(_, combo) in sorted {
        let (c1, c2) = combo_cards(combo as usize);
        let compat = all_total - all_card[c1.index()] - all_card[c2.index()]
            + opp_reach[combo as usize] as f64;
        out[combo as usize] = (u * compat) as f32;
    }
}
