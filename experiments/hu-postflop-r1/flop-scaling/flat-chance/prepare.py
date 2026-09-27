"""Generate a pinned research-only flat chance-output solver source."""
from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import subprocess
from pathlib import Path

SOURCE = "crates/engine/src/solver.rs"
BASE_SHA256 = "e116e2554915768e0ccef89e86dccb0ffb89d0a2ff309343c0992fc8d7b02e4b"
HELPER = '''/// Borrow disjoint variable-length chance outputs, including empty rows.
fn chance_output_rows<'a>(values: &'a mut [f32], dims: &[usize]) -> Vec<&'a mut [f32]> {
    let mut rest = values;
    let mut rows = Vec::with_capacity(dims.len());
    for &dim in dims {
        let (row, tail) = rest.split_at_mut(dim);
        rows.push(row);
        rest = tail;
    }
    assert!(rest.is_empty(), "chance output dimensions do not cover buffer");
    rows
}

'''


def transform(original: bytes) -> bytes:
    if hashlib.sha256(original).hexdigest() != BASE_SHA256:
        raise ValueError("Original full source pin differs")
    text = original.decode("utf-8")
    marker = "/// CFR update pass for player `p`, writing p's counterfactual values"
    if text.count(marker) != 1:
        raise ValueError("CFR source anchor differs")
    text = text.replace(marker, HELPER + marker, 1)
    for is_cfr in (True, False):
        function = "fn cfr_pass<" if is_cfr else "fn value_pass<"
        start = text.index("                let results: Vec<Vec<f32>> = child_ids", text.index(function))
        end = text.index("            } else {", start)
        old = text[start:end]
        if old.count(".collect();") != 1 or old.count(".map_init(Scratch::new") != 1:
            raise ValueError("Expected one ordered chance-output collection")
        parent = "my_reach.len() as u32" if is_cfr else "parent_dim"
        before = f'''                let output_dims: Vec<usize> = (0..node.num_children as usize)
                    .map(|pos| {{
                        let deal = *ctx.tree.deal(&node, pos);
                        ctx.tree.mapped_dim(deal.maps[ctx.p], {parent}) as usize
                    }})
                    .collect();
                let output_len = output_dims
                    .iter()
                    .try_fold(0usize, |sum, &dim| sum.checked_add(dim))
                    .expect("chance output dimensions overflow");
                // Zero once before fan-out; the experiment must measure this
                // serial first-touch as well as allocation effects.
                let mut outputs = scratch.take(output_len);
'''
        new = old.replace("                let results: Vec<Vec<f32>> = child_ids", "                child_ids", 1)
        new = new.replace("                    .enumerate()", "                    .zip(chance_output_rows(&mut outputs, &output_dims))\n                    .enumerate()", 1)
        if is_cfr:
            new = new.replace(".map_init(Scratch::new, |scratch, (pos, (child, mut view))| {",
                              ".for_each_init(Scratch::new, |scratch, (pos, ((child, mut view), child_out))| {")
        else:
            new = new.replace(".map_init(Scratch::new, |scratch, (pos, child)| {",
                              ".for_each_init(Scratch::new, |scratch, (pos, (child, child_out))| {")
        new = new.replace("                        let mut child_out = scratch.take(my_dim);", "                        debug_assert_eq!(child_out.len(), my_dim);", 1)
        new = new.replace("                            &mut child_out,", "                            child_out,", 1)
        new = new.replace("                        child_out\n                    })\n                    .collect();",
                          "                    });", 1)
        new = new.replace("                for (pos, child_out) in results.into_iter().enumerate() {",
                          "                let mut offset = 0;\n                for (pos, &dim) in output_dims.iter().enumerate() {\n                    let child_out = &outputs[offset..offset + dim];\n                    offset += dim;", 1)
        new = new.replace(".accumulate_values(deal.maps[ctx.p], deal.weight, &child_out, out);",
                          ".accumulate_values(deal.maps[ctx.p], deal.weight, child_out, out);", 1)
        if "results" in new or ".map_init" in new or "&mut child_out" in new:
            raise ValueError("Incomplete chance-output transformation")
        text = text[:start] + before + new + "                scratch.put(outputs);\n" + text[end:]
    text = text.replace("// Indexed (ordered) collect, then in-child-order fold below:",
                        "// Disjoint output rows, then in-child-order fold below:", 1)
    text = text.replace("// the same reference. Ordered collect + in-order fold below",
                        "// the same reference. Disjoint rows + in-order fold below", 1)
    # These existing algorithmic boundaries must survive verbatim.
    for anchor in ("let child_budget = par_budget.saturating_sub(1);",
                   "if par_budget > 0 && node.num_children as usize >= ctx.par.min_children {"):
        if text.count(anchor) != original.decode().count(anchor):
            raise ValueError("Changed scheduling controls")
    return text.encode("utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    root, out = args.source_root.resolve(strict=True), args.out.resolve()
    if out.is_relative_to(root / "crates") or out.is_relative_to(root / "target"):
        raise ValueError("Research source belongs outside crates/ and target/")
    original = (root / SOURCE).read_bytes()
    unformatted = transform(original)
    formatter = subprocess.run(["rustfmt", "--version"], capture_output=True,
                               check=True, timeout=30).stdout.decode().strip()
    changed = subprocess.run(["rustfmt", "--edition", "2024", "--emit", "stdout"],
                             input=unformatted, capture_output=True,
                             check=True, timeout=30).stdout
    out.mkdir(parents=True, exist_ok=False)
    (out / "solver.rs").write_bytes(changed)
    patch = "".join(difflib.unified_diff(original.decode().splitlines(keepends=True),
        changed.decode().splitlines(keepends=True), fromfile="a/"+SOURCE,
        tofile="b/"+SOURCE)).encode()
    (out / "candidate.patch").write_bytes(patch)
    (out / "manifest.json").write_text(json.dumps({
        "schema":"solvers.r1.flat-chance-preparation/v1",
        "source_path":SOURCE,"source_sha256":BASE_SHA256,
        "formatter":formatter,
        "unformatted_candidate_sha256":hashlib.sha256(unformatted).hexdigest(),
        "candidate_sha256":hashlib.sha256(changed).hexdigest(),
        "candidate_bytes":len(changed),
        "patch_sha256":hashlib.sha256(patch).hexdigest(),
        "scope":"CFR and value chance outputs only; scheduling and ordered accumulation preserved",
        "rust_compile":None,"quality":None,"performance":None,"production_adopted":False,
    },indent=2)+"\n",encoding="utf-8")


if __name__ == "__main__":
    main()
