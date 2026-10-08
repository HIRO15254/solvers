//! Terminal evaluation kernels over the fixed seat-specific compact supports
//! (see [`crate::PostflopHands`]), shared by every fold and showdown terminal.
//!
//! Invariant relied on by both kernels: by the time a reach vector reaches a
//! terminal, the chance masks have already zeroed the opponent's reach on
//! every hand blocked by a card dealt on the path. A kernel may therefore be
//! handed a hand list that is a superset of the hands live at this terminal
//! (`fold_kernel` reuses one list per seat for every street and runout), and
//! the inclusion-exclusion sums stay exact because dead hands contribute zero.
//! The f64 sums add the nonzero terms in the same order as the former dense
//! kernels (rank order, then global combo order), so results are bit-identical.
//! The f32 showdown sweep maintains one utility-weighted total/card array:
//! all hands start at lose, tied groups move to tie, then to win. Own hands
//! need two card loads plus the identical-combo tie correction. Opponent
//! additions are unconditional and follow fixed rank/hand order, independent
//! of thread count. Only live ranked own entries are written; the caller
//! zeros the remaining entries. The fused f32 showdown/fold variant shares
//! opponent sums and the own-hand loop for terminal siblings with the same
//! board and opponent reach. It clears the fold output first and writes only
//! live ranked own hands; the rank table must cover every board-live support
//! entry. Opponent-node add variants leave dead own entries untouched. For
//! different fold/call reaches on one board, fold compat sums use the
//! showdown opponent list: it covers every board-live support entry, and
//! omitted dead entries have zero reach. The sweep starts at
//! u_lose * call_total/card + u_fold * fold_total/card and restores both
//! identical-combo corrections, adding both outcomes in one own-hand loop.
//! Lone terminals also add directly, without a temporary or an add pass.
//! Evaluation/EV/BR and all f64 CFR calls retain the original kernels.

use crate::hands::ABSENT;
use nlh::HandRank;
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hand {
    pub local: u16,
    pub cards: [u8; 2],
}
#[derive(Default)]
pub(crate) struct RankedHands {
    pub hands: Vec<Hand>,
    // Rank and exclusive end offset; within a group, global combo order.
    pub groups: Vec<(HandRank, usize)>,
}
fn add(hands: &[Hand], reach: &[f32], total: &mut f64, card: &mut [f64; 52]) {
    for h in hands {
        let r = reach[h.local as usize] as f64;
        if r != 0.0 {
            *total += r;
            card[h.cards[0] as usize] += r;
            card[h.cards[1] as usize] += r;
        }
    }
}
pub(crate) fn compat_sums(hands: &[Hand], reach: &[f32]) -> (f64, [f64; 52]) {
    let mut total = 0.0;
    let mut card = [0.0; 52];
    add(hands, reach, &mut total, &mut card);
    (total, card)
}
fn same_reach(same: &[u16], h: Hand, reach: &[f32]) -> f64 {
    let other = same[h.local as usize];
    if other == ABSENT {
        0.0
    } else {
        reach[other as usize] as f64
    }
}
pub(crate) fn showdown_kernel(
    own: &RankedHands,
    opp: &RankedHands,
    same: &[u16],
    utilities: [f64; 3],
    reach: &[f32],
    out: &mut [f32],
) {
    let [u_win, u_tie, u_lose] = utilities;
    let (all_total, all_card) = compat_sums(&opp.hands, reach);
    let mut below_total = 0.0;
    let mut below_card = [0.0; 52];
    let mut oi = 0;
    let mut os = 0;
    let mut start = 0;
    for &(rank, end) in &own.groups {
        while oi < opp.groups.len() && opp.groups[oi].0 < rank {
            let oe = opp.groups[oi].1;
            let mut group_total = 0.0;
            // Legacy total adds groups; card sums add individual hands.
            add(&opp.hands[os..oe], reach, &mut group_total, &mut below_card);
            below_total += group_total;
            os = oe;
            oi += 1;
        }
        let mut group_total = 0.0;
        let mut group_card = [0.0; 52];
        let tied = oi < opp.groups.len() && opp.groups[oi].0 == rank;
        let oe = if tied { opp.groups[oi].1 } else { os };
        add(&opp.hands[os..oe], reach, &mut group_total, &mut group_card);
        for &h in &own.hands[start..end] {
            let [a, b] = h.cards.map(usize::from);
            let same = same_reach(same, h, reach);
            let win = below_total - below_card[a] - below_card[b];
            let tie = group_total - group_card[a] - group_card[b] + same;
            let compat = all_total - all_card[a] - all_card[b] + same;
            let lose = compat - win - tie;
            out[h.local as usize] = (u_win * win + u_tie * tie + u_lose * lose) as f32;
        }
        if tied {
            below_total += group_total;
            let mut unused = 0.0;
            add(&opp.hands[os..oe], reach, &mut unused, &mut below_card);
            os = oe;
            oi += 1;
        }
        start = end;
    }
}
pub(crate) fn fold_kernel(
    own: &[Hand],
    opp: &[Hand],
    same: &[u16],
    u: f64,
    board: u64,
    reach: &[f32],
    out: &mut [f32],
) {
    let (total, card) = compat_sums(opp, reach);
    for &h in own {
        let [a, b] = h.cards.map(usize::from);
        if board & ((1u64 << a) | (1u64 << b)) != 0 {
            out[h.local as usize] = 0.0;
            continue;
        }
        out[h.local as usize] =
            (u * (total - card[a] - card[b] + same_reach(same, h, reach))) as f32;
    }
}

fn add_relaxed_f32(hands: &[Hand], reach: &[f32], total: &mut f32, card: &mut [f32; 52]) {
    for h in hands {
        let r = reach[h.local as usize];
        *total += r;
        card[h.cards[0] as usize] += r;
        card[h.cards[1] as usize] += r;
    }
}
pub(crate) fn compat_sums_relaxed_f32(hands: &[Hand], reach: &[f32]) -> (f32, [f32; 52]) {
    let mut total = 0.0;
    let mut card = [0.0; 52];
    add_relaxed_f32(hands, reach, &mut total, &mut card);
    (total, card)
}
fn same_reach_relaxed_f32(same: &[u16], h: Hand, reach: &[f32]) -> f32 {
    let other = same[h.local as usize];
    if other == ABSENT {
        0.0
    } else {
        reach[other as usize]
    }
}
fn add_weighted_f32(
    hands: &[Hand],
    reach: &[f32],
    weight: f32,
    total: &mut f32,
    card: &mut [f32; 52],
) {
    for h in hands {
        let r = weight * reach[h.local as usize];
        *total += r;
        card[h.cards[0] as usize] += r;
        card[h.cards[1] as usize] += r;
    }
}
pub(crate) fn showdown_kernel_relaxed_f32(
    own: &RankedHands,
    opp: &RankedHands,
    same: &[u16],
    utilities: [f64; 3],
    reach: &[f32],
    out: &mut [f32],
) {
    let [win, tie, lose] = utilities;
    showdown_relaxed_f32::<false, false, false>(
        own,
        opp,
        same,
        [win, tie, lose, 0.0],
        reach,
        out,
        &mut [],
        &[],
    );
}

/// Utilities are [showdown win, tie, lose, fold]. The fold output is cleared
/// here, then written only for live ranked own hands. Both outcomes share all
/// opponent passes and the identical-combo lookup in one own-hand loop.
pub(crate) fn showdown_fold_kernel_relaxed_f32(
    own: &RankedHands,
    opp: &RankedHands,
    same: &[u16],
    utilities: [f64; 4],
    reach: &[f32],
    out: &mut [f32],
    fold_out: &mut [f32],
) {
    showdown_relaxed_f32::<true, false, false>(
        own,
        opp,
        same,
        utilities,
        reach,
        out,
        fold_out,
        &[],
    );
}

/// Add a lone showdown directly; only live ranked own hands are visited.
pub(crate) fn add_showdown_kernel_relaxed_f32(
    own: &RankedHands,
    opp: &RankedHands,
    same: &[u16],
    utilities: [f64; 3],
    reach: &[f32],
    out: &mut [f32],
) {
    let [win, tie, lose] = utilities;
    showdown_relaxed_f32::<false, true, false>(
        own,
        opp,
        same,
        [win, tie, lose, 0.0],
        reach,
        out,
        &mut [],
        &[],
    );
}

/// Different action-scaled reaches, same board. Fold compatible mass is
/// linear in total/card sums: seed the showdown sweep with both weighted
/// masses and restore the identical-combo fold correction in the own loop.
pub(crate) fn add_showdown_fold_kernel_relaxed_f32(
    own: &RankedHands,
    opp: &RankedHands,
    same: &[u16],
    utilities: [f64; 4],
    call_reach: &[f32],
    fold_reach: &[f32],
    out: &mut [f32],
) {
    showdown_relaxed_f32::<false, true, true>(
        own,
        opp,
        same,
        utilities,
        call_reach,
        out,
        &mut [],
        fold_reach,
    );
}

#[allow(clippy::too_many_arguments)]
fn showdown_relaxed_f32<const FOLD: bool, const ADD: bool, const OPP_FOLD: bool>(
    own: &RankedHands,
    opp: &RankedHands,
    same_indices: &[u16],
    utilities: [f64; 4],
    reach: &[f32],
    out: &mut [f32],
    fold_out: &mut [f32],
    fold_reach: &[f32],
) {
    let [u_win, u_tie, u_lose, u_fold] = utilities.map(|u| u as f32);
    if FOLD {
        fold_out.fill(0.0);
    }
    let (all_total, all_card) = compat_sums_relaxed_f32(&opp.hands, reach);
    let mut total = u_lose * all_total;
    let mut card = all_card.map(|v| u_lose * v);
    if OPP_FOLD {
        // The rank table covers all board-live support hands; omitted dead
        // hands have zero reach, so it supplies the fold compat sums too.
        let (fold_total, fold_card) = compat_sums_relaxed_f32(&opp.hands, fold_reach);
        total += u_fold * fold_total;
        for (c, f) in card.iter_mut().zip(fold_card) {
            *c += u_fold * f;
        }
    }
    let mut oi = 0;
    let mut os = 0;
    let mut start = 0;
    for &(rank, end) in &own.groups {
        while oi < opp.groups.len() && opp.groups[oi].0 < rank {
            let oe = opp.groups[oi].1;
            add_weighted_f32(
                &opp.hands[os..oe],
                reach,
                u_win - u_lose,
                &mut total,
                &mut card,
            );
            os = oe;
            oi += 1;
        }
        let tied = oi < opp.groups.len() && opp.groups[oi].0 == rank;
        let oe = if tied { opp.groups[oi].1 } else { os };
        let mut group_total = 0.0;
        let mut group_card = [0.0; 52];
        for h in &opp.hands[os..oe] {
            let [a, b] = h.cards.map(usize::from);
            let r = reach[h.local as usize];
            let weighted = (u_tie - u_lose) * r;
            group_total += r;
            group_card[a] += r;
            group_card[b] += r;
            card[a] += weighted;
            card[b] += weighted;
        }
        total += (u_tie - u_lose) * group_total;
        // K - C[a] - C[b] + u_tie * same: two card loads per own hand.
        for &h in &own.hands[start..end] {
            let [a, b] = h.cards.map(usize::from);
            // Reuse one identical-combo lookup and its existing ABSENT
            // check for both reaches; no second data-dependent branch.
            let (same, fold_same) = if OPP_FOLD {
                let other = same_indices[h.local as usize];
                if other == ABSENT {
                    (0.0, 0.0)
                } else {
                    (reach[other as usize], fold_reach[other as usize])
                }
            } else {
                (same_reach_relaxed_f32(same_indices, h, reach), 0.0)
            };
            let mut value = total - card[a] - card[b] + u_tie * same;
            if OPP_FOLD {
                value += u_fold * fold_same;
            }
            if ADD {
                out[h.local as usize] += value;
            } else {
                out[h.local as usize] = value;
            }
            if FOLD {
                fold_out[h.local as usize] =
                    u_fold * (all_total - all_card[a] - all_card[b] + same);
            }
        }
        if tied {
            // Reuse the existing 52-card merge to convert tie to win.
            total += (u_win - u_tie) * group_total;
            for (c, g) in card.iter_mut().zip(group_card) {
                *c += (u_win - u_tie) * g;
            }
            os = oe;
            oi += 1;
        }
        start = end;
    }
}

// Frozen fdd3618 f32 implementation for accuracy regression tests.
#[cfg(test)]
pub(crate) fn head_showdown_kernel_relaxed_f32(
    own: &RankedHands,
    opp: &RankedHands,
    same: &[u16],
    utilities: [f64; 3],
    reach: &[f32],
    out: &mut [f32],
) {
    let [u_win, u_tie, u_lose] = utilities.map(|u| u as f32);
    let (all_total, all_card) = compat_sums_relaxed_f32(&opp.hands, reach);
    let mut below_total = 0.0;
    let mut below_card = [0.0; 52];
    let mut oi = 0;
    let mut os = 0;
    let mut start = 0;
    for &(rank, end) in &own.groups {
        while oi < opp.groups.len() && opp.groups[oi].0 < rank {
            let oe = opp.groups[oi].1;
            let mut group_total = 0.0;
            // Total adds groups; strictly-below card sums add individual hands.
            add_relaxed_f32(&opp.hands[os..oe], reach, &mut group_total, &mut below_card);
            below_total += group_total;
            os = oe;
            oi += 1;
        }
        let mut group_total = 0.0;
        let mut group_card = [0.0; 52];
        let tied = oi < opp.groups.len() && opp.groups[oi].0 == rank;
        let oe = if tied { opp.groups[oi].1 } else { os };
        add_relaxed_f32(&opp.hands[os..oe], reach, &mut group_total, &mut group_card);
        for &h in &own.hands[start..end] {
            let [a, b] = h.cards.map(usize::from);
            let same = same_reach_relaxed_f32(same, h, reach);
            let win = below_total - below_card[a] - below_card[b];
            let tie = group_total - group_card[a] - group_card[b] + same;
            let compat = all_total - all_card[a] - all_card[b] + same;
            let lose = compat - win - tie;
            out[h.local as usize] = u_win * win + u_tie * tie + u_lose * lose;
        }
        if tied {
            below_total += group_total;
            for (below, group) in below_card.iter_mut().zip(group_card) {
                *below += group;
            }
            os = oe;
            oi += 1;
        }
        start = end;
    }
}
pub(crate) fn fold_kernel_relaxed_f32(
    own: &[Hand],
    opp: &[Hand],
    same: &[u16],
    u: f64,
    board: u64,
    reach: &[f32],
    out: &mut [f32],
) {
    fold_relaxed_f32::<false>(own, opp, same, u, board, reach, out);
}
pub(crate) fn add_fold_kernel_relaxed_f32(
    own: &[Hand],
    opp: &[Hand],
    same: &[u16],
    u: f64,
    board: u64,
    reach: &[f32],
    out: &mut [f32],
) {
    fold_relaxed_f32::<true>(own, opp, same, u, board, reach, out);
}
fn fold_relaxed_f32<const ADD: bool>(
    own: &[Hand],
    opp: &[Hand],
    same: &[u16],
    u: f64,
    board: u64,
    reach: &[f32],
    out: &mut [f32],
) {
    let u = u as f32;
    let (total, card) = compat_sums_relaxed_f32(opp, reach);
    for &h in own {
        let [a, b] = h.cards.map(usize::from);
        if board & ((1u64 << a) | (1u64 << b)) != 0 {
            if !ADD {
                out[h.local as usize] = 0.0;
            }
            continue;
        }
        let value = u * (total - card[a] - card[b] + same_reach_relaxed_f32(same, h, reach));
        if ADD {
            out[h.local as usize] += value;
        } else {
            out[h.local as usize] = value;
        }
    }
}

#[cfg(test)]
mod legacy {
    use nlh::{HandRank, combo_cards};

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
    pub(crate) fn fold_kernel(
        sorted: &[(HandRank, u32)],
        u: f64,
        opp_reach: &[f32],
        out: &mut [f32],
    ) {
        let (all_total, all_card) = compat_sums(sorted, opp_reach);
        for &(_, combo) in sorted {
            let (c1, c2) = combo_cards(combo as usize);
            let compat = all_total - all_card[c1.index()] - all_card[c2.index()]
                + opp_reach[combo as usize] as f64;
            out[combo as usize] = (u * compat) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{ChipEv, NoRake, PayoffPipeline};
    use crate::{PostflopConfig, PostflopHands, build_postflop_game};
    use hu_engine::TerminalEvaluator;
    use nlh::{Card, CardSet, Chips, NUM_COMBOS, PerPlayer, Player, Range, combo_cards, rank_of};

    #[test]
    fn f32_synthetic_kernels_close() {
        let list = |stride: usize| {
            let mut hands: Vec<Hand> = (0..NUM_COMBOS)
                .filter(|h| h % stride != 0)
                .enumerate()
                .map(|(local, h)| {
                    let (a, b) = combo_cards(h);
                    Hand {
                        local: local as u16,
                        cards: [a.index() as u8, b.index() as u8],
                    }
                })
                .collect();
            // Identical combos must have the same synthetic rank in both seats.
            let rank =
                |h: &Hand| HandRank((u16::from(h.cards[0]) * 53 + u16::from(h.cards[1])) % 19);
            hands.sort_by_key(|h| (rank(h), h.local));
            let mut groups = Vec::new();
            for (i, h) in hands.iter().enumerate() {
                let rank = rank(h);
                if groups.last().is_none_or(|&(r, _)| r != rank) {
                    groups.push((rank, i + 1));
                } else {
                    groups.last_mut().unwrap().1 = i + 1;
                }
            }
            RankedHands { hands, groups }
        };
        let own = list(3);
        let opp = list(5);
        let mut same = vec![ABSENT; own.hands.len()];
        for h in &own.hands {
            if let Some(o) = opp.hands.iter().find(|o| o.cards == h.cards) {
                same[h.local as usize] = o.local;
            }
        }
        let mut maximum = 0.0f64;
        for scale in [0.0, 0.001, 1.0, 100.0] {
            let reach: Vec<f32> = (0..opp.hands.len())
                .map(|i| {
                    if i % 7 == 0 {
                        0.0
                    } else {
                        (i * 17 % 101 + 1) as f32 / 103.0 * scale
                    }
                })
                .collect();
            for kind in 0..2 {
                let mut exact = vec![0.0; own.hands.len()];
                let mut out = exact.clone();
                if kind == 0 {
                    showdown_kernel(&own, &opp, &same, [1.25, -0.125, -1.75], &reach, &mut exact);
                } else {
                    fold_kernel(&own.hands, &opp.hands, &same, -1.25, 1, &reach, &mut exact);
                }
                {
                    if kind == 0 {
                        showdown_kernel_relaxed_f32(
                            &own,
                            &opp,
                            &same,
                            [1.25, -0.125, -1.75],
                            &reach,
                            &mut out,
                        );
                    } else {
                        fold_kernel_relaxed_f32(
                            &own.hands, &opp.hands, &same, -1.25, 1, &reach, &mut out,
                        );
                    }
                    let denom = exact.iter().map(|v| f64::from(v.abs())).fold(0.0, f64::max);
                    for (&a, &b) in exact.iter().zip(&out) {
                        assert!(b.is_finite());
                        if denom == 0.0 {
                            assert_eq!(a, b);
                        } else {
                            maximum = maximum.max((f64::from(a) - f64::from(b)).abs() / denom);
                        }
                    }
                }
            }
        }
        assert!(maximum < 1e-5);
        println!("synthetic f32 relative infinity error: {maximum:e}");
    }

    #[test]
    fn asymmetric_fractional_support_matches_dense_kernels_bitwise() {
        for board in ["2c 7d 9h Js Qs", "Ah Kh Qd Jc Ts", "2s 3s 4s 5s 6s"] {
            let board: Vec<Card> = board
                .split_whitespace()
                .map(|c| c.parse().unwrap())
                .collect();
            let board_set: CardSet = board.iter().copied().collect();
            let range = |p: Player| {
                let mut r = Range::default();
                for h in 0..NUM_COMBOS {
                    if !(h + p.index() * 3).is_multiple_of(p.index() + 3) {
                        r.set_weight(h, ((h % 17 + 1) as f32) / 17.0);
                    }
                }
                r
            };
            let config = PostflopConfig {
                board: board.clone(),
                ranges: PerPlayer::new(range(Player::P0), range(Player::P1)),
                pot: Chips(2),
                effective_stack: Chips(5),
                ..Default::default()
            };
            let game = build_postflop_game(
                &config,
                PayoffPipeline {
                    rake: &NoRake,
                    utility: &ChipEv,
                },
            );
            let hands = &game.game.evaluator.hands;
            let mut sorted = Vec::new();
            let mut fold = Vec::new();
            for h in 0..NUM_COMBOS {
                let (a, b) = combo_cards(h);
                if board_set.contains(a) || board_set.contains(b) {
                    continue;
                }
                sorted.push((rank_of(board.iter().copied().chain([a, b])), h as u32));
                fold.push((nlh::HandRank(0), h as u32));
            }
            sorted.sort_unstable();
            for p in Player::BOTH {
                let opp = p.opponent();
                let reach: Vec<f32> = game.game.root_ranges[opp]
                    .iter()
                    .enumerate()
                    .map(|(i, &w)| {
                        if i % 7 == 0 {
                            0.0
                        } else {
                            w * ((i % 11 + 1) as f32 / 11.0)
                        }
                    })
                    .collect();
                let global_reach = hands.expand(opp, &reach);
                let mut actual = vec![f32::NAN; hands.len(p)];
                game.game.evaluator.eval(0, p, &reach, &mut actual);
                let mut expected = vec![0.0; NUM_COMBOS];
                legacy::showdown_kernel(&sorted, 1.0, 0.0, -1.0, &global_reach, &mut expected);
                for (&h, &v) in hands.combos(p).iter().zip(&actual) {
                    assert_eq!(
                        v.to_bits(),
                        expected[h as usize].to_bits(),
                        "showdown {board:?} {p:?} {h}"
                    );
                }
                let list = |q| {
                    hands
                        .combos(q)
                        .iter()
                        .enumerate()
                        .map(|(i, &h)| {
                            let (a, b) = combo_cards(h as usize);
                            Hand {
                                local: i as u16,
                                cards: [a.index() as u8, b.index() as u8],
                            }
                        })
                        .collect::<Vec<_>>()
                };
                fold_kernel(
                    &list(p),
                    &list(opp),
                    &hands.same[p],
                    -1.25,
                    0,
                    &reach,
                    &mut actual,
                );
                legacy::fold_kernel(&fold, -1.25, &global_reach, &mut expected);
                for (&h, &v) in hands.combos(p).iter().zip(&actual) {
                    assert_eq!(
                        v.to_bits(),
                        expected[h as usize].to_bits(),
                        "fold {board:?} {p:?} {h}"
                    );
                }
            }
        }
    }

    #[test]
    fn support_keeps_every_positive_weight_and_has_global_round_trip() {
        let board: Vec<Card> = "2c 7d 9h"
            .split_whitespace()
            .map(|c| c.parse().unwrap())
            .collect();
        let mut p0 = Range::default();
        let a = nlh::combo_index("As".parse().unwrap(), "Ah".parse().unwrap());
        let b = nlh::combo_index("Ks".parse().unwrap(), "Kh".parse().unwrap());
        let blocked = nlh::combo_index(board[0], "3c".parse().unwrap());
        p0.set_weight(a, f32::from_bits(1));
        p0.set_weight(blocked, 1.0);
        let mut p1 = Range::default();
        p1.set_weight(b, 0.25);
        let hands = PostflopHands::new(&board, &PerPlayer::new(p0, p1));
        assert_eq!(hands.combos(Player::P0), &[a as u16]);
        assert_eq!(hands.combos(Player::P1), &[b as u16]);
        assert_eq!(hands.local(Player::P0, blocked), None);
        assert_eq!(hands.local(Player::P0, b), None);
        assert_eq!(hands.same[Player::P0], [crate::hands::ABSENT]);
        assert_eq!(
            hands.compact(Player::P1, &hands.expand(Player::P1, &[0.625])),
            [0.625]
        );
    }
}
