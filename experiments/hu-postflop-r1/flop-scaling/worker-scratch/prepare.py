"""Prepare a single-factor invocation-owned ScratchBank research copy, without building."""
from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import re
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SOURCE = "crates/engine/src/solver.rs"
SOURCE_SHA256 = "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a"

BANK = r'''
/// Invocation-owned idle pools. An index is only a cache hint, never an
/// exclusivity proof: nested Rayon work and other pools may reuse that index.
/// At most one idle Scratch is retained per bin; active leases and their byte
/// capacities are not bounded by that count. Dropping this bank frees its bins.
struct ScratchBank {
    bins: Vec<Mutex<Option<Scratch>>>,
}

impl ScratchBank {
    fn new() -> Self {
        let workers = rayon::current_num_threads();
        Self::with_bins(if workers > 1 { workers } else { 0 })
    }

    fn with_bins(count: usize) -> Self {
        Self {
            bins: (0..count).map(|_| Mutex::new(None)).collect(),
        }
    }

    fn checkout(&self) -> ScratchLease<'_> {
        self.checkout_for(rayon::current_thread_index())
    }

    fn checkout_for(&self, index: Option<usize>) -> ScratchLease<'_> {
        let bin = index.and_then(|i| self.bins.get(i));
        if let Some(bin) = bin
            && let Ok(mut idle) = bin.try_lock()
        {
            let scratch = idle.take().unwrap_or_default();
            // No guard or RefCell borrow survives into the recursive walk.
            return ScratchLease {
                scratch,
                bin: Some(bin),
            };
        }
        // Pool-external, out-of-range, contended or poisoned: never wait.
        ScratchLease {
            scratch: Scratch::new(),
            bin: None,
        }
    }
}

struct ScratchLease<'a> {
    scratch: Scratch,
    bin: Option<&'a Mutex<Option<Scratch>>>,
}

impl Deref for ScratchLease<'_> {
    type Target = Scratch;

    fn deref(&self) -> &Self::Target {
        &self.scratch
    }
}

impl DerefMut for ScratchLease<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.scratch
    }
}

impl Drop for ScratchLease<'_> {
    fn drop(&mut self) {
        // Preserve an evaluator/recorder panic; discard incomplete scratch.
        // No user code runs under the lock, and poisoned locks are not unwrapped.
        if std::thread::panicking() {
            return;
        }
        if let Some(bin) = self.bin
            && let Ok(mut idle) = bin.try_lock()
            && idle.is_none()
        {
            *idle = Some(std::mem::take(&mut self.scratch));
        }
        // If a nested lease returned first, or the bin is busy, discard this
        // owned scratch instead of growing retention or waiting on other work.
    }
}

'''

TESTS = r'''

#[cfg(test)]
mod scratch_bank_tests {
    use super::*;
    use crate::schedule::Dcfr;
    use crate::storage::{F32Storage, I16Storage};
    use crate::tree::{ReachMap, TempNode, TreeSpec};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;

    #[test]
    fn reentrant_leases_are_disjoint_and_only_one_returns() {
        let bank = ScratchBank::with_bins(1);
        let mut outer = bank.checkout_for(Some(0));
        let mut a = outer.take(8);
        a.fill(11.0);
        let mut inner = bank.checkout_for(Some(0));
        let mut b = inner.take(8);
        assert_ne!(a.as_ptr(), b.as_ptr());
        b.fill(29.0);
        let retained = b.as_ptr();
        inner.put(b);
        drop(inner);
        assert_eq!(a, vec![11.0; 8]);
        outer.put(a);
        drop(outer);
        let mut next = bank.checkout_for(Some(0));
        let b = next.take(8);
        assert_eq!(b.as_ptr(), retained);
        assert!(b.iter().all(|x| x.to_bits() == 0));
    }

    #[test]
    fn reused_buffers_zero_empty_smaller_and_larger_dimensions() {
        let bank = ScratchBank::with_bins(1);
        let mut first = bank.checkout_for(Some(0));
        let mut buffer = first.take(32);
        buffer.fill(f32::NAN);
        let allocation = buffer.as_ptr();
        first.put(buffer);
        drop(first);
        for size in [0, 3, 32, 65, 1] {
            let mut lease = bank.checkout_for(Some(0));
            let mut buffer = lease.take(size);
            assert_eq!(buffer.len(), size);
            assert!(buffer.iter().all(|x| x.to_bits() == 0));
            if size <= 32 && size != 1 {
                assert_eq!(buffer.as_ptr(), allocation);
            }
            buffer.fill(-7.0);
            lease.put(buffer);
        }
    }

    #[test]
    fn contention_and_invalid_indices_use_uncached_owned_scratch() {
        let bank = ScratchBank::with_bins(1);
        let guard = bank.bins[0].lock().unwrap();
        // Holding the same mutex here would deadlock a blocking implementation.
        for index in [Some(0), Some(1), None] {
            let mut lease = bank.checkout_for(index);
            assert!(lease.bin.is_none());
            let buffer = lease.take(4);
            assert_eq!(buffer, vec![0.0; 4]);
            lease.put(buffer);
        }
        drop(guard);
        assert!(bank.bins[0].lock().unwrap().is_none());
    }

    #[test]
    fn panic_is_propagated_and_poison_does_not_cause_another_panic() {
        let bank = ScratchBank::with_bins(1);
        let failure = catch_unwind(AssertUnwindSafe(|| {
            let mut lease = bank.checkout_for(Some(0));
            lease.put(vec![9.0; 16]);
            panic!("original evaluator panic");
        }));
        assert_eq!(failure.unwrap_err().downcast_ref::<&str>(), Some(&"original evaluator panic"));
        assert!(bank.bins[0].lock().unwrap().is_none());
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = bank.bins[0].lock().unwrap();
            panic!("intentional test poison");
        }));
        let mut lease = bank.checkout_for(Some(0));
        assert!(lease.bin.is_none());
        let buffer = lease.take(2);
        lease.put(buffer);
        drop(lease);
    }

    #[test]
    fn invocation_drop_has_no_global_owner_of_cached_buffers() {
        let bank = Arc::new(ScratchBank::with_bins(2));
        let weak = Arc::downgrade(&bank);
        for index in 0..2 {
            let mut lease = bank.checkout_for(Some(index));
            lease.put(vec![1.0; 64]);
        }
        assert!(bank.bins.iter().all(|bin| bin.lock().unwrap().is_some()));
        assert_eq!(Arc::strong_count(&bank), 1);
        drop(bank);
        // ScratchBank has only owned Vec/Mutex/Option/Scratch fields, whose
        // normal field destruction releases the cached Vec allocations.
        assert!(weak.upgrade().is_none());
    }

    struct Payoff;

    impl TerminalEvaluator for Payoff {
        fn eval(&self, terminal: u32, p: Player, opp: &[f32], out: &mut [f32]) {
            let payoff = if p == Player::P0 { terminal as f32 } else { -(terminal as f32) };
            out.fill(opp.iter().sum::<f32>() * payoff);
        }
    }

    fn game() -> CompiledGame<Payoff> {
        let action = || TempNode::Action {
            player: Player::P0,
            children: vec![TempNode::Terminal { id: 1, tag: 0 }, TempNode::Terminal { id: 3, tag: 0 }],
            tag: 0,
        };
        let inner = || TempNode::Chance {
            deals: (0..4).map(|_| (0.25, PerPlayer::new(ReachMap::Identity, ReachMap::Identity), action())).collect(),
            tag: 0,
        };
        let tree = PublicTree::compile(TreeSpec {
            root: TempNode::Chance {
                deals: (0..4).map(|_| (0.25, PerPlayer::new(ReachMap::Identity, ReachMap::Identity), inner())).collect(),
                tag: 0,
            },
            masks: Vec::new(),
            transitions: Vec::new(),
            root_dims: PerPlayer::new(1, 1),
        });
        CompiledGame {
            tree, evaluator: Payoff, root_ranges: PerPlayer::new(vec![1.0], vec![1.0]),
            normalizer: 1.0, zero_sum: true,
        }
    }

    fn compare_run_and_step<S: Storage>() {
        let mut reference = None;
        for workers in [1, 2, 4] {
            rayon::ThreadPoolBuilder::new().num_threads(workers).build().unwrap().install(|| {
                let make = || {
                    let mut solver = Solver::<_, S>::new(game(), Box::<Dcfr>::default(), Some(3));
                    solver.set_par(ParConfig { chance_depth: 2, min_children: 2 });
                    solver
                };
                let mut run = make();
                assert_eq!(run.expected_value(Player::P0), 2.0);
                assert_eq!(run.best_response_value(Player::P0), 3.0);
                run.run(3);
                let mut steps = make();
                for _ in 0..3 { steps.step(); }
                assert_eq!(run.state(), steps.state());
                for p in Player::BOTH {
                    assert_eq!(run.expected_value(p).to_bits(), steps.expected_value(p).to_bits());
                    assert_eq!(run.best_response_value(p).to_bits(), steps.best_response_value(p).to_bits());
                    assert_eq!(run.expected_values_everywhere(p), steps.expected_values_everywhere(p));
                }
                let state = run.state();
                if let Some(reference) = &reference { assert_eq!(&state, reference); }
                else { reference = Some(state); }
            });
        }
    }

    #[test]
    fn invocation_lifetimes_preserve_nested_chance_f32() {
        compare_run_and_step::<F32Storage>();
    }

    #[test]
    fn invocation_lifetimes_preserve_nested_chance_i16() {
        compare_run_and_step::<I16Storage>();
    }
}
'''


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def transform(original: bytes) -> tuple[bytes, dict]:
    if sha(original) != SOURCE_SHA256:
        raise ValueError("Production solver source pin differs")
    text = original.decode()
    edits: list[tuple[str, str]] = []

    def replace(old: str, new: str, count: int = 1) -> None:
        nonlocal text
        if text.count(old) != count:
            raise ValueError(f"Unexpected transform occurrence count: {old[:100]!r}")
        text = text.replace(old, new)
        edits.append((new, old))

    replace("use std::sync::Mutex;", "use std::ops::{Deref, DerefMut};\nuse std::sync::Mutex;" + BANK)
    replace("self.step_with_plan(action_plan.as_ref());", "self.step_with_plan(action_plan.as_ref(), &bank);", 2)
    replace("        let action_plan = ActionPlan::new(&self.game.tree);\n", "        let action_plan = ActionPlan::new(&self.game.tree);\n        let bank = ScratchBank::new();\n", 3)
    replace("fn step_with_plan(&mut self, action_plan: Option<&ActionPlan>) {", "fn step_with_plan(&mut self, action_plan: Option<&ActionPlan>, bank: &ScratchBank) {")
    replace("                discounts: &discounts,\n                par: self.par,", "                discounts: &discounts,\n                par: self.par,\n                bank,")
    replace("    discounts: &'w Discounts,\n    par: ParConfig,", "    discounts: &'w Discounts,\n    par: ParConfig,\n    bank: &'w ScratchBank,")
    replace("/// pools and disjoint outputs; their task-local allocations are not retained\n/// across passes.", "/// leases and disjoint outputs. Idle scratch is retained by the invocation\n/// bank across its passes, with at most one idle Scratch per worker-index bin.")

    # The two CFR factories read the existing private pass context. The three
    # value factories use a private extra argument, keeping ValueCtx, ValuePass,
    # and McSolver's ev_pass/br_pass callers source-compatible.
    replace(".map_init(Scratch::new, |scratch, (pos, (child, mut view))| {", ".map_init(|| ctx.bank.checkout(), |scratch, (pos, (child, mut view))| {\n                        let scratch = scratch.deref_mut();")
    replace(".for_each_init(Scratch::new, |scratch, (a, (view, child_out))| {", ".for_each_init(|| ctx.bank.checkout(), |scratch, (a, (view, child_out))| {\n            let scratch = scratch.deref_mut();")
    replace(".map_init(Scratch::new, |scratch, (pos, child)| {", ".map_init(|| bank.checkout(), |scratch, (pos, child)| {\n                        let scratch = scratch.deref_mut();")
    replace(".for_each_init(Scratch::new, |scratch, (a, row)| {", ".for_each_init(|| bank.checkout(), |scratch, (a, row)| {\n                        let scratch = scratch.deref_mut();")
    replace("                    Scratch::new,\n                    |scratch, (a, child_out)| {", "                    || bank.checkout(),\n                    |scratch, (a, child_out)| {\n                        let scratch = scratch.deref_mut();")
    replace("fn value_pass<E: TerminalEvaluator, S: Storage, C, R>(\n    ctx: &ValueCtx<'_, E, S>,", "fn value_pass<E: TerminalEvaluator, S: Storage, C, R>(\n    ctx: &ValueCtx<'_, E, S>,\n    bank: &ScratchBank,")
    # Recursive calls are all in the body of value_pass; root callers are
    # handled separately to keep McSolver's signatures unchanged.
    begin, end = text.index("fn value_pass<"), text.index("/// The recorder [`value_pass`]")
    body = text[begin:end]
    matches = list(re.finditer(r"value_pass\(\n( +)ctx,\n", body))
    if len(matches) != 6:
        raise ValueError("Expected six recursive value calls")
    for indent in sorted({match[1] for match in matches}, key=len):
        old = f"value_pass(\n{indent}ctx,\n"
        replace(old, f"value_pass(\n{indent}ctx,\n{indent}bank,\n", body.count(old))
    replace("        value_pass(\n            &ctx,\n", "        value_pass(\n            &ctx,\n            &bank,\n")
    replace("    let action_plan = ActionPlan::new(ctx.tree);\n    value_pass(\n        ctx,\n", "    let action_plan = ActionPlan::new(ctx.tree);\n    let bank = ScratchBank::new();\n    value_pass(\n        ctx,\n        &bank,\n", 2)
    replace("\n#[cfg(test)]\nmod action_plan_tests {", TESTS + "\n#[cfg(test)]\nmod action_plan_tests {")
    restored = text
    for new, old in reversed(edits):
        restored = restored.replace(new, old)
    if restored.encode() != original:
        raise ValueError("Inverse transform did not reproduce original source")
    return text.encode(), {"inverse_before_rustfmt_byte_exact": True,
                           "production_source_sha256": SOURCE_SHA256,
                           "parallel_factory_replacements": 5,
                           "recursive_value_bank_arguments": 6,
                           "new_rust_tests": 7,
                           "flat_chance_changes": False,
                           "public_and_mccfr_value_signatures_unchanged": True}


def pin(name: str, data: bytes) -> dict:
    return {"path": name, "bytes": len(data), "sha256": sha(data)}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    original = (ROOT / SOURCE).read_bytes()
    raw, checks = transform(original)
    version = subprocess.run(["rustfmt", "--version"], capture_output=True, check=True, timeout=30)
    formatted = subprocess.run(["rustfmt", "--edition", "2024", "--emit", "stdout"],
                               input=raw, capture_output=True, check=True, timeout=30)
    candidate = formatted.stdout
    if (ROOT / SOURCE).read_bytes() != original:
        raise ValueError("Production source changed during generation")
    patch = "".join(difflib.unified_diff(original.decode().splitlines(keepends=True),
                                       candidate.decode().splitlines(keepends=True),
                                       fromfile="a/" + SOURCE, tofile="b/" + SOURCE)).encode()
    provenance = {"schema": "solvers.r1.worker-scratch-preparation/v1",
                  "source": pin(SOURCE, original), "preparer": pin("prepare.py", Path(__file__).read_bytes()),
                  "candidate": pin("solver.rs", candidate), "patch": pin("candidate.patch", patch),
                  "formatter": {"version": version.stdout.decode().strip(),
                                "argv": ["rustfmt", "--edition", "2024", "--emit", "stdout"],
                                "stderr": formatted.stderr.decode()},
                  "static_checks": checks, "production_adopted": False,
                  "rust_compile": None, "rust_tests_executed": False,
                  "quality_measurement": None, "performance_measurement": None}
    outputs = {"solver.rs": candidate, "candidate.patch": patch,
               "provenance.json": (json.dumps(provenance, indent=2) + "\n").encode()}
    for name, data in outputs.items():
        target = HERE / name
        if args.check:
            if target.read_bytes() != data:
                raise ValueError(f"Retained output differs: {name}")
        elif target.exists():
            raise FileExistsError(f"Refusing to overwrite {target}")
    if not args.check:
        for name, data in outputs.items():
            (HERE / name).write_bytes(data)
    print(json.dumps({"passed": True, "mode": "check" if args.check else "prepare",
                      "candidate": provenance["candidate"], "static_checks": checks}, indent=2))


if __name__ == "__main__":
    main()
