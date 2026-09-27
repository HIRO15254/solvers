"""Reversible adapter-only CFR chance-depth observation; never builds or solves."""
from pathlib import Path
import argparse
import difflib
import hashlib
import json

HERE = Path(__file__).resolve().parent
BASE = HERE.parents[1] / "cpu-occupancy/adapter/solve.rs"
BASE_SHA = "a6746b4316216231f3a4bf02120968d7b78ddb876d6d04cd611bf1efed7ba2a5"
FRONTIER = '''    // Structural counts, not task counts; collected outside all measured clocks.
    let mut frontier = [0_usize; 2];
    let mut frontier_children = [0_usize; 2];
    let mut stack = vec![(0_u32, 0_usize)];
    while let Some((id, ancestors)) = stack.pop() {
        let node = game.tree.node(id);
        let is_chance = node.kind == NodeKind::Chance;
        if is_chance && node.num_children >= 12 {
            for depth in 1..=2 {
                if ancestors < depth {
                    frontier[depth - 1] += 1;
                    frontier_children[depth - 1] += usize::from(node.num_children);
                }
            }
        }
        for child in game.tree.children(id) {
            stack.push((child, ancestors + usize::from(is_chance)));
        }
    }
    drop(stack);
    write_json(
        &out.join("grain.json"),
        &format!(
            "{{\\"schema\\":\\"r1.flop-chance-grain-observation/v1\\",\\"case\\":\\"{case}\\",\\"cfr_depth\\":{depth},\\"quality_depth\\":2,\\"min_children\\":12,\\"eligible_chance_nodes_by_depth\\":{frontier:?},\\"eligible_child_edges_by_depth\\":{frontier_children:?},\\"counts_scope\\":\\"one structural root traversal; not tasks, iterations or seat passes\\"}}"
        ),
    )?;
'''

def replacements():
    return [
        ('//! Usage: solve.exe narrow|expanded 1|2|4|8|16|32 1..128 NEW_OUTPUT_DIRECTORY\n//! Arguments after the case are worker count and exact iteration count.',
         '//! Usage: solve narrow|expanded 1|16|32 2|16 1|2 NEW_OUTPUT_DIRECTORY\n//! Arguments after the case are worker count, exact iterations and CFR chance depth.'),
        ('    let mut value = ProcessCpuTimespec { tv_sec: 0, tv_nsec: 0 };', '    let mut value = ProcessCpuTimespec {\n        tv_sec: 0,\n        tv_nsec: 0,\n    };'),
        ('    assert_eq!(result, 0, "clock_gettime failed: {}", std::io::Error::last_os_error());',
         '    assert_eq!(\n        result,\n        0,\n        "clock_gettime failed: {}",\n        std::io::Error::last_os_error()\n    );'),
        ('    let mut fields = status.lines().filter_map(|line| line.strip_prefix("Cpus_allowed_list:"));',
         '    let mut fields = status\n        .lines()\n        .filter_map(|line| line.strip_prefix("Cpus_allowed_list:"));'),
        ('    if value.is_empty() || fields.next().is_some()\n        || !value.bytes().all(|b| b.is_ascii_digit() || b == b\',\' || b == b\'-\')',
         '    if value.is_empty()\n        || fields.next().is_some()\n        || !value\n            .bytes()\n            .all(|b| b.is_ascii_digit() || b == b\',\' || b == b\'-\')'),
        ('        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid Cpus_allowed_list"));',
         '        return Err(std::io::Error::new(\n            std::io::ErrorKind::InvalidData,\n            "invalid Cpus_allowed_list",\n        ));'),
        ('    assert_eq!(cpu_allowed_list, allowed_cpu_list()?, "CPU affinity changed");',
         '    assert_eq!(\n        cpu_allowed_list,\n        allowed_cpu_list()?,\n        "CPU affinity changed"\n    );'),
        ('4,\n        "usage: solve narrow|expanded 1|2|4|8|16|32 1..128 NEW_OUTPUT_DIRECTORY"',
         '5,\n        "usage: solve narrow|expanded 1|16|32 2|16 1|2 NEW_OUTPUT_DIRECTORY"'),
        ('    let (config, support, elements, normalizer) = fixture(case);\n    let out = Path::new(&args[3]);',
         '    let depth: u32 = args[3].parse()?;\n    assert!([1, 2].contains(&depth), "CFR chance depth");\n    assert!([1, 16, 32].contains(&threads), "workers");\n    assert!([2, 16].contains(&iterations), "fixed iterations");\n    let (config, support, elements, normalizer) = fixture(case);\n    let out = Path::new(&args[4]);'),
        ('\\"chance_depth\\":2,\\"min_children\\":12', '\\"chance_depth\\":{depth},\\"quality_chance_depth\\":2,\\"min_children\\":12'),
        ('    event("build", "completed");', FRONTIER + '    event("build", "completed");'),
        ('    solver.set_par(ParConfig {\n        chance_depth: 2,', '    solver.set_par(ParConfig {\n        chance_depth: depth,'),
        ('    // Public API calls, including their precise zero-sum exploitability policy.',
         '    // Only CFR depth varies; all quality traversals retain the original setting.\n    solver.set_par(ParConfig {\n        chance_depth: 2,\n        min_children: 12,\n    });\n    // Public API calls, including their precise zero-sum exploitability policy.'),
    ]

def derive(source):
    for old, new in replacements():
        if source.count(old) != 1:
            raise ValueError("adapter anchor differs")
        source = source.replace(old, new)
    return source

def inverse(source):
    for old, new in reversed(replacements()):
        if source.count(new) != 1:
            raise ValueError("inverse adapter anchor differs")
        source = source.replace(new, old)
    return source

def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    original = BASE.read_bytes()
    if pin(original)["sha256"] != BASE_SHA:
        raise ValueError("original CPU adapter differs")
    source = derive(original.decode("utf-8"))
    assert inverse(source).encode() == original
    path = HERE / "solve.rs"
    if args.check:
        assert path.read_bytes() == source.encode()
    else:
        path.write_text(source, encoding="utf-8", newline="\n")
        (HERE / "adapter.patch").write_text("".join(difflib.unified_diff(original.decode().splitlines(True), source.splitlines(True), "cpu-occupancy/adapter/solve.rs", "chance-grain/adapter/solve.rs")), encoding="utf-8", newline="\n")
        (HERE / "provenance.json").write_text(json.dumps({"schema": "r1.chance-grain-adapter/v1", "original": pin(original), "generated": pin(source.encode()), "inverse_exact": True, "native_execution": False}, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": "passed", "source": pin(source.encode()), "inverse_exact": True}))

if __name__ == "__main__":
    main()
