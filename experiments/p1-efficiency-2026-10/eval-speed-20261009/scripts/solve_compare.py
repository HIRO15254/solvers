"""Full old/new flop solve and exact saved strategy/EV comparison."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import time

import zstandard

ROOT = Path(__file__).resolve().parents[4]
EXP = Path(__file__).resolve().parents[1]

def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def payload_sha(path):
    """Hash every decoded byte, zeroing only SolMeta.wall_secs (8 bytes)."""
    digest = hashlib.sha256()
    with path.open("rb") as compressed:
        digest.update(compressed.read(50))  # identical fixed header
        with zstandard.ZstdDecompressor().stream_reader(compressed) as stream:
            def varint():
                value = 0
                shift = 0
                while True:
                    byte = stream.read(1)
                    assert byte
                    digest.update(byte)
                    value |= (byte[0] & 127) << shift
                    if byte[0] < 128:
                        return value
                    shift += 7
            config_len = varint()
            digest.update(stream.read(config_len))
            varint()  # SolMeta.iterations
            digest.update(stream.read(40))  # expl[2], ev[2], nash_conv
            storage_len = varint()
            digest.update(stream.read(storage_len))
            wall = stream.read(8)
            assert len(wall) == 8
            digest.update(bytes(8))
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
    return digest.hexdigest()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--case", default="c_flop1")
    parser.add_argument("--storage", default="f32", choices=["f32", "i16", "i16-f32avg"])
    args = parser.parse_args()
    label = "full-solve" if args.case == "c_flop1" and args.storage == "f32" else f"full-{args.case}-{args.storage}"
    config = EXP / f"configs/{args.case}-{args.storage}-solve.toml"
    if not config.exists():
        text = (EXP / f"configs/{args.case}.toml").read_text(encoding="utf-8")
        text = text.replace('storage = "f32"', f'storage = "{args.storage}"')
        config.write_text(text + "\nfinal_checkpoint = false\n", encoding="utf-8")
    raw = EXP / "raw" / label
    raw.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%d-%H%M%S")
    records = []
    for version, binary in [("old", ROOT / "target/eval-old/release/solvers.exe"),
                            ("new", ROOT / "target/release/solvers.exe")]:
        run = ROOT / f"runs/p1eff/{label}-{stamp}-{version}"
        command = [str(binary), "solve", str(config), "--out", str(run), "--threads", "8"]
        print(version + " full solve", flush=True)
        start = time.perf_counter()
        proc = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=True)
        elapsed = time.perf_counter() - start
        (raw / (version + ".log")).write_text(proc.stdout + proc.stderr, encoding="utf-8")
        metrics = json.loads((run / "run.json").read_text())
        (raw / (version + "-run.json")).write_text(json.dumps(metrics, indent=2), encoding="utf-8")
        hashes = {}
        for view in ["strategy", "ev"]:
            export = raw / (version + "-" + view + ".json")
            cmd = [str(binary), "export", str(run / "solution.sol"), view, "--node", "root", "--output", str(export)]
            subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, check=True)
            hashes[view] = dict(sha256=sha(export), bytes=export.stat().st_size)
            # Root exports are compact evidence and stay alongside raw results.
            hashes[view]["retainedPath"] = str(export)
        records.append(dict(version=version, command=command, binarySha256=sha(binary), wallSeconds=elapsed,
                            metrics=metrics, exports=hashes, solution=dict(path=str(run / "solution.sol"),
                            sha256=sha(run / "solution.sol"), normalizedPayloadSha256=payload_sha(run / "solution.sol"), bytes=(run / "solution.sol").stat().st_size)))
    old, new = records
    assert old["solution"]["normalizedPayloadSha256"] == new["solution"]["normalizedPayloadSha256"]
    for key in ["evP0", "evP1", "explP0", "explP1", "nashConv"]:
        assert struct.pack("!d", old["metrics"][key]) == struct.pack("!d", new["metrics"][key]), key
    assert old["metrics"]["iterations"] == new["metrics"]["iterations"]
    for view in old["exports"]:
        assert old["exports"][view]["sha256"] == new["exports"][view]["sha256"], view
    (raw / "result.json").write_text(json.dumps(dict(records=records, identicalMetrics=True, identicalExports=True, identicalFullPayloadExceptWallSeconds=True), indent=2), encoding="utf-8")
    print(json.dumps(records, indent=2), flush=True)
    for key in ["evP0", "evP1", "explP0", "explP1", "nashConv"]:
        print(f"old=new {key}={old['metrics'][key]:.17g}", flush=True)

if __name__ == "__main__":
    main()
