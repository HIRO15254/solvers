"""Throwaway instrumentation (never committed): how much of each CFR pass's own storage updates
happen under an all-zero opponent reach (and under an all-zero own reach).

Usage: python3 zstats_patch.py <tree>. Adds hu_engine::zstats counters. At every updating
player's action node in cfr_pass it adds the node's storage elements (A*H) to:
 0 all, 1 opp all-zero, 2 own all-zero, 3 both all-zero; node counts to 4..7 likewise;
 8 elements whose own hand reach is zero (strategy-sum adds 0 there), 9 the same under nonzero opp reach.
p1_bench resets the counters after warmup and prints them after the timed iterations.
"""
import sys
from pathlib import Path

root = Path(sys.argv[1])

lib = root / "crates/hu-engine/src/lib.rs"
s = lib.read_text()
s = s.replace("mod mccfr;\n", """mod mccfr;
pub mod zstats {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    pub static C: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
    pub fn reset() {
        for c in &C {
            c.store(0, Relaxed);
        }
    }
    pub fn add(i: usize, v: u64) {
        C[i].fetch_add(v, Relaxed);
    }
    pub fn report() -> Vec<u64> {
        C.iter().map(|c| c.load(Relaxed)).collect()
    }
}
""", 1)
lib.write_text(s)

sv = root / "crates/hu-engine/src/solver.rs"
s = sv.read_text()
anchor = """            let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
            debug_assert_eq!(num_hands, my_reach.len());
"""
assert s.count(anchor) == 1, "anchor missing"
s = s.replace(anchor, anchor + """            {
                let opp_zero = opp_reach.iter().all(|&x| x == 0.0);
                let own_zero = my_reach.iter().all(|&x| x == 0.0);
                let n = sref.len() as u64;
                for (i, hit) in [(0, true), (1, opp_zero), (2, own_zero), (3, opp_zero && own_zero)] {
                    if hit {
                        crate::zstats::add(i, n);
                        crate::zstats::add(i + 4, 1);
                    }
                }
                let zero_hands = my_reach.iter().filter(|&&x| x == 0.0).count() as u64;
                crate::zstats::add(8, zero_hands * num_actions as u64);
                if !opp_zero {
                    crate::zstats::add(9, zero_hands * num_actions as u64);
                }
            }
""", 1)
sv.write_text(s)

bench = root / "crates/hu-postflop/examples/p1_bench.rs"
s = bench.read_text()
a = "    solver.run(args.warmup);\n"
assert s.count(a) == 1
s = s.replace(a, a + "    hu_engine::zstats::reset();\n", 1)
b = "    let iter_secs = t.elapsed().as_secs_f64();\n"
assert s.count(b) == 1
s = s.replace(b, b + '    eprintln!("ZSTATS {:?}", hu_engine::zstats::report());\n', 1)
bench.write_text(s)
print("patched")
