"""Portable final-pipeline check using trusted checkout code, never retained code."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("trusted_final_pipeline", Path(__file__).with_name("run.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)
if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--expect", choices=("completed", "failed", "ready", "awaiting_build"))
    args = parser.parse_args()
    report = runner.check(args.out)
    if args.expect:
        runner.require(report["status"] == args.expect, "unexpected terminal state")
    print(json.dumps(report, indent=2, allow_nan=False))
