"""T22: rewrite the indexed per-hand loops of cfr_pass as zipped slice loops (bit-identical, vectorizable)."""
import sys
from pathlib import Path

p = Path(sys.argv[1]) / "crates/hu-engine/src/solver.rs"
s = p.read_text(encoding="utf-8")
edits = [
    # helper before cfr_pass's context struct users: insert right before `fn cfr_pass`
    ("fn cfr_pass<E: TerminalEvaluator, V: StorageView, const PRUNE: bool>(",
     "/// `dst[h] = a[h] * b[h]`. Zipped equal-length slices keep the hot per-hand\n"
     "/// loops of `cfr_pass` free of bounds checks so they vectorize; elementwise\n"
     "/// products are exact, so results match the indexed loops bit for bit.\n"
     "#[inline]\n"
     "fn mul_into(dst: &mut [f32], a: &[f32], b: &[f32]) {\n"
     "    let (a, b) = (&a[..dst.len()], &b[..dst.len()]);\n"
     "    for ((d, &x), &y) in dst.iter_mut().zip(a).zip(b) {\n"
     "        *d = x * y;\n"
     "    }\n"
     "}\n"
     "\n"
     "fn cfr_pass<E: TerminalEvaluator, V: StorageView, const PRUNE: bool>("),
    ("""                            let mut reach = scratch.take(num_hands);
                            for h in 0..num_hands {
                                reach[h] = my_reach[h] * sigma[a * num_hands + h];
                            }
""", """                            let mut reach = scratch.take(num_hands);
                            mul_into(&mut reach, my_reach, &sigma[a * num_hands..]);
"""),
    ("""                    let row = &sigma[a * num_hands..(a + 1) * num_hands];
                    for h in 0..num_hands {
                        my_next[h] = my_reach[h] * row[h];
                    }
""", """                    mul_into(&mut my_next, my_reach, &sigma[a * num_hands..]);
"""),
    ("""            for a in 0..num_actions {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                for h in 0..num_hands {
                    node_cfv[h] += row[h] * cfvs[a * num_hands + h];
                }
            }

            // Instantaneous regret, computed in place.
            for a in 0..num_actions {
                for h in 0..num_hands {
                    cfvs[a * num_hands + h] -= node_cfv[h];
                }
            }
""", """            for (row, values) in sigma
                .chunks_exact(num_hands.max(1))
                .zip(cfvs.chunks_exact(num_hands.max(1)))
            {
                for ((cfv, &s), &v) in node_cfv.iter_mut().zip(row).zip(values) {
                    *cfv += s * v;
                }
            }

            // Instantaneous regret, computed in place.
            for values in cfvs.chunks_exact_mut(num_hands.max(1)) {
                for (v, &cfv) in values.iter_mut().zip(node_cfv.iter()) {
                    *v -= cfv;
                }
            }
"""),
    ("""            for a in 0..num_actions {
                for h in 0..num_hands {
                    cfvs[a * num_hands + h] = my_reach[h] * sigma[a * num_hands + h];
                }
            }
""", """            for (weighted, row) in cfvs
                .chunks_exact_mut(num_hands.max(1))
                .zip(sigma.chunks_exact(num_hands.max(1)))
            {
                mul_into(weighted, my_reach, row);
            }
"""),
    ("""                                let mut reach = scratch.take(num_hands);
                                for h in 0..num_hands {
                                    reach[h] = opp_reach[h] * sigma[a * num_hands + h];
                                }
""", """                                let mut reach = scratch.take(num_hands);
                                mul_into(&mut reach, opp_reach, &sigma[a * num_hands..]);
"""),
    ("""                    if !zero_opp {
                        let row = &sigma[a * num_hands..(a + 1) * num_hands];
                        for h in 0..num_hands {
                            opp_next[h] = opp_reach[h] * row[h];
                        }
                    }
""", """                    if !zero_opp {
                        mul_into(&mut opp_next, opp_reach, &sigma[a * num_hands..]);
                    }
"""),
    ("""                    if !zero_opp {
                        for h in 0..out.len() {
                            out[h] += child_out[h];
                        }
                    }
""", """                    if !zero_opp {
                        for (dst, &v) in out.iter_mut().zip(child_out.iter()) {
                            *dst += v;
                        }
                    }
"""),
]
for old, new in edits:
    assert s.count(old) == 1, old[:100]
    s = s.replace(old, new, 1)
p.write_text(s, encoding="utf-8", newline="\n")
print("patched")
