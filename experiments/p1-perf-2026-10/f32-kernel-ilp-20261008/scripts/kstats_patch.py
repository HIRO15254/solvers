"""Throwaway instrumentation (never committed): count zero opponent reach at CFR terminals.

Usage: python3 kstats_patch.py <tree>. Adds hu_postflop::kstats counters, counts per f32 CFR
terminal call the opponent hands in the kernel's list and how many have zero reach, and makes
p1_bench reset the counters after warmup and print them after the timed iterations.
"""
import sys
from pathlib import Path

root = Path(sys.argv[1])

lib = root / "crates/hu-postflop/src/lib.rs"
s = lib.read_text()
s = s.replace("pub mod game;\n", """pub mod game;
pub mod kstats {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    // 0 fold calls, 1 fold list entries, 2 fold zero entries,
    // 3 showdown calls, 4 showdown list entries, 5 showdown zero entries,
    // 6 calls with >= 50% zeros, 7 calls with >= 90% zeros.
    pub static C: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
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

pf = root / "crates/hu-postflop/src/postflop.rs"
s = pf.read_text()
anchor = "        match term.kind {\n            TerminalKind::Fold { .. } => {\n                let kernel = match variant {"
assert anchor in s, "anchor missing"
s = s.replace(anchor, """        if matches!(variant, hu_engine::CfrPrecision::F32) {
            let (base, list): (usize, &[Hand]) = match term.kind {
                TerminalKind::Fold { .. } => (0, self.fold_combos[p.opponent()].as_slice()),
                TerminalKind::Showdown => (3, self.rank_tables[term.table as usize][p.opponent()].hands.as_slice()),
            };
            let zeros = list.iter().filter(|h| opp_reach[h.local as usize] == 0.0).count() as u64;
            let n = list.len() as u64;
            crate::kstats::add(base, 1);
            crate::kstats::add(base + 1, n);
            crate::kstats::add(base + 2, zeros);
            if zeros * 2 >= n {
                crate::kstats::add(6, 1);
            }
            if zeros * 10 >= n * 9 {
                crate::kstats::add(7, 1);
            }
        }
""" + anchor, 1)
pf.write_text(s)

bench = root / "crates/hu-postflop/examples/p1_bench.rs"
s = bench.read_text()
a = "    solver.run(args.warmup);\n    let warmup_secs = t.elapsed().as_secs_f64();\n"
assert a in s
s = s.replace(a, a + "    hu_postflop::kstats::reset();\n", 1)
b = "    let iter_secs = t.elapsed().as_secs_f64();\n"
assert b in s
s = s.replace(b, b + '    eprintln!("KSTATS {:?}", hu_postflop::kstats::report());\n', 1)
bench.write_text(s)
print("patched")
