"""Prepare reversible F32 CFR update research sources; never build or solve."""
from __future__ import annotations

import argparse
import difflib
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
BASE_PINS = {
    "solver.rs": "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a",
    "storage.rs": "1929ce7947c6b60d9aea8d75540065c8c9a0596a3f59d82bbf8f4b6f687fd1a5",
    "lib.rs": "4d3f7a3b7551fbf1547dd45e0fb5170ebfdd45fd83a7ebefe132562d5cf885d8",
}

UPDATE_TYPE = '''/// Inputs for one acting-player CFR update, all in this node's hand space.
/// `action_values` and `strategy` are action-major A*H slices; `node_values`
/// and `reach` contain H entries. They must not alias the mutable storage.
/// The contents of `action_values` after the call are unspecified scratch.
/// This groups borrows only: it allocates nothing and owns no solver state.
pub struct CfrUpdate<'a> {
    pub action_values: &'a mut [f32],
    pub node_values: &'a [f32],
    pub reach: &'a [f32],
    pub strategy: &'a [f32],
}

'''

DEFAULT_METHOD = '''    /// Applies the acting-player regret and average-strategy updates.
    /// The default keeps the historical scratch transforms and backend-call
    /// order, including both complete I16 block quantizations. F32 backends
    /// may fuse each producer with its storage update without changing any
    /// per-element arithmetic. `action_values` is scratch after this call.
    fn fused_update(&mut self, r: StorageRef, ref_idx: u32, update: CfrUpdate<'_>, d: &Discounts) {
        let CfrUpdate {
            action_values,
            node_values,
            reach,
            strategy,
        } = update;
        let (num_actions, num_hands) = (r.num_actions as usize, r.num_hands as usize);
        debug_assert_eq!(action_values.len(), r.len());
        debug_assert_eq!(strategy.len(), r.len());
        debug_assert_eq!(node_values.len(), num_hands);
        debug_assert_eq!(reach.len(), num_hands);
        for a in 0..num_actions {
            for h in 0..num_hands {
                action_values[a * num_hands + h] -= node_values[h];
            }
        }
        self.update_regrets(r, ref_idx, action_values, d);
        for a in 0..num_actions {
            for h in 0..num_hands {
                action_values[a * num_hands + h] = reach[h] * strategy[a * num_hands + h];
            }
        }
        self.accumulate_strategy(r, ref_idx, action_values, d);
    }

'''

F32_HELPER = '''// Fuse each producer with its F32 storage update, avoiding two scratch
// write/read round trips. Keep all regret updates before all strategy updates.
// These are the same f32 operations as the separate paths: no mul_add,
// reassociation, reciprocal, zero-reach shortcut or changed discount cast.
fn fused_update_f32_impl(
    regrets: &mut [f32],
    strategy_sum: &mut [f32],
    offset: usize,
    r: StorageRef,
    update: CfrUpdate<'_>,
    d: &Discounts,
) {
    let CfrUpdate {
        action_values,
        node_values,
        reach,
        strategy,
    } = update;
    let (num_actions, num_hands) = (r.num_actions as usize, r.num_hands as usize);
    debug_assert_eq!(action_values.len(), r.len());
    debug_assert_eq!(strategy.len(), r.len());
    debug_assert_eq!(node_values.len(), num_hands);
    debug_assert_eq!(reach.len(), num_hands);
    let regrets = &mut regrets[offset..offset + r.len()];
    let (pos, neg) = (d.pos as f32, d.neg as f32);
    for a in 0..num_actions {
        for h in 0..num_hands {
            let i = a * num_hands + h;
            let delta: f32 = action_values[i] - node_values[h];
            let regret = &mut regrets[i];
            let factor = if *regret > 0.0 { pos } else { neg };
            let mut updated = *regret * factor + delta;
            if d.floor_neg && updated < 0.0 {
                updated = 0.0;
            }
            *regret = updated;
        }
    }
    let strategy_sum = &mut strategy_sum[offset..offset + r.len()];
    let avg = d.avg as f32;
    if d.reset_avg {
        for a in 0..num_actions {
            for h in 0..num_hands {
                let i = a * num_hands + h;
                let weighted: f32 = reach[h] * strategy[i];
                strategy_sum[i] = weighted;
            }
        }
    } else {
        for a in 0..num_actions {
            for h in 0..num_hands {
                let i = a * num_hands + h;
                let weighted: f32 = reach[h] * strategy[i];
                let sum = &mut strategy_sum[i];
                *sum = *sum * avg + weighted;
            }
        }
    }
}

'''

F32_OWNED_METHOD = '''    fn fused_update(&mut self, r: StorageRef, _ref_idx: u32, update: CfrUpdate<'_>, d: &Discounts) {
        fused_update_f32_impl(
            &mut self.regrets,
            &mut self.strategy_sum,
            r.offset,
            r,
            update,
            d,
        );
    }

'''

F32_VIEW_METHOD = '''    fn fused_update(&mut self, r: StorageRef, _ref_idx: u32, update: CfrUpdate<'_>, d: &Discounts) {
        let local = self.local_offset(r);
        fused_update_f32_impl(
            &mut *self.regrets,
            &mut *self.strategy_sum,
            local,
            r,
            update,
            d,
        );
    }

'''

OLD_CALL = '''            // Instantaneous regret, computed in place.
            for a in 0..num_actions {
                for h in 0..num_hands {
                    cfvs[a * num_hands + h] -= node_cfv[h];
                }
            }
            views
                .own()
                .update_regrets(sref, sref.index, &cfvs, ctx.discounts);

            // Overwrite `cfvs` again as the reach-weighted strategy buffer.
            for a in 0..num_actions {
                for h in 0..num_hands {
                    cfvs[a * num_hands + h] = my_reach[h] * sigma[a * num_hands + h];
                }
            }
            views
                .own()
                .accumulate_strategy(sref, sref.index, &cfvs, ctx.discounts);
'''

NEW_CALL = '''            // F32 consumes the same inputs directly; other backends retain
            // the historical scratch transforms through the default method.
            views.own().fused_update(
                sref,
                sref.index,
                CfrUpdate {
                    action_values: &mut cfvs,
                    node_values: &node_cfv,
                    reach: my_reach,
                    strategy: &sigma,
                },
                ctx.discounts,
            );
'''


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def replacements(name):
    if name == "solver.rs":
        old = "    StateMismatch, Storage, StorageRef, StorageSpan, StorageState, StorageStateRef, StorageView,"
        new = "    CfrUpdate, StateMismatch, Storage, StorageRef, StorageSpan, StorageState, StorageStateRef,\n    StorageView,"
        return [(old, new), (OLD_CALL, NEW_CALL)]
    if name == "storage.rs":
        trait_anchor = "/// The four element-level operations shared by a full [`Storage`] backend"
        method_anchor = "    /// Writes the normalized average strategy into `out`. Hands never"
        helper_anchor = "fn average_strategy_impl(strategy_sum: &[f32], offset: usize, r: StorageRef, out: &mut [f32]) {"
        owned_anchor = "    fn average_strategy(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {\n        average_strategy_impl(&self.strategy_sum, r.offset, r, out);"
        view_anchor = "    fn average_strategy(&self, r: StorageRef, _ref_idx: u32, out: &mut [f32]) {\n        let local = self.local_offset(r);\n        average_strategy_impl(&*self.strategy_sum, local, r, out);"
        return [(trait_anchor, UPDATE_TYPE + trait_anchor),
                (method_anchor, DEFAULT_METHOD + method_anchor),
                (helper_anchor, F32_HELPER + helper_anchor),
                (owned_anchor, F32_OWNED_METHOD + owned_anchor),
                (view_anchor, F32_VIEW_METHOD + view_anchor)]
    if name == "lib.rs":
        old = "    F32Storage, F32View, I16Storage, I16View, StateMismatch, Storage, StorageOps, StorageRef,\n    StorageSpan, StorageState, StorageStateRef, StorageView,"
        new = "    CfrUpdate, F32Storage, F32View, I16Storage, I16View, StateMismatch, Storage, StorageOps,\n    StorageRef, StorageSpan, StorageState, StorageStateRef, StorageView,"
        return [(old, new)]
    raise ValueError("Unknown source: " + name)


def transform(name, original):
    if pin(original)["sha256"] != BASE_PINS[name]:
        raise ValueError("Baseline source differs: " + name)
    value = original.decode("utf-8")
    for old, new in replacements(name):
        if value.count(old) != 1:
            raise ValueError("Replacement anchor is not unique: " + name)
        value = value.replace(old, new, 1)
    generated = value.encode("utf-8")
    if restore(name, generated) != original:
        raise ValueError("Unexpected edits: " + name)
    return generated


def restore(name, generated):
    value = generated.decode("utf-8")
    for old, new in reversed(replacements(name)):
        if value.count(new) != 1:
            raise ValueError("Inverse anchor is not unique: " + name)
        value = value.replace(new, old, 1)
    result = value.encode("utf-8")
    if pin(result)["sha256"] != BASE_PINS[name]:
        raise ValueError("Restored baseline differs: " + name)
    return result


def expected_outputs():
    output, originals, patches = {}, {}, []
    for name in BASE_PINS:
        relative = "crates/engine/src/" + name
        original = (ROOT / relative).read_bytes()
        candidate = transform(name, original)
        if name == "storage.rs":
            marker = b"// --- I16Storage: quantized backend"
            if original[original.index(marker):] != candidate[candidate.index(marker):]:
                raise ValueError("I16 implementation or storage tests changed")
        originals[relative] = pin(original)
        output["baseline/" + name] = original
        output["candidate/" + name] = candidate
        patches.append("".join(difflib.unified_diff(original.decode().splitlines(True), candidate.decode().splitlines(True),
                                                  fromfile="a/" + relative, tofile="b/" + relative)))
    output["candidate.patch"] = "".join(patches).encode("utf-8")
    context = ["Cargo.lock", "Cargo.toml", "crates/engine/Cargo.toml", "crates/engine/src/schedule.rs",
               "crates/engine/src/scratch.rs", "crates/engine/src/mccfr.rs"]
    context += [p.relative_to(ROOT).as_posix() for p in sorted((ROOT / "crates/cfr-ref").rglob("*"))
                if p.is_file() and p.suffix in (".rs", ".toml")]
    receipt = {"schema": "r1.fused-cfr-update-source/v1", "baseline": originals,
               "unchanged_context": {name: pin((ROOT / name).read_bytes()) for name in context},
               "generator": pin(Path(__file__).read_bytes()),
               "preparation_controls": {name: pin((HERE / name).read_bytes())
                                        for name in ("test_prepare.py", "protocol.jp.md", "tests/fused_update.rs")},
               "native_test_deployment": {"tests/fused_update.rs": "crates/engine/tests/fused_update.rs"},
               "generated": {name: pin(data) for name, data in output.items()},
               "inverse_byte_identical": True, "i16_implementation_byte_unchanged": True,
               "native_compiled": False, "native_executed": False, "performance_claim": False,
               "scope": "Research source preparation only; production and independent oracle untouched",
               "license": "Repository-original derivation, MIT OR Apache-2.0; no external implementation used"}
    output["provenance.json"] = (json.dumps(receipt, indent=2) + "\n").encode()
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    output = expected_outputs()
    for name, data in output.items():
        path = HERE / name
        if args.check and (not path.is_file() or path.read_bytes() != data):
            raise ValueError("Saved evidence differs: " + name)
        if path.exists() and path.read_bytes() != data:
            raise ValueError("Refusing to replace different evidence: " + name)
    if not args.check:
        for name, data in output.items():
            path = HERE / name
            path.parent.mkdir(parents=True, exist_ok=True)
            if not path.exists():
                with path.open("xb") as stream:
                    stream.write(data)
    print(json.dumps({"status": "checked" if args.check else "prepared", "files": {name: pin(data) for name, data in output.items()}}))


if __name__ == "__main__":
    main()
