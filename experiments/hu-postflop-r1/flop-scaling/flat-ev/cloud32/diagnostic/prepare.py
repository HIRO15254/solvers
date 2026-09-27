"""Generate a reversible, unexecuted process-CPU diagnostic adapter; no compiler."""
import difflib
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
SOURCE = HERE.parent / "solve.rs"
SOURCE_SHA = "63cba37a5b0f79dfed2ca0ddf879daada3f0fee094ee5f287f86bc1f37414d46"


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def replacements(helper):
    cpu_output = r'''    write_json(
        &out.join("cpu.json"),
        &format!(
            "{{\"schema\":\"r1.flop-cloud32-process-cpu/v1\",\"case\":\"{case}\",\"threads\":{threads},\"iterations\":{iterations},\"clock\":\"CLOCK_PROCESS_CPUTIME_ID\",\"clock_id\":2,\"scope\":\"all threads in this process\",\"cfr_cpu_seconds\":{cfr_cpu_seconds},\"quality_cpu_seconds\":{quality_cpu_seconds},\"ev_cpu_seconds\":{ev_cpu_seconds:?},\"br_cpu_seconds\":{br_cpu_seconds:?},\"exploitability_cpu_seconds\":{exploitability_cpu_seconds},\"cfr_wall_seconds\":{cfr_seconds},\"quality_wall_seconds\":{quality_seconds},\"performance_claim\":false}}"
        ),
    )?;
'''
    return [
        ("\nfn event(", "\n" + helper + "\nfn event("),
        ('    event("cfr", "started");\n    let started = Instant::now();',
         '    event("cfr", "started");\n    let cfr_cpu_started = process_cpu_ns();\n    let started = Instant::now();'),
        ('    let cfr_seconds = started.elapsed().as_secs_f64();',
         '    let cfr_seconds = started.elapsed().as_secs_f64();\n    let cfr_cpu_seconds = process_cpu_elapsed(cfr_cpu_started);'),
        ('    let started = Instant::now();\n    let mut ev = [0.0_f64; 2];',
         '    let quality_cpu_started = process_cpu_ns();\n    let started = Instant::now();\n'
         '    let mut ev_cpu_seconds = [0.0_f64; 2];\n    let mut br_cpu_seconds = [0.0_f64; 2];\n'
         '    let mut ev = [0.0_f64; 2];'),
        ('        ev[p.index()] = pool.install(|| solver.expected_value(p));',
         '        let phase_cpu_started = process_cpu_ns();\n'
         '        ev[p.index()] = pool.install(|| solver.expected_value(p));\n'
         '        ev_cpu_seconds[p.index()] = process_cpu_elapsed(phase_cpu_started);'),
        ('        br[p.index()] = pool.install(|| solver.best_response_value(p));',
         '        let phase_cpu_started = process_cpu_ns();\n'
         '        br[p.index()] = pool.install(|| solver.best_response_value(p));\n'
         '        br_cpu_seconds[p.index()] = process_cpu_elapsed(phase_cpu_started);'),
        ('    let gains = pool.install(|| solver.exploitability()).0;',
         '    let exploitability_cpu_started = process_cpu_ns();\n'
         '    let gains = pool.install(|| solver.exploitability()).0;\n'
         '    let exploitability_cpu_seconds = process_cpu_elapsed(exploitability_cpu_started);'),
        ('    let quality_seconds = started.elapsed().as_secs_f64();',
         '    let quality_seconds = started.elapsed().as_secs_f64();\n'
         '    let quality_cpu_seconds = process_cpu_elapsed(quality_cpu_started);'),
        ('    event("probe", "completed");', cpu_output + '    event("probe", "completed");'),
    ]


def transform(original, helper):
    if pin(original)["sha256"] != SOURCE_SHA:
        raise ValueError("Frozen Cloud32 adapter differs")
    value = original.decode("utf-8")
    for old, new in replacements(helper):
        if value.count(old) != 1:
            raise ValueError("CPU insertion anchor differs")
        value = value.replace(old, new, 1)
    generated = value.encode("utf-8")
    if restore(generated, helper) != original:
        raise ValueError("Changes outside CPU instrumentation")
    return generated


def restore(generated, helper):
    value = generated.decode("utf-8")
    for old, new in reversed(replacements(helper)):
        if value.count(new) != 1:
            raise ValueError("CPU inverse anchor differs")
        value = value.replace(new, old, 1)
    restored = value.encode("utf-8")
    if pin(restored)["sha256"] != SOURCE_SHA:
        raise ValueError("Restored adapter differs from frozen source")
    return restored


def main():
    original = SOURCE.read_bytes()
    helper_raw = (HERE / "cpu_clock.rs.inc").read_bytes()
    generated = transform(original, helper_raw.decode("utf-8"))
    patch = "".join(difflib.unified_diff(original.decode().splitlines(True), generated.decode().splitlines(True),
                                        fromfile="cloud32/solve.rs", tofile="cloud32/diagnostic/solve.rs")).encode()
    outputs = {"solve.rs": generated, "adapter.patch": patch}
    for name, data in outputs.items():
        path = HERE / name
        if path.exists() and path.read_bytes() != data:
            raise ValueError("Refusing to overwrite different generated output: " + name)
    for name, data in outputs.items():
        (HERE / name).write_bytes(data)
    receipt = {"schema": "r1.flat-ev-cpu-adapter-source/v1", "source": pin(original),
               "helper": pin(helper_raw), "generator": pin(Path(__file__).read_bytes()),
               "generated": {name: pin(data) for name, data in outputs.items()},
               "inverse_byte_identical": True, "native_compiled": False, "native_executed": False,
               "scope": "Static adapter generation only; process CPU measurement and perturbation uncalibrated"}
    (HERE / "provenance.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
