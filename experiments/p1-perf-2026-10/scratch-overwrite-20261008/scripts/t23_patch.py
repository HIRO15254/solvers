"""T23: skip zeroing scratch buffers that cfr_pass fully overwrites before reading (bit-identical)."""
import sys
from pathlib import Path

root = Path(sys.argv[1])


def patch(rel, edits):
    p = root / rel
    s = p.read_text(encoding="utf-8")
    for old, new in edits:
        assert s.count(old) == 1, (rel, old[:160])
        s = s.replace(old, new, 1)
    p.write_text(s, encoding="utf-8", newline="\n")


patch("crates/hu-engine/src/scratch.rs", [
    ("""    /// Returns a buffer to the pool for reuse.
""", """    /// As [`Self::take`], but reused elements are not zeroed: the caller
    /// must write every element before reading any. Only growth beyond the
    /// buffer's previous length is zero-filled. Debug builds fill the whole
    /// buffer with NaN instead, so a missed write shows up in tests.
    pub fn take_overwrite(&mut self, len: usize) -> Vec<f32> {
        let mut buf = self.free.pop().unwrap_or_default();
        if cfg!(debug_assertions) {
            buf.clear();
            buf.resize(len, f32::NAN);
        } else if buf.len() >= len {
            buf.truncate(len);
        } else {
            buf.resize(len, 0.0);
        }
        buf
    }

    /// Returns a buffer to the pool for reuse.
"""),
])

patch("crates/hu-engine/src/solver.rs", [
    # Chance node, parallel deals: map_reach_into writes every element.
    ("""                            let mut my_next = scratch.take(row.len());
                            let mut opp_next = scratch.take(opp_dim);
                            ctx.tree
                                .map_reach_into(deal.maps[ctx.p], my_reach, &mut my_next);
""", """                            let mut my_next = scratch.take_overwrite(row.len());
                            let mut opp_next = scratch.take_overwrite(opp_dim);
                            ctx.tree
                                .map_reach_into(deal.maps[ctx.p], my_reach, &mut my_next);
"""),
    # Chance node, sequential deals.
    ("""                    let mut my_next = scratch.take(my_dim);
                    let mut opp_next = scratch.take(opp_dim);
                    ctx.tree
                        .map_reach_into(deal.maps[ctx.p], my_reach, &mut my_next);
                    ctx.tree
                        .map_reach_into(deal.maps[ctx.p.opponent()], opp_reach, &mut opp_next);
""", """                    let mut my_next = scratch.take_overwrite(my_dim);
                    let mut opp_next = scratch.take_overwrite(opp_dim);
                    ctx.tree
                        .map_reach_into(deal.maps[ctx.p], my_reach, &mut my_next);
                    ctx.tree
                        .map_reach_into(deal.maps[ctx.p.opponent()], opp_reach, &mut opp_next);
"""),
    # Own node: regret matching writes the whole strategy.
    ("""            let mut sigma = scratch.take(sref.len());
            views
                .own()
                .regret_matching_cfr(sref, sref.index, &mut sigma, ctx.cfr_precision);
""", """            let mut sigma = scratch.take_overwrite(sref.len());
            views
                .own()
                .regret_matching_cfr(sref, sref.index, &mut sigma, ctx.cfr_precision);
"""),
    ("""            let mut my_next = scratch.take(num_hands);

            if !out.is_empty() && parallel_actions(ctx.tree, node_id) {
""", """            let mut my_next = scratch.take_overwrite(num_hands);

            if !out.is_empty() && parallel_actions(ctx.tree, node_id) {
"""),
    ("""                            let mut reach = scratch.take(num_hands);
                            mul_into(&mut reach, my_reach, &sigma[a * num_hands..]);
""", """                            let mut reach = scratch.take_overwrite(num_hands);
                            mul_into(&mut reach, my_reach, &sigma[a * num_hands..]);
"""),
    # Opponent node.
    ("""            let mut sigma = scratch.take(if zero_opp { 0 } else { sref.len() });
""", """            let mut sigma = scratch.take_overwrite(if zero_opp { 0 } else { sref.len() });
"""),
    ("""            // `take` initializes this reusable buffer to zero. In the
            // zero-reach case it is shared immutably by parallel children.
            let mut opp_next = scratch.take(num_hands);
""", """            // In the zero-reach case this reusable buffer stays zero and is
            // shared immutably by parallel children; otherwise every use
            // overwrites it first.
            let mut opp_next = if zero_opp {
                scratch.take(num_hands)
            } else {
                scratch.take_overwrite(num_hands)
            };
"""),
    ("""                                let mut reach = scratch.take(num_hands);
                                mul_into(&mut reach, opp_reach, &sigma[a * num_hands..]);
""", """                                let mut reach = scratch.take_overwrite(num_hands);
                                mul_into(&mut reach, opp_reach, &sigma[a * num_hands..]);
"""),
])
print("patched")
