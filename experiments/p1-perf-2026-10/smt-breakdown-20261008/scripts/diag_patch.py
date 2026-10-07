"""Throwaway diagnostic build (never committed; results are meaningless, only timing is used).

Usage: python3 diag_patch.py <tree>. After p1_bench's warmup, the env var P1DIAG (bit mask) disables
parts of every CFR pass for the timed iterations, so the strategy stays frozen at the warmup state:
 1 terminal kernels (eval_cfr), 2 regret updates, 4 strategy-sum accumulation.
The time removed by each part at 16 and 32 threads shows which work limits the SMT configuration.
"""
import sys
from pathlib import Path

root = Path(sys.argv[1])

lib = root / "crates/hu-engine/src/lib.rs"
s = lib.read_text()
s = s.replace("mod mccfr;\n", """mod mccfr;
pub mod diag {
    use std::sync::atomic::{AtomicU8, Ordering::Relaxed};
    pub static SKIP: AtomicU8 = AtomicU8::new(0);
    pub fn skip(bit: u8) -> bool {
        SKIP.load(Relaxed) & bit != 0
    }
}
""", 1)
lib.write_text(s)

sv = root / "crates/hu-engine/src/solver.rs"
s = sv.read_text()
for old, new in [
    ("                ctx.evaluator.eval_cfr(node.aux, ctx.p, opp_reach, out);\n",
     "                if !crate::diag::skip(1) {\n                    ctx.evaluator.eval_cfr(node.aux, ctx.p, opp_reach, out);\n                }\n"),
    ("            views\n                .own()\n                .update_regrets(sref, sref.index, &cfvs, ctx.discounts);\n",
     "            if !crate::diag::skip(2) {\n                views\n                    .own()\n                    .update_regrets(sref, sref.index, &cfvs, ctx.discounts);\n            }\n"),
    ("            views\n                .own()\n                .accumulate_strategy(sref, sref.index, &cfvs, ctx.discounts);\n",
     "            if !crate::diag::skip(4) {\n                views\n                    .own()\n                    .accumulate_strategy(sref, sref.index, &cfvs, ctx.discounts);\n            }\n"),
]:
    assert s.count(old) == 1, old
    s = s.replace(old, new, 1)
sv.write_text(s)

bench = root / "crates/hu-postflop/examples/p1_bench.rs"
s = bench.read_text()
a = "    let warmup_secs = t.elapsed().as_secs_f64();\n"
assert s.count(a) == 1
s = s.replace(a, a + """    let diag: u8 = std::env::var("P1DIAG").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    hu_engine::diag::SKIP.store(diag, std::sync::atomic::Ordering::Relaxed);
""", 1)
bench.write_text(s)
print("patched")
