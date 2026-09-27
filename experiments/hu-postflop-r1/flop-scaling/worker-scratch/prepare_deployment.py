"""Generate VM15 controls and a small, source-pinned deployment package only."""
import hashlib
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
OLD = HERE.parent / "flat-ev/cloud32"
CLOUD = ROOT / "experiments/hu-postflop-r1/cloud"
VM = CLOUD / "vm15"


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def main():
    VM.mkdir(exist_ok=True)
    records = {}
    for name in ("capture-command.py", "start.py", "run.sh", "recover.sh", "fetch-dependencies.sh"):
        raw = (CLOUD / "vm14" / name).read_bytes()
        text = raw.decode().replace("VM14", "VM15").replace("vm14", "vm15")
        text = text.replace("flat-ev", "worker-scratch").replace("flat", "worker")
        if name == "run.sh":
            text = text.replace('phase=prepare\n',
                'phase=durability-tests\npython3 -B -m unittest discover -s "$controls/.." -p test_durable.py -v > "$wrapper/durability-tests.stdout.log" 2> "$wrapper/durability-tests.stderr.log"\nphase=prepare\n', 1)
        path = VM / name
        path.write_text(text, encoding="utf-8", newline="\n")
        records[name] = {"original": pin(raw), "generated": pin(path.read_bytes())}
    (VM / "README.md").write_text(
        "# VM15 worker scratch experiment\n\n"
        "Research only. One small2-vCPU Spot VM for bootstrap and dependencies; resize the same stopped VM to32 CPUs for fresh native builds, candidate unit/integration tests, and64 matrix stages.\n"
        "Absolute75-minute cloud STOP is never extended; the experiment has at most40 minutes and ends at least15 minutes before STOP. Recover and delete the VM and auto-delete disk after a terminal outcome.\n"
        "No automatic retries, no resume across boots. Linux file/directory fsync and independent completed-case checkpoints protect finished blocks; incomplete blocks remain incomplete.\n"
        "The workload and performance guard are fixed in ../../flop-scaling/worker-scratch/protocol.jp.md. CPU-intensive verification stays on GCP.\n",
        encoding="utf-8")
    (VM / "control-derivation.json").write_text(json.dumps(records, indent=2) + "\n", encoding="utf-8")
    old = (OLD / "pack.py").read_text()
    text = old.replace("flat-ev", "worker-scratch").replace("flat", "worker").replace("vm14", "vm15")
    text = text.replace("deployment01.tar.gz", "deployment02.tar.gz")
    candidate = pin((HERE / "solver.rs").read_bytes())["sha256"]
    text = text.replace("ec9e5e6b5230684fce6f2315370ceda0e05fb4346eb7e4ff44ea05e6028aabcd", candidate)
    text = text.replace('HERE.parent / "timing/run.py"', 'ROOT / "experiments/hu-postflop-r1/flop-scaling/flat-ev/timing/run.py"')
    text = text.replace('HERE.parent / "generic-checks/execution-checks01/verification.json"', 'HERE.parent / "durable.py"')
    # Include checker and candidate provenance before any workload starts.
    text = text.replace('"protocol.md", "provenance.json", "adapter.patch", "install.py")',
                        '"protocol.md", "provenance.json", "adapter.patch", "install.py", "analyze.py", "derivation.json")')
    text = text.replace('controls += [ROOT /', 'controls += [HERE.parent / "provenance.json", HERE.parent / "candidate.patch", HERE.parent / "test_durable.py"]\n    controls += [ROOT /', 1)
    path = HERE / "cloud32/pack.py"
    path.write_text(text, encoding="utf-8", newline="\n")
    (HERE / "cloud32/protocol.md").write_bytes((HERE / "protocol.jp.md").read_bytes())
    spec = importlib.util.spec_from_file_location("worker_pack", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.main()


if __name__ == "__main__":
    main()
