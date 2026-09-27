"""Reversible, source-only CLOCK_MONOTONIC markers for the fixed CPU probe."""
from pathlib import Path
import argparse
import difflib
import hashlib
import json

HERE = Path(__file__).resolve().parent
BASE = HERE.parents[1] / "cpu-occupancy/adapter/solve.rs"
BASE_SHA = "a6746b4316216231f3a4bf02120968d7b78ddb876d6d04cd611bf1efed7ba2a5"


def replacements():
    # The first six edits only reproduce rustfmt's formatting of the old probe.
    edits = [
        ('    let mut value = ProcessCpuTimespec { tv_sec: 0, tv_nsec: 0 };', '    let mut value = ProcessCpuTimespec {\n        tv_sec: 0,\n        tv_nsec: 0,\n    };'),
        ('    assert_eq!(result, 0, "clock_gettime failed: {}", std::io::Error::last_os_error());', '    assert_eq!(\n        result,\n        0,\n        "clock_gettime failed: {}",\n        std::io::Error::last_os_error()\n    );'),
        ('    let mut fields = status.lines().filter_map(|line| line.strip_prefix("Cpus_allowed_list:"));', '    let mut fields = status\n        .lines()\n        .filter_map(|line| line.strip_prefix("Cpus_allowed_list:"));'),
        ('    if value.is_empty() || fields.next().is_some()\n        || !value.bytes().all(|b| b.is_ascii_digit() || b == b\',\' || b == b\'-\')', '    if value.is_empty()\n        || fields.next().is_some()\n        || !value\n            .bytes()\n            .all(|b| b.is_ascii_digit() || b == b\',\' || b == b\'-\')'),
        ('        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid Cpus_allowed_list"));', '        return Err(std::io::Error::new(\n            std::io::ErrorKind::InvalidData,\n            "invalid Cpus_allowed_list",\n        ));'),
        ('    assert_eq!(cpu_allowed_list, allowed_cpu_list()?, "CPU affinity changed");', '    assert_eq!(\n        cpu_allowed_list,\n        allowed_cpu_list()?,\n        "CPU affinity changed"\n    );'),
        ('//! Usage: solve.exe narrow|expanded 1|2|4|8|16|32 1..128 NEW_OUTPUT_DIRECTORY', '//! Usage: flop_cpu_profile_probe narrow|expanded 1|16|32 64 NEW_OUTPUT_DIRECTORY'),
        ('usage: solve narrow|expanded 1|2|4|8|16|32 1..128 NEW_OUTPUT_DIRECTORY', 'usage: flop_cpu_profile_probe narrow|expanded 1|16|32 64 NEW_OUTPUT_DIRECTORY'),
        ('    assert!([1, 2, 4, 8, 16, 32].contains(&threads), "workers");\n    assert!((1..=128).contains(&iterations), "iteration limit is 128");', '    assert!([1, 16, 32].contains(&threads), "workers");\n    assert_eq!(iterations, 64, "the predeclared profile uses exactly N64");'),
        ('fn fixture(case: &str)', (HERE / "markers.rs.inc").read_text(encoding="utf-8") + 'fn fixture(case: &str)'),
        ('    pool.install(|| {\n        assert_eq!(rayon::current_num_threads(), threads);', '    let cfr_start_ns = monotonic_ns();\n    pool.install(|| {\n        assert_eq!(rayon::current_num_threads(), threads);'),
        ('        solver.run(iterations);\n    });', '        solver.run(iterations);\n    });\n    let cfr_end_ns = monotonic_ns();'),
        ('    let state_bytes = write_state(&out.join("state.bin"), &solver, iterations)?;', '    let state_start_ns = monotonic_ns();\n    let state_bytes = write_state(&out.join("state.bin"), &solver, iterations)?;\n    let state_end_ns = monotonic_ns();'),
        ('    let quality_cpu_started = process_cpu_ns();', '    let quality_start_ns = monotonic_ns();\n    let quality_cpu_started = process_cpu_ns();'),
        ('    let mut ev = [0.0_f64; 2];', '    let mut ev_ns = [[0_u64; 2]; 2];\n    let mut br_ns = [[0_u64; 2]; 2];\n    let mut ev = [0.0_f64; 2];'),
        ('        ev[p.index()] = pool.install(|| solver.expected_value(p));', '        ev_ns[p.index()][0] = monotonic_ns();\n        ev[p.index()] = pool.install(|| solver.expected_value(p));\n        ev_ns[p.index()][1] = monotonic_ns();'),
        ('        br[p.index()] = pool.install(|| solver.best_response_value(p));', '        br_ns[p.index()][0] = monotonic_ns();\n        br[p.index()] = pool.install(|| solver.best_response_value(p));\n        br_ns[p.index()][1] = monotonic_ns();'),
        ('    let gains = pool.install(|| solver.exploitability()).0;', '    let gains_start_ns = monotonic_ns();\n    let gains = pool.install(|| solver.exploitability()).0;\n    let gains_end_ns = monotonic_ns();'),
        ('    let quality_cpu_seconds = process_cpu_elapsed(quality_cpu_started);', '    let quality_cpu_seconds = process_cpu_elapsed(quality_cpu_started);\n    let quality_end_ns = monotonic_ns();'),
        ('    event("probe", "completed");', '''    // Serialize only after all computational intervals; no marker I/O inside CFR.
    write_phases(
        &out.join("phases.json"),
        case,
        threads,
        iterations,
        [
            [cfr_start_ns, cfr_end_ns],
            [state_start_ns, state_end_ns],
            [quality_start_ns, quality_end_ns],
            ev_ns[0],
            ev_ns[1],
            br_ns[0],
            br_ns[1],
            [gains_start_ns, gains_end_ns],
        ],
    )?;
    event("probe", "completed");'''),
    ]
    return edits


def transform(source, reverse=False):
    edits = replacements()
    for old, new in (reversed(edits) if reverse else edits):
        left, right = (new, old) if reverse else (old, new)
        if source.count(left) != 1:
            raise ValueError("source anchor differs: " + left[:80])
        source = source.replace(left, right)
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
    source = transform(original.decode("utf-8"))
    assert transform(source, reverse=True).encode() == original
    patch = "".join(difflib.unified_diff(original.decode().splitlines(True), source.splitlines(True), "cpu-occupancy/adapter/solve.rs", "cpu-profile/adapter/solve.rs"))
    provenance = {
        "schema": "r1.cpu-profile-adapter-source/v1",
        "original_path": "experiments/hu-postflop-r1/flop-scaling/cpu-occupancy/adapter/solve.rs",
        "original": pin(original), "generated": pin(source.encode()),
        "generator": pin(Path(__file__).read_bytes()),
        "marker_helper": pin((HERE / "markers.rs.inc").read_bytes()),
        "patch": pin(patch.encode()), "inverse_exact": True,
        "production_source_modified": False, "native_execution": False,
        "fixed_iterations": 64, "workers": [1, 16, 32],
        "example": "flop_cpu_profile_probe",
    }
    outputs = {"solve.rs": source, "adapter.patch": patch,
               "provenance.json": json.dumps(provenance, indent=2) + "\n"}
    for name, data in outputs.items():
        path = HERE / name
        if args.check:
            assert path.read_bytes() == data.encode(), name
        else:
            path.write_text(data, encoding="utf-8", newline="\n")
    print(json.dumps({"status": "passed", "source": pin(source.encode()), "inverse_exact": True, "native_execution": False}))


if __name__ == "__main__":
    main()
