"""Portable verification; executes only this checkout's trusted verifier code."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("exact_mass_campaign", Path(__file__).with_name("run.py"))
campaign = importlib.util.module_from_spec(spec)
spec.loader.exec_module(campaign)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--expect", choices=("ready", "completed", "failed"))
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    result = campaign.check(args.out)
    campaign.require(args.expect is None or result["status"] == args.expect, "unexpected status")
    if args.report:
        campaign.save(args.report, result)
    print(json.dumps(result, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
