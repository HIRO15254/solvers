"""T22 part 2: zipped loops in value_pass/profile_pass and a vectorizable all-zero test (bit-identical)."""
import sys
from pathlib import Path

p = Path(sys.argv[1]) / "crates/hu-engine/src/solver.rs"
s = p.read_text(encoding="utf-8")
edits = [
    ("fn chance_len(tree: &PublicTree, node: NodeId, p: Player, dim: usize) -> usize {",
     "/// `v.iter().all(|&x| x == 0.0)` (both signed zeros count, NaN does not),\n"
     "/// tested 16 lanes at a time by OR-ing the bits without the sign bit.\n"
     "fn all_zero(v: &[f32]) -> bool {\n"
     "    let mut chunks = v.chunks_exact(16);\n"
     "    for chunk in &mut chunks {\n"
     "        if chunk.iter().fold(0u32, |acc, &x| acc | (x.to_bits() << 1)) != 0 {\n"
     "            return false;\n"
     "        }\n"
     "    }\n"
     "    chunks.remainder().iter().all(|&x| x == 0.0)\n"
     "}\n"
     "\n"
     "fn chance_len(tree: &PublicTree, node: NodeId, p: Player, dim: usize) -> usize {"),
    ("            if !PRUNE || !opp_reach.iter().all(|&x| x == 0.0) {\n                ctx.evaluator.eval_cfr(",
     "            if !PRUNE || !all_zero(opp_reach) {\n                ctx.evaluator.eval_cfr("),
    ("            if siblings && (!PRUNE || !opp_reach.iter().all(|&x| x == 0.0)) {",
     "            if siblings && (!PRUNE || !all_zero(opp_reach)) {"),
    ("            let zero_opp = PRUNE && opp_reach.iter().all(|&x| x == 0.0);",
     "            let zero_opp = PRUNE && all_zero(opp_reach);"),
    ("    if !RECORD && opp_reach.iter().all(|&x| x == 0.0) {",
     "    if !RECORD && all_zero(opp_reach) {"),
    ("""                        let mut reach = scratch.take(hands);
                        for h in 0..hands {
                            reach[h] = opp_reach[h] * sigma[a * hands + h];
                        }
""", """                        let mut reach = scratch.take(hands);
                        mul_into(&mut reach, opp_reach, &sigma[a * hands..]);
"""),
    ("""                for a in 0..actions {
                    let row = &mut flat[a * dim * channels..(a + 1) * dim * channels];
                    let (child_ev, child_br) = value_row::<EV, BR>(row, dim);
                    for h in 0..dim {
                        if EV {
                            if own {
                                ev[h] += sigma[a * hands + h] * child_ev[h];
                            } else {
                                ev[h] += child_ev[h];
                            }
                        }
                        if BR {
                            if own {
                                br[h] = br[h].max(child_br[h]);
                            } else {
                                br[h] += child_br[h];
                            }
                        }
                    }
                }
""", """                for a in 0..actions {
                    let row = &mut flat[a * dim * channels..(a + 1) * dim * channels];
                    let (child_ev, child_br) = value_row::<EV, BR>(row, dim);
                    // EV and BR rows are independent, so separate zipped
                    // loops keep the per-hand arithmetic exactly as before.
                    if EV {
                        let child_ev = &child_ev[..dim];
                        if own {
                            let row = &sigma[a * hands..a * hands + dim];
                            for ((e, &s), &c) in ev[..dim].iter_mut().zip(row).zip(child_ev) {
                                *e += s * c;
                            }
                        } else {
                            for (e, &c) in ev[..dim].iter_mut().zip(child_ev) {
                                *e += c;
                            }
                        }
                    }
                    if BR {
                        let child_br = &child_br[..dim];
                        if own {
                            for (b, &c) in br[..dim].iter_mut().zip(child_br) {
                                *b = b.max(c);
                            }
                        } else {
                            for (b, &c) in br[..dim].iter_mut().zip(child_br) {
                                *b += c;
                            }
                        }
                    }
                }
"""),
    ("""                for (a, child) in ctx.tree.children(node_id).enumerate() {
                    for h in 0..hands {
                        reach[h] = opp_reach[h] * sigma[a * hands + h];
                    }
""", """                for (a, child) in ctx.tree.children(node_id).enumerate() {
                    mul_into(&mut reach, opp_reach, &sigma[a * hands..]);
"""),
    ("""                    for h in 0..dim {
                        if EV {
                            ev[h] += child_ev[h];
                        }
                        if BR {
                            br[h] += child_br[h];
                        }
                    }
""", """                    if EV {
                        for (e, &c) in ev[..dim].iter_mut().zip(&child_ev[..dim]) {
                            *e += c;
                        }
                    }
                    if BR {
                        for (b, &c) in br[..dim].iter_mut().zip(&child_br[..dim]) {
                            *b += c;
                        }
                    }
"""),
    ("""                let mut next = scratch.take(hands);
                for h in 0..hands {
                    next[h] = reach[node.player][h] * sigma[a * hands + h];
                }
""", """                let mut next = scratch.take(hands);
                mul_into(&mut next, reach[node.player], &sigma[a * hands..]);
"""),
    ("""                for p in Player::BOTH {
                    for h in 0..ev[p].len() {
                        if node.player == p {
                            ev[p][h] += sigma[a * hands + h] * child[p][h];
                        } else {
                            ev[p][h] += child[p][h];
                        }
                    }
                }
""", """                for p in Player::BOTH {
                    let len = ev[p].len();
                    if node.player == p {
                        let row = &sigma[a * hands..a * hands + len];
                        for ((e, &s), &c) in ev[p].iter_mut().zip(row).zip(&child[p][..len]) {
                            *e += s * c;
                        }
                    } else {
                        for (e, &c) in ev[p].iter_mut().zip(&child[p][..len]) {
                            *e += c;
                        }
                    }
                }
"""),
    ("""#[cfg(test)]
#[path = "solver/sibling_tests.rs"]
mod sibling_tests;
""", """#[cfg(test)]
#[path = "solver/sibling_tests.rs"]
mod sibling_tests;

#[cfg(test)]
mod loop_tests {
    use super::{all_zero, mul_into};

    #[test]
    fn all_zero_matches_the_scalar_test() {
        let special = [0.0f32, -0.0, 1e-45, -1e-45, f32::NAN, f32::INFINITY, 1.0, -2.5];
        for len in [0, 1, 15, 16, 17, 31, 32, 33, 100] {
            for (i, &value) in special.iter().enumerate() {
                for at in [0, len / 2, len.saturating_sub(1)] {
                    let mut v = vec![if i % 2 == 0 { 0.0 } else { -0.0 }; len];
                    if len > 0 {
                        v[at] = value;
                    }
                    assert_eq!(all_zero(&v), v.iter().all(|&x| x == 0.0), "{len} {value} {at}");
                }
            }
        }
    }

    #[test]
    fn mul_into_matches_indexed_products() {
        let a: Vec<f32> = (0..37).map(|i| (i as f32 * 0.37).sin()).collect();
        let b: Vec<f32> = (0..40).map(|i| (i as f32 * 1.3).cos() - 0.25).collect();
        let mut dst = vec![f32::NAN; 37];
        mul_into(&mut dst, &a, &b[3..]);
        for h in 0..37 {
            assert_eq!(dst[h].to_bits(), (a[h] * b[3 + h]).to_bits());
        }
    }
}
"""),
]
for old, new in edits:
    assert s.count(old) == 1, old[:120]
    s = s.replace(old, new, 1)
p.write_text(s, encoding="utf-8", newline="\n")
print("patched")
