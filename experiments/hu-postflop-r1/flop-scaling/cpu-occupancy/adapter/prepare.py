"""Generate the baseline CPU-occupancy adapter reversibly; no native tools."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
SOURCE = HERE.parents[1] / "flat-ev/cloud32/solve.rs"
CLOCK_SOURCE = SOURCE.parent / "diagnostic/cpu_clock.rs.inc"
BASELINE = HERE.parents[4] / "crates/engine/src/solver.rs"
SOURCE_SHA = "63cba37a5b0f79dfed2ca0ddf879daada3f0fee094ee5f287f86bc1f37414d46"
CLOCK_SHA = "5d58c11e1b31c1f0e0d51d8a49aa96d27ff224e77feb746ddae16f8094ac1ceb"
BASELINE_SHA = "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a"
SCHEMA = "r1.flop-cpu-occupancy/v1"

AFFINITY_HELPER = '''fn allowed_cpu_list() -> std::io::Result<String> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    let mut fields = status.lines().filter_map(|line| line.strip_prefix("Cpus_allowed_list:"));
    let value = fields.next().unwrap_or("").trim();
    if value.is_empty() || fields.next().is_some()
        || !value.bytes().all(|b| b.is_ascii_digit() || b == b',' || b == b'-')
    {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid Cpus_allowed_list"));
    }
    Ok(value.to_owned())
}
'''


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def replacements(helper):
    cpu_output = r'''    assert_eq!(cpu_allowed_list, allowed_cpu_list()?, "CPU affinity changed");
    write_json(
        &out.join("cpu.json"),
        &format!(
            "{{\"schema\":\"r1.flop-cpu-occupancy/v1\",\"case\":\"{case}\",\"threads\":{threads},\"iterations\":{iterations},\"clock\":\"CLOCK_PROCESS_CPUTIME_ID\",\"clock_id\":2,\"scope\":\"all threads in this process\",\"cpu_allowed_list\":\"{cpu_allowed_list}\",\"cfr_cpu_seconds\":{cfr_cpu_seconds},\"quality_cpu_seconds\":{quality_cpu_seconds},\"ev_cpu_seconds\":{ev_cpu_seconds:?},\"br_cpu_seconds\":{br_cpu_seconds:?},\"exploitability_cpu_seconds\":{exploitability_cpu_seconds},\"cfr_wall_seconds\":{cfr_seconds},\"quality_wall_seconds\":{quality_seconds},\"ev_wall_seconds\":{ev_wall_seconds:?},\"br_wall_seconds\":{br_wall_seconds:?},\"exploitability_wall_seconds\":{exploitability_wall_seconds},\"performance_claim\":false}}"
        ),
    )?;
'''
    return [
        ("\nfn event(", "\n" + helper + "\n" + AFFINITY_HELPER + "\nfn event("),
        ('    event("cfr", "started");\n    let started = Instant::now();',
         '    let cpu_allowed_list = allowed_cpu_list()?;\n    event("cfr", "started");\n'
         '    let cfr_cpu_started = process_cpu_ns();\n    let started = Instant::now();'),
        ('    let cfr_seconds = started.elapsed().as_secs_f64();',
         '    let cfr_seconds = started.elapsed().as_secs_f64();\n    let cfr_cpu_seconds = process_cpu_elapsed(cfr_cpu_started);'),
        ('    let started = Instant::now();\n    let mut ev = [0.0_f64; 2];',
         '    let quality_cpu_started = process_cpu_ns();\n    let started = Instant::now();\n'
         '    let mut ev_cpu_seconds = [0.0_f64; 2];\n    let mut br_cpu_seconds = [0.0_f64; 2];\n'
         '    let mut ev_wall_seconds = [0.0_f64; 2];\n    let mut br_wall_seconds = [0.0_f64; 2];\n'
         '    let mut ev = [0.0_f64; 2];'),
        ('        ev[p.index()] = pool.install(|| solver.expected_value(p));',
         '        let phase_cpu_started = process_cpu_ns();\n        let phase_wall_started = Instant::now();\n'
         '        ev[p.index()] = pool.install(|| solver.expected_value(p));\n'
         '        ev_wall_seconds[p.index()] = phase_wall_started.elapsed().as_secs_f64();\n'
         '        ev_cpu_seconds[p.index()] = process_cpu_elapsed(phase_cpu_started);'),
        ('        br[p.index()] = pool.install(|| solver.best_response_value(p));',
         '        let phase_cpu_started = process_cpu_ns();\n        let phase_wall_started = Instant::now();\n'
         '        br[p.index()] = pool.install(|| solver.best_response_value(p));\n'
         '        br_wall_seconds[p.index()] = phase_wall_started.elapsed().as_secs_f64();\n'
         '        br_cpu_seconds[p.index()] = process_cpu_elapsed(phase_cpu_started);'),
        ('    let gains = pool.install(|| solver.exploitability()).0;',
         '    let exploitability_cpu_started = process_cpu_ns();\n    let exploitability_wall_started = Instant::now();\n'
         '    let gains = pool.install(|| solver.exploitability()).0;\n'
         '    let exploitability_wall_seconds = exploitability_wall_started.elapsed().as_secs_f64();\n'
         '    let exploitability_cpu_seconds = process_cpu_elapsed(exploitability_cpu_started);'),
        ('    let quality_seconds = started.elapsed().as_secs_f64();',
         '    let quality_seconds = started.elapsed().as_secs_f64();\n'
         '    let quality_cpu_seconds = process_cpu_elapsed(quality_cpu_started);'),
        ('    event("probe", "completed");', cpu_output + '    event("probe", "completed");'),
    ]


def transform(original, helper):
    if pin(original)["sha256"] != SOURCE_SHA or pin(helper.encode())["sha256"] != CLOCK_SHA:
        raise ValueError("Frozen adapter or CPU clock helper differs")
    value = original.decode("utf-8")
    for old, new in replacements(helper):
        if value.count(old) != 1:
            raise ValueError("Instrumentation insertion anchor differs")
        value = value.replace(old, new, 1)
    generated = value.encode("utf-8")
    if restore(generated, helper) != original:
        raise ValueError("Changes outside instrumentation")
    return generated


def restore(generated, helper):
    if pin(helper.encode())["sha256"] != CLOCK_SHA:
        raise ValueError("Frozen CPU clock helper differs")
    value = generated.decode("utf-8")
    for old, new in reversed(replacements(helper)):
        if value.count(new) != 1:
            raise ValueError("Instrumentation inverse anchor differs")
        value = value.replace(new, old, 1)
    restored = value.encode("utf-8")
    if pin(restored)["sha256"] != SOURCE_SHA:
        raise ValueError("Restored adapter differs from frozen source")
    return restored


def expected_outputs():
    original, helper, baseline = SOURCE.read_bytes(), CLOCK_SOURCE.read_bytes(), BASELINE.read_bytes()
    if pin(baseline)["sha256"] != BASELINE_SHA:
        raise ValueError("Baseline solver source differs")
    generated = transform(original, helper.decode("utf-8"))
    diff = "".join(difflib.unified_diff(original.decode().splitlines(True), generated.decode().splitlines(True),
                                       fromfile="flat-ev/cloud32/solve.rs", tofile="cpu-occupancy/adapter/solve.rs")).encode()
    outputs = {"solve.rs": generated, "cpu_clock.rs.inc": helper, "adapter.patch": diff}
    receipt = {"schema": "r1.cpu-occupancy-adapter-source/v1", "source": pin(original),
               "baseline_solver": pin(baseline), "clock_helper": pin(helper),
               "generator": pin(Path(__file__).read_bytes()), "example": "flop_cpu_occupancy_probe",
               "generated": {name: pin(data) for name, data in outputs.items()},
               "cpu_schema": SCHEMA, "inverse_byte_identical": True,
               "native_compiled": False, "native_executed": False,
               "scope": "Static generation only; CPU occupancy and instrumentation perturbation uncalibrated"}
    outputs["provenance.json"] = (json.dumps(receipt, indent=2) + "\n").encode()
    return outputs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    outputs = expected_outputs()
    for name, data in outputs.items():
        path = HERE / name
        if args.check and (not path.exists() or path.read_bytes() != data):
            raise ValueError("Saved output differs: " + name)
        if path.exists() and path.read_bytes() != data:
            raise ValueError("Refusing to overwrite different output: " + name)
    if not args.check:
        for name, data in outputs.items():
            path = HERE / name
            if not path.exists():
                with path.open("xb") as stream:
                    stream.write(data)
    print(json.dumps({"status": "checked" if args.check else "generated", "files": {name: pin(data) for name, data in outputs.items()}}))


if __name__ == "__main__":
    main()
