//! Terminal evaluation kernels shared by every postflop terminal (fold and
//! showdown, at any street). The multi-street builder and river shim share
//! the same mass semantics, including exact subtraction at wide reach ratios.
//!
//! Invariant relied on by both kernels: by the time a reach vector reaches
//! a terminal, the engine's chance-node masks have already zeroed the
//! opponent's reach on every combo blocked by a board card dealt on the
//! path to that terminal. A kernel may therefore be handed a `sorted` combo
//! list that is a *superset* of the combos actually live at this specific
//! terminal — the fold kernel in particular is handed one list per game
//! (disjoint only from the game's starting board) and reused for fold
//! terminals at every street and every runout, rather than a separate exact
//! list per board — and the inclusion-exclusion sums below stay exactly
//! correct, because every "dead" combo the superset drags in contributes
//! zero.

use cards::{HandRank, Player, combo_cards};

use crate::compatibility::ExactSums;
use crate::hands::PostflopHands;
use crate::mass::{
    ExactMass, Mass, MassAnalysis, MassScale, MassWidth, analyze_mass, f64_mass_is_exact,
};

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
/// The linear equal-rank sweep preserves the historical float operation order
/// when its masses are exactly representable; otherwise it subtracts integer
/// masses before converting them. Neither vector is expanded to 1,326 hands.
pub(crate) fn showdown_kernel_compact(
    sorted: &[RankEntry],
    utilities: [f64; 3],
    player: Player,
    opp_reach: &[f32],
    out: &mut [f32],
) {
    let opponent = player.opponent();
    let analysis = analyze_mass(opp_reach);
    if !analysis.is_f64_exact() {
        showdown_compact_exact(sorted, utilities, player, opp_reach, out, analysis);
        return;
    }
    let (all_total, all_card) = compact_compat_sums(sorted, opponent, opp_reach);
    let mut below_total = 0.0;
    let mut below_card = [0.0; 52];
    let mut group_card = [0.0; 52];
    let mut group_start = 0;
    while group_start < sorted.len() {
        let rank = sorted[group_start].rank;
        let mut group_end = group_start;
        while group_end < sorted.len() && sorted[group_end].rank == rank {
            group_end += 1;
        }
        let group = &sorted[group_start..group_end];
        // Keep one scratch array for the sweep. The preceding group cleared
        // every card it touched after all tie values had consumed its sums.
        let mut group_total = 0.0;
        for &entry in group {
            let r = entry.reach(opponent, opp_reach);
            if r != 0.0 {
                group_total += r;
                group_card[entry.cards[0] as usize] += r;
                group_card[entry.cards[1] as usize] += r;
            }
        }
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
                // These sums are dead after tie evaluation. Repeated cards
                // may be cleared more than once; untouched cards stay zero.
                group_card[entry.cards[0] as usize] = 0.0;
                group_card[entry.cards[1] as usize] = 0.0;
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
    let analysis = analyze_mass(opp_reach);
    if !analysis.is_f64_exact() {
        fold_compact_exact(sorted, utility, player, opp_reach, out, analysis);
        return;
    }
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
#[cfg(test)]
pub(crate) fn showdown_kernel(
    sorted: &[(HandRank, u32)],
    u_win: f64,
    u_tie: f64,
    u_lose: f64,
    opp_reach: &[f32],
    out: &mut [f32],
) {
    if !f64_mass_is_exact(opp_reach) {
        showdown_exact(
            sorted,
            |entry| entry.0,
            |&(_, combo)| global_entry(combo, opp_reach),
            |index, [win, tie, lose]| {
                out[index] =
                    (u_win * win.to_f64() + u_tie * tie.to_f64() + u_lose * lose.to_f64()) as f32;
            },
        );
        return;
    }
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
#[cfg(test)]
pub(crate) fn fold_kernel(sorted: &[(HandRank, u32)], u: f64, opp_reach: &[f32], out: &mut [f32]) {
    if !f64_mass_is_exact(opp_reach) {
        fold_exact(sorted, |&(_, combo)| global_entry(combo, opp_reach), u, out);
        return;
    }
    let (all_total, all_card) = compat_sums(sorted, opp_reach);
    for &(_, combo) in sorted {
        let (c1, c2) = combo_cards(combo as usize);
        let compat = all_total - all_card[c1.index()] - all_card[c2.index()]
            + opp_reach[combo as usize] as f64;
        out[combo as usize] = (u * compat) as f32;
    }
}

struct EntryReach {
    cards: [usize; 2],
    reach: f32,
    output: Option<usize>,
}

fn global_entry(combo: u32, reach: &[f32]) -> EntryReach {
    let index = combo as usize;
    let (a, b) = combo_cards(index);
    EntryReach {
        cards: [a.index(), b.index()],
        reach: reach[index],
        output: Some(index),
    }
}

fn exact_sums<T, M: ExactMass>(
    table: &[T],
    describe: &impl Fn(&T) -> EntryReach,
    scale: MassScale,
) -> ExactSums<M> {
    let mut sums = ExactSums::default();
    for entry in table {
        let entry = describe(entry);
        if entry.reach != 0.0 {
            sums.add_mass(entry.cards, M::from_f32_scaled(entry.reach, scale));
        }
    }
    sums
}

// Keep the larger integer scratch space outside the f64 path's stack frame.
#[inline(never)]
fn showdown_exact<T>(
    sorted: &[T],
    rank: impl Fn(&T) -> HandRank,
    describe: impl Fn(&T) -> EntryReach,
    write: impl FnMut(usize, [Mass; 3]),
) {
    showdown_integer(sorted, rank, describe, MassScale::WIDE, write);
}

#[inline(never)]
fn showdown_integer<T, M: ExactMass>(
    sorted: &[T],
    rank: impl Fn(&T) -> HandRank,
    describe: impl Fn(&T) -> EntryReach,
    scale: MassScale,
    mut write: impl FnMut(usize, [M; 3]),
) {
    let all = exact_sums::<_, M>(sorted, &describe, scale);
    let mut below = ExactSums::<M>::default();
    let mut start = 0;
    while start < sorted.len() {
        let strength = rank(&sorted[start]);
        let mut end = start + 1;
        while end < sorted.len() && rank(&sorted[end]) == strength {
            end += 1;
        }
        let group = &sorted[start..end];
        let tied = exact_sums::<_, M>(group, &describe, scale);
        for entry in group {
            let entry = describe(entry);
            if let Some(index) = entry.output {
                let own_reach = M::from_f32_scaled(entry.reach, scale);
                let win = below.compatible_mass(entry.cards, M::ZERO);
                let tie = tied.compatible_mass(entry.cards, own_reach);
                let compatible = all.compatible_mass(entry.cards, own_reach);
                // Subtract before conversion: tiny losing mass must survive
                // beside a large win or tie, just like card removal itself.
                let lose = compatible.sub(win).sub(tie);
                write(index, [win, tie, lose]);
            }
        }
        for entry in group {
            let entry = describe(entry);
            if entry.reach != 0.0 {
                below.add_mass(entry.cards, M::from_f32_scaled(entry.reach, scale));
            }
        }
        start = end;
    }
}

#[cfg(test)]
#[inline(never)]
fn fold_exact<T>(sorted: &[T], describe: impl Fn(&T) -> EntryReach, utility: f64, out: &mut [f32]) {
    fold_integer::<_, Mass>(sorted, describe, MassScale::WIDE, utility, out);
}

#[inline(never)]
fn fold_integer<T, M: ExactMass>(
    sorted: &[T],
    describe: impl Fn(&T) -> EntryReach,
    scale: MassScale,
    utility: f64,
    out: &mut [f32],
) {
    let all = exact_sums::<_, M>(sorted, &describe, scale);
    for entry in sorted {
        let entry = describe(entry);
        if let Some(index) = entry.output {
            let mass = all.compatible_mass(entry.cards, M::from_f32_scaled(entry.reach, scale));
            out[index] = (utility * mass.to_f64_scaled(scale)) as f32;
        }
    }
}

// Reuse the unchanged gate's analysis instead of scanning reach a second time.
// The common f64 path does not construct an exponent scale. Dispatch once per
// terminal; each bucket stores just a u64, u128, or five-word Mass.
#[inline(never)]
fn showdown_compact_exact(
    sorted: &[RankEntry],
    utilities: [f64; 3],
    player: Player,
    reach: &[f32],
    out: &mut [f32],
    analysis: MassAnalysis,
) {
    let (width, scale) = analysis.integer();
    match width {
        MassWidth::U64 => {
            showdown_compact_integer::<u64>(sorted, utilities, player, reach, scale, out)
        }
        MassWidth::U128 => {
            showdown_compact_integer::<u128>(sorted, utilities, player, reach, scale, out)
        }
        MassWidth::Wide => {
            showdown_compact_integer::<Mass>(sorted, utilities, player, reach, scale, out)
        }
    }
}

fn showdown_compact_integer<M: ExactMass>(
    sorted: &[RankEntry],
    utilities: [f64; 3],
    player: Player,
    reach: &[f32],
    scale: MassScale,
    out: &mut [f32],
) {
    showdown_integer::<_, M>(
        sorted,
        |entry| entry.rank,
        |entry| EntryReach {
            cards: entry.cards.map(usize::from),
            reach: entry.index(player.opponent()).map_or(0.0, |i| reach[i]),
            output: entry.index(player),
        },
        scale,
        |index, [win, tie, lose]| {
            out[index] = (utilities[0] * win.to_f64_scaled(scale)
                + utilities[1] * tie.to_f64_scaled(scale)
                + utilities[2] * lose.to_f64_scaled(scale)) as f32;
        },
    );
}

#[inline(never)]
fn fold_compact_exact(
    sorted: &[RankEntry],
    utility: f64,
    player: Player,
    reach: &[f32],
    out: &mut [f32],
    analysis: MassAnalysis,
) {
    let (width, scale) = analysis.integer();
    let describe = |entry: &RankEntry| EntryReach {
        cards: entry.cards.map(usize::from),
        reach: entry.index(player.opponent()).map_or(0.0, |i| reach[i]),
        output: entry.index(player),
    };
    match width {
        MassWidth::U64 => fold_integer::<_, u64>(sorted, describe, scale, utility, out),
        MassWidth::U128 => fold_integer::<_, u128>(sorted, describe, scale, utility, out),
        MassWidth::Wide => fold_integer::<_, Mass>(sorted, describe, scale, utility, out),
    }
}

/// Cold reporting path: preserve fractional subnormal equity numerators by
/// keeping masses and the half-tie payoff in f64 until the final ratio.
pub(crate) fn equity_mass_f64(
    sorted: &[(HandRank, u32)],
    opp_reach: &[f32],
    numerator: &mut [f64],
    denominator: &mut [f64],
) {
    if f64_mass_is_exact(opp_reach) {
        let (all_total, all_card) = compat_sums(sorted, opp_reach);
        let mut below_total = 0.0;
        let mut below_card = [0.0; 52];
        let mut start = 0;
        while start < sorted.len() {
            let rank = sorted[start].0;
            let mut end = start + 1;
            while end < sorted.len() && sorted[end].0 == rank {
                end += 1;
            }
            let group = &sorted[start..end];
            let (tied_total, tied_card) = compat_sums(group, opp_reach);
            for &(_, combo) in group {
                let index = combo as usize;
                let (a, b) = combo_cards(index);
                let [a, b] = [a.index(), b.index()];
                let win = below_total - below_card[a] - below_card[b];
                let tie = tied_total - tied_card[a] - tied_card[b] + opp_reach[index] as f64;
                let compatible = all_total - all_card[a] - all_card[b] + opp_reach[index] as f64;
                numerator[index] += win + 0.5 * tie;
                denominator[index] += compatible;
            }
            below_total += tied_total;
            for &(_, combo) in group {
                let entry = global_entry(combo, opp_reach);
                if entry.reach != 0.0 {
                    for card in entry.cards {
                        below_card[card] += entry.reach as f64;
                    }
                }
            }
            start = end;
        }
        return;
    }
    showdown_exact(
        sorted,
        |entry| entry.0,
        |&(_, combo)| global_entry(combo, opp_reach),
        |index, [win, tie, lose]| {
            numerator[index] += win.to_f64() + 0.5 * tie.to_f64();
            denominator[index] += win.add(tie).add(lose).to_f64();
        },
    );
}

#[cfg(test)]
#[path = "kernel_tests.rs"]
mod tests;

#[cfg(test)]
mod sparse_rank_group_tests {
    use super::{ABSENT, RankEntry, analyze_mass, showdown_kernel_compact};
    use cards::{HandRank, Player};

    // Frozen pre-candidate F64 accumulation order. This is an operation-order
    // reference for this research patch, not the independent poker oracle.
    fn reference_sums(
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

    fn reference_compact(
        sorted: &[RankEntry],
        utilities: [f64; 3],
        player: Player,
        opp_reach: &[f32],
        out: &mut [f32],
    ) {
        let opponent = player.opponent();
        assert!(analyze_mass(opp_reach).is_f64_exact());
        let (all_total, all_card) = reference_sums(sorted, opponent, opp_reach);
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
            let (group_total, group_card) = reference_sums(group, opponent, opp_reach);
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

    fn table(rows: &[(u16, [u8; 2], [bool; 2])]) -> (Vec<RankEntry>, [usize; 2]) {
        let mut dimensions = [0; 2];
        let mut entries = Vec::new();
        for &(rank, cards, present) in rows {
            assert_ne!(cards[0], cards[1]);
            assert!(present.into_iter().any(|value| value));
            let indices = std::array::from_fn(|seat| {
                if present[seat] {
                    let local = dimensions[seat] as u16;
                    dimensions[seat] += 1;
                    local
                } else {
                    ABSENT
                }
            });
            entries.push(RankEntry {
                rank: HandRank(rank),
                cards,
                indices,
            });
        }
        assert!(entries.windows(2).all(|pair| pair[0].rank <= pair[1].rank));
        (entries, dimensions)
    }

    fn compare(sorted: &[RankEntry], dimensions: [usize; 2], reaches: [Vec<f32>; 2]) {
        for player in Player::BOTH {
            let reach = &reaches[player.opponent().index()];
            assert_eq!(reach.len(), dimensions[player.opponent().index()]);
            assert!(
                analyze_mass(reach).is_f64_exact(),
                "fixture escaped F64 branch"
            );
            for utilities in [
                [100.0, 0.0, -100.0],
                [93.0, -7.0, -107.0],
                [0.1, -3.25, 2.5],
            ] {
                // Unrepresented output slots must retain their payload as well.
                let sentinel = f32::from_bits(0x7fc0_0123);
                let mut expected = vec![sentinel; dimensions[player.index()] + 2];
                let mut actual = expected.clone();
                reference_compact(sorted, utilities, player, reach, &mut expected);
                showdown_kernel_compact(sorted, utilities, player, reach, &mut actual);
                let expected: Vec<_> = expected.iter().map(|value| value.to_bits()).collect();
                let actual: Vec<_> = actual.iter().map(|value| value.to_bits()).collect();
                assert_eq!(
                    actual, expected,
                    "player={player:?}, utilities={utilities:?}"
                );
            }
        }
    }

    fn repeated_cards() -> (Vec<RankEntry>, [usize; 2]) {
        table(&[
            (10, [0, 1], [true, true]),
            (10, [2, 3], [false, true]),
            (10, [4, 5], [true, false]),
            (10, [6, 7], [false, true]),
            (20, [0, 2], [true, true]),
            (20, [1, 3], [true, true]),
            (30, [0, 4], [true, true]),
            (30, [2, 5], [false, true]),
            (30, [8, 9], [true, false]),
            (40, [1, 8], [true, true]),
            (40, [4, 9], [false, true]),
            (50, [0, 10], [true, false]),
            (60, [1, 11], [false, true]),
            (70, [0, 12], [true, true]),
        ])
    }

    #[test]
    fn sparse_rank_groups_repeated_cards_and_asymmetric_support_match_old_bits() {
        let (sorted, dimensions) = repeated_cards();
        assert_ne!(dimensions[0], dimensions[1]);
        let reaches = dimensions.map(|n| (0..n).map(|i| ((i % 7) + 1) as f32 * 0.125).collect());
        compare(&sorted, dimensions, reaches);
    }

    #[test]
    fn sparse_rank_groups_zero_missing_and_tied_mass_match_old_bits() {
        let (sorted, dimensions) = repeated_cards();
        for active in [None, Some(0), Some(1), Some(4), Some(7)] {
            let reaches = dimensions.map(|n| {
                (0..n)
                    .map(|i| {
                        if active == Some(i) {
                            0.375
                        } else if i % 2 == 0 {
                            0.0
                        } else {
                            -0.0
                        }
                    })
                    .collect()
            });
            compare(&sorted, dimensions, reaches);
        }
    }

    #[test]
    fn sparse_rank_groups_dense_ties_fractional_and_subnormal_mass_match_old_bits() {
        let mut rows = Vec::new();
        for a in 0..16 {
            for b in (a + 1)..16 {
                let i = rows.len();
                rows.push((
                    (i / 12) as u16,
                    [a, b],
                    [i % 5 != 0 || i % 3 == 0, i % 3 != 0],
                ));
            }
        }
        let (sorted, dimensions) = table(&rows);
        for subnormal in [false, true] {
            let reaches = dimensions.map(|n| {
                (0..n)
                    .map(|i| {
                        if i % 4 == 0 {
                            0.0
                        } else if subnormal {
                            f32::from_bits((i % 15 + 1) as u32)
                        } else {
                            f32::from_bits(0x3e00_0000 + ((i * 2971) % 0x1f_ffff) as u32)
                        }
                    })
                    .collect()
            });
            compare(&sorted, dimensions, reaches);
        }
    }

    #[test]
    fn sparse_rank_groups_empty_table_keeps_outputs() {
        compare(&[], [0, 0], [vec![], vec![]]);
    }
}
