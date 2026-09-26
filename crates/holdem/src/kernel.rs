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

use cards::{HandRank, Player, combo_cards};

use crate::hands::PostflopHands;

/// A build-time prepared entry in the root support union. The two cards and
/// both seat-local indices replace the historical global combo id at the same
/// eight-byte footprint. Tables retain (rank, global combo) order; local IDs
/// stay fixed across subsequent public deals.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct RankEntry {
    pub(crate) rank: HandRank,
    cards: [u8; 2],
    indices: [u16; 2],
}

const ABSENT: u16 = u16::MAX;
const _: () = assert!(std::mem::size_of::<RankEntry>() == 8);

impl RankEntry {
    pub(crate) fn new(rank: HandRank, combo: u32, hands: &PostflopHands) -> Self {
        let (a, b) = combo_cards(combo as usize);
        Self {
            rank,
            cards: [a.index() as u8, b.index() as u8],
            indices: Player::BOTH
                .map(|p| hands.index(p, combo as usize).map_or(ABSENT, |i| i as u16)),
        }
    }

    pub(crate) fn index(self, player: Player) -> Option<usize> {
        let index = self.indices[player.index()];
        (index != ABSENT).then_some(index as usize)
    }

    fn reach(self, opponent: Player, reach: &[f32]) -> f64 {
        self.index(opponent).map_or(0.0, |i| reach[i] as f64)
    }
}

fn compact_compat_sums(
    sorted: &[RankEntry],
    opponent: Player,
    opp_reach: &[f32],
) -> (f64, [f64; 52]) {
    let mut total = 0.0;
    let mut per_card = [0.0; 52];
    for &entry in sorted {
        let r = entry.reach(opponent, opp_reach);
        if r != 0.0 {
            total += r;
            per_card[entry.cards[0] as usize] += r;
            per_card[entry.cards[1] as usize] += r;
        }
    }
    (total, per_card)
}

/// Showdown evaluation with seat-local reach and CFV indices.
/// The linear equal-rank sweep and every floating-point accumulation order
/// match `showdown_kernel`, without expanding either vector to 1,326 hands.
pub(crate) fn showdown_kernel_compact(
    sorted: &[RankEntry],
    utilities: [f64; 3],
    player: Player,
    opp_reach: &[f32],
    out: &mut [f32],
) {
    let opponent = player.opponent();
    let (all_total, all_card) = compact_compat_sums(sorted, opponent, opp_reach);
    let mut below_total = 0.0;
    let mut below_card = [0.0; 52];
    let mut group_start = 0;
    while group_start < sorted.len() {
        let rank = sorted[group_start].rank;
        let mut group_end = group_start;
        while group_end < sorted.len() && sorted[group_end].rank == rank {
            group_end += 1;
        }
        let group = &sorted[group_start..group_end];
        let (group_total, group_card) = compact_compat_sums(group, opponent, opp_reach);
        for &entry in group {
            let Some(index) = entry.index(player) else {
                continue;
            };
            let [a, b] = entry.cards.map(usize::from);
            let own_reach = entry.reach(opponent, opp_reach);
            let win = below_total - below_card[a] - below_card[b];
            let tie = group_total - group_card[a] - group_card[b] + own_reach;
            let compat = all_total - all_card[a] - all_card[b] + own_reach;
            let lose = compat - win - tie;
            out[index] = (utilities[0] * win + utilities[1] * tie + utilities[2] * lose) as f32;
        }
        below_total += group_total;
        // Keep combo-wise additions, rather than adding the aggregated group
        // card sums, to preserve the historical floating-point order.
        for &entry in group {
            let r = entry.reach(opponent, opp_reach);
            if r != 0.0 {
                below_card[entry.cards[0] as usize] += r;
                below_card[entry.cards[1] as usize] += r;
            }
        }
        group_start = group_end;
    }
}

/// Fold evaluation with seat-local reach and CFV indices.
pub(crate) fn fold_kernel_compact(
    sorted: &[RankEntry],
    utility: f64,
    player: Player,
    opp_reach: &[f32],
    out: &mut [f32],
) {
    let opponent = player.opponent();
    let (all_total, all_card) = compact_compat_sums(sorted, opponent, opp_reach);
    for &entry in sorted {
        if let Some(index) = entry.index(player) {
            let [a, b] = entry.cards.map(usize::from);
            let compat = all_total - all_card[a] - all_card[b] + entry.reach(opponent, opp_reach);
            out[index] = (utility * compat) as f32;
        }
    }
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

#[cfg(test)]
#[path = "kernel_tests.rs"]
mod tests;
