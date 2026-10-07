"""Throwaway diagnostic (never committed; applied on top of diag_patch.py, timing only).

Extra P1DIAG bits inside the f32 terminal kernels:
 8 showdown own-hand loop, 16 fold own-hand loop, 32 opponent passes (add_relaxed_f32),
 64 the per-group 52-card merge of a tied opponent group into the below sums.
Run with bits 2|4 as well so the strategy stays frozen at the warmup state.
"""
import sys
from pathlib import Path

root = Path(sys.argv[1])
k = root / "crates/hu-postflop/src/kernel.rs"
s = k.read_text()
edits = [
    ("fn add_relaxed_f32(hands: &[Hand], reach: &[f32], total: &mut f32, card: &mut [f32; 52]) {\n",
     "fn add_relaxed_f32(hands: &[Hand], reach: &[f32], total: &mut f32, card: &mut [f32; 52]) {\n    if hu_engine::diag::skip(32) {\n        return;\n    }\n"),
    ("""        add_relaxed_f32(&opp.hands[os..oe], reach, &mut group_total, &mut group_card);
        for &h in &own.hands[start..end] {
""", """        add_relaxed_f32(&opp.hands[os..oe], reach, &mut group_total, &mut group_card);
        let own_hands = if hu_engine::diag::skip(8) { &own.hands[start..start] } else { &own.hands[start..end] };
        for &h in own_hands {
"""),
    ("""        if tied {
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
pub(crate) fn fold_kernel_relaxed_f32(""", """        if tied {
            below_total += group_total;
            if !hu_engine::diag::skip(64) {
                for (below, group) in below_card.iter_mut().zip(group_card) {
                    *below += group;
                }
            }
            os = oe;
            oi += 1;
        }
        start = end;
    }
}
pub(crate) fn fold_kernel_relaxed_f32("""),
    ("""    let (total, card) = compat_sums_relaxed_f32(opp, reach);
    for &h in own {
        let [a, b] = h.cards.map(usize::from);
        if board & ((1u64 << a) | (1u64 << b)) != 0 {
            out[h.local as usize] = 0.0;
            continue;
        }
        out[h.local as usize] =
            u * (total - card[a] - card[b] + same_reach_relaxed_f32(same, h, reach));""",
     """    let (total, card) = compat_sums_relaxed_f32(opp, reach);
    let own = if hu_engine::diag::skip(16) { &own[..0] } else { own };
    for &h in own {
        let [a, b] = h.cards.map(usize::from);
        if board & ((1u64 << a) | (1u64 << b)) != 0 {
            out[h.local as usize] = 0.0;
            continue;
        }
        out[h.local as usize] =
            u * (total - card[a] - card[b] + same_reach_relaxed_f32(same, h, reach));"""),
]
for old, new in edits:
    assert s.count(old) == 1, old[:80]
    s = s.replace(old, new, 1)
k.write_text(s)
print("patched")
