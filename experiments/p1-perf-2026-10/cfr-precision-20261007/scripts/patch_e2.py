"""Add bit-identical T8a kernel candidates to the prototype copy in e2/ (measurement only)."""
import sys

root = sys.argv[1]
k = root + "/crates/hu-postflop/src/kernel.rs"
t = open(k, encoding="utf-8").read()
t = t.replace("""    Exact,
    F64Fold,
    F32,
}""", """    Exact,
    F64Fold,
    F32,
    T8aL,
    T8aLB,
    T8aLW,
    T8aLBW,
}""", 1)
t = t.replace("""            Ok(s) if s == "f32" => Self::F32,""", """            Ok(s) if s == "f32" => Self::F32,
            Ok(s) if s == "t8a_l" => Self::T8aL,
            Ok(s) if s == "t8a_lb" => Self::T8aLB,
            Ok(s) if s == "t8a_lw" => Self::T8aLW,
            Ok(s) if s == "t8a_lbw" => Self::T8aLBW,""", 1)
t = t.replace("""            Self::F32 => "f32",""", """            Self::F32 => "f32",
            Self::T8aL => "t8a_l",
            Self::T8aLB => "t8a_lb",
            Self::T8aLW => "t8a_lw",
            Self::T8aLBW => "t8a_lbw",""", 1)
marker = "#[cfg(test)]\nmod legacy {"
assert marker in t
code = r'''const T8A_ZERO: [f64; 52] = [0.0; 52];
#[inline(always)]
fn t8a_add<const B: bool>(hands: &[Hand], reach: &[f32], total: &mut f64, card: &mut [f64; 52]) {
    if B {
        add_relaxed_f64(hands, reach, total, card)
    } else {
        add(hands, reach, total, card)
    }
}
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn t8a_emit(
    h: Hand,
    same: &[u16],
    reach: &[f32],
    u: [f64; 3],
    win: f64,
    group_total: f64,
    group_card: &[f64; 52],
    all_total: f64,
    all_card: &[f64; 52],
    out: &mut [f32],
) {
    let [a, b] = h.cards.map(usize::from);
    let same = same_reach(same, h, reach);
    let tie = group_total - group_card[a] - group_card[b] + same;
    let compat = all_total - all_card[a] - all_card[b] + same;
    let lose = compat - win - tie;
    out[h.local as usize] = (u[0] * win + u[1] * tie + u[2] * lose) as f32;
}
/// T8a candidates, bit-identical to `showdown_kernel`: lazy group zeroing,
/// `B` branchless sums, `W` win-first single scan of tied groups.
pub(crate) fn showdown_kernel_t8a<const B: bool, const W: bool>(
    own: &RankedHands,
    opp: &RankedHands,
    same: &[u16],
    utilities: [f64; 3],
    reach: &[f32],
    out: &mut [f32],
) {
    let mut all_total = 0.0;
    let mut all_card = [0.0; 52];
    t8a_add::<B>(&opp.hands, reach, &mut all_total, &mut all_card);
    let mut below_total = 0.0;
    let mut below_card = [0.0; 52];
    let mut wins = [0.0f64; 64];
    let mut oi = 0;
    let mut os = 0;
    let mut start = 0;
    for &(rank, end) in &own.groups {
        while oi < opp.groups.len() && opp.groups[oi].0 < rank {
            let oe = opp.groups[oi].1;
            let mut group_total = 0.0;
            t8a_add::<B>(&opp.hands[os..oe], reach, &mut group_total, &mut below_card);
            below_total += group_total;
            os = oe;
            oi += 1;
        }
        let tied = oi < opp.groups.len() && opp.groups[oi].0 == rank;
        let mine = &own.hands[start..end];
        if !tied {
            for &h in mine {
                let [a, b] = h.cards.map(usize::from);
                let win = below_total - below_card[a] - below_card[b];
                t8a_emit(h, same, reach, utilities, win, 0.0, &T8A_ZERO, all_total, &all_card, out);
            }
        } else {
            let oe = opp.groups[oi].1;
            let group = &opp.hands[os..oe];
            let mut group_total = 0.0;
            let mut group_card = [0.0; 52];
            if W && mine.len() <= wins.len() {
                for (w, h) in wins.iter_mut().zip(mine) {
                    let [a, b] = h.cards.map(usize::from);
                    *w = below_total - below_card[a] - below_card[b];
                }
                for h in group {
                    let r = reach[h.local as usize] as f64;
                    if B || r != 0.0 {
                        group_total += r;
                        let [a, b] = h.cards.map(usize::from);
                        group_card[a] += r;
                        group_card[b] += r;
                        below_card[a] += r;
                        below_card[b] += r;
                    }
                }
                for (&w, &h) in wins.iter().zip(mine) {
                    t8a_emit(h, same, reach, utilities, w, group_total, &group_card, all_total, &all_card, out);
                }
            } else {
                t8a_add::<B>(group, reach, &mut group_total, &mut group_card);
                for &h in mine {
                    let [a, b] = h.cards.map(usize::from);
                    let win = below_total - below_card[a] - below_card[b];
                    t8a_emit(h, same, reach, utilities, win, group_total, &group_card, all_total, &all_card, out);
                }
                let mut unused = 0.0;
                t8a_add::<B>(group, reach, &mut unused, &mut below_card);
            }
            below_total += group_total;
            os = oe;
            oi += 1;
        }
        start = end;
    }
}

'''
t = t.replace(marker, code + marker, 1)
open(k, "w", encoding="utf-8", newline="\n").write(t)

p = root + "/crates/hu-postflop/src/postflop.rs"
s = open(p, encoding="utf-8").read()
a = """                    kernel::CfrKernel::F32 => kernel::fold_kernel_relaxed_f32,"""
assert a in s
s = s.replace(a, a + """
                    kernel::CfrKernel::T8aL | kernel::CfrKernel::T8aLW => kernel::fold_kernel,
                    kernel::CfrKernel::T8aLB | kernel::CfrKernel::T8aLBW => {
                        kernel::fold_kernel_relaxed_f64
                    }""", 1)
b = """                    kernel::CfrKernel::F32 => kernel::showdown_kernel_relaxed_f32,"""
assert b in s
s = s.replace(b, b + """
                    kernel::CfrKernel::T8aL => kernel::showdown_kernel_t8a::<false, false>,
                    kernel::CfrKernel::T8aLB => kernel::showdown_kernel_t8a::<true, false>,
                    kernel::CfrKernel::T8aLW => kernel::showdown_kernel_t8a::<false, true>,
                    kernel::CfrKernel::T8aLBW => kernel::showdown_kernel_t8a::<true, true>,""", 1)
open(p, "w", encoding="utf-8", newline="\n").write(s)
print("patched")
