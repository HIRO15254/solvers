"""Verify retained action-scaling bytes without original VM paths or executing retained code."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("checked_action_runner", HERE / "run.py")
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)


def verify(directories):
    store = run.portable.Store(directories)
    plans = [path for path in store.paths if path.endswith("/plan.json")
             and store.read(path).get("schema") == "r1.action-scaling-plan/v1"]
    run.require(plans, "no action-scaling plan retained")
    results = []
    for path in plans:
        plan = store.read(path)
        # Collectors may retain source files only inside the exact archive.
        # Verify its manifest/hash/file set and expose those original bytes
        # before the frozen runner checks pins for source-local helpers.
        # Nothing from the retained source is imported or executed.
        for role in run.ROLES:
            run.portable.source(store, plan["sources"][role]["root"],
                                plan["pins"][role + "_manifest"], plan["pins"][role + "_archive"])
        output = str(run.PurePosixPath(path).parent)
        results.append({"plan": store.identity(path), "result": store.identity(run.join(output, "result.json")),
                        "summary": run.check(store, output)})
    return {"schema": "r1.action-scaling-portable-verification/v1", "payload_integrity": "verified", "runs": results,
            "recovered_content_aliases": store.recovered_aliases,
            "scope": "Original source/binary/log/canonical bytes verified; compiler and Python executable bytes may be identity-only. Source-after is the recorded validation assertion. Native OS peak is recorded, while sampled memory is recomputed. Failed runs have no adoption guard."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--retained", type=Path, action="append", required=True)
    parser.add_argument("--expect", choices=("completed", "failed"))
    args = parser.parse_args()
    report = verify(args.retained)
    if args.expect:
        for item in report["runs"]:
            run.require((item["summary"].get("outcome") == "failed") == (args.expect == "failed"), "unexpected campaign outcome")
    print(json.dumps(report, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
