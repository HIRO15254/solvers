#!/usr/bin/env python3
"""Verify retained River diagnostics offline; never certify unknown reference conditions."""
from __future__ import annotations

import argparse
from decimal import Decimal
import importlib.util
import io
import json
from pathlib import Path
import sys
import tarfile

HERE = Path(__file__).resolve().parent
EXP = HERE.parent


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def verify():
    common_path = EXP / "pipeline/vm06-comparison-verification.py"
    common = module(common_path, "river_common")
    common.BUNDLES["reference/evidence-vm06-river"] = (
        "vm06-river-diagnostics.tar.gz",
        "16908b41d812497625ed1fe3da78de576e13310064d8a36d71b7171f52025e8b", 97)
    ev = common.Evidence()
    require = common.require
    build = ev.json("/opt/r1/comparison-build/identity-after.json")
    require(build["state"] == "completed" and build["old_targets_reused"] is False,
            "missing fresh source03 build")
    reports = []
    intervals = []
    for number, nodes in (("019", 33), ("017", 81)):
        case = "HU-R0-" + number
        root = "/opt/r1/diagnostic-" + number + "/"
        execution = ev.json(root + "execution.json")
        require(execution["state"] == "completed" and execution["case_id"] == case,
                "diagnostic did not complete")
        require(execution["binary"] == build["binaries"]["candidate-v3"], "wrong binary")
        require(execution["source_id"] == "source-archive-sha256:" +
                build["sources"]["candidate-v3"]["sha256"], "wrong source")
        ev.bound(execution["binary"])
        ev.bound(build["sources"]["candidate-v3"])
        archive_bytes = ev.raw(execution["input_archive"])
        with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:gz") as archive:
            members = {item.name: item for item in archive if item.isfile()}
            require(len(members) == len(execution["inputs"]), "input archive shape differs")
            for name, identity in execution["inputs"].items():
                expected = ev.raw(identity)
                require(archive.extractfile(members[identity["archive_member"]]).read() == expected,
                        "archive/input bytes differ")
                if name != "README.md":
                    require((HERE / case / name).read_bytes() == expected,
                            "local diagnostic input differs: " + name)
        stages = execution["stages"]
        require([s["name"] for s in stages] == ["input_check", "config_validate", "solve",
                "export_tree", "export_summary", "compare"], "stage set differs")
        for stage in stages:
            adapted = {**stage, "record": stage["supervisor_record"],
                       "exit_code": stage["child_exit_code"], "stdout": stage["outputs"]["stdout"]}
            record = ev.stage(adapted)
            require(record["argv"] == stage["argv"] and record["outputs"] == stage["outputs"]
                    and stage["supervisor_exit_code"] == 0, "execution/supervisor differs")
            interval = [common.instant(record["started_at"]), common.instant(record["ended_at"])]
            require(not intervals or intervals[-1][1] <= interval[0], "diagnostic stages overlap")
            intervals.append(interval)
        for artifact in execution["artifacts"].values():
            require(artifact["status"] == "present", "diagnostic artifact missing")
            ev.bound(artifact)
        status = ev.json(root + "comparison-status.json")
        comparison = ev.json(status["report"])
        require(status["case_id"] == comparison["case_id"] == case, "case mismatch")
        for item in (status, comparison, comparison["ev_diagnostic"]):
            require(item["condition_match"] == "unverified" and
                    item["quality_status"] == "not_evaluated" and item["acceptance"] is None,
                    "diagnostic upgraded to quality acceptance")
        checker_path = HERE / case / "check_diagnostic.py"
        checker = module(checker_path, "checker_" + number)
        observed = checker.read_json(HERE / case / "observed.json")
        local_run = (HERE / "evidence-vm06-river/records/river" /
                     ("diagnostic" + number) / "run/run.toml")
        for config, field in ((HERE / case / "diagnostic.toml", "input_check"),
                              (local_run, "run_config_check")):
            checked = json.loads(json.dumps(checker.check_config(config, observed), default=str))
            require(checked == comparison[field], "recomputed config check differs")
        tree = json.loads(ev.raw(root + "tree.json"), parse_float=Decimal)
        summary = json.loads(ev.raw(root + "summary.json"), parse_float=Decimal)
        computed_tree = checker.check_tree(tree, observed)
        computed_ev = checker.compare_summary(summary, observed, computed_tree)
        serialized_ev = json.loads(json.dumps(computed_ev, default=str))
        require(computed_tree == comparison["tree_check"] and computed_tree["public_nodes"] == nodes,
                "recomputed tree differs")
        require(serialized_ev == comparison["ev_diagnostic"], "recomputed EV diagnostics differ")
        for kind in ("tree", "summary"):
            require(common.sha(ev.raw(root + kind + ".json")) == comparison["exports_sha256"][kind],
                    "compared export hash differs")
        require(common.sha((HERE / case / "observed.json").read_bytes()) == comparison["observed_sha256"],
                "observation identity differs")
        reports.append({"case": case, "tree": computed_tree, "ev_diagnostic": serialized_ev,
                        "execution": common.identity(HERE / "evidence-vm06-river/records/river" /
                                                     ("diagnostic" + number) / "execution.json"),
                        "checker": common.identity(checker_path),
                        "source_id": execution["source_id"], "binary": execution["binary"]})
    return {"schema": "solvers.r1-river-diagnostic-verification/v1",
            "status": "verified_diagnostic_only", "verifier": common.identity(Path(__file__)),
            "shared_evidence_verifier": common.identity(common_path), "bundles": ev.bundles,
            "verified_supervisor_stages": len(ev.stages), "cases": reports,
            "quality_status": "not_evaluated", "acceptance": None,
            "limitations": ["Source03 diagnostic, not final source06 performance or 24-case acceptance.",
                            "Rake collection semantics, reference solve version and accuracy remain unverified.",
                            "EV is live average-profile metadata; no saved-policy BR audit in these diagnostics.",
                            "Rounded reference EV agreement cannot establish identical finite games."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    try:
        report = verify()
        rendered = json.dumps(report, indent=2, allow_nan=False) + "\n"
        if args.out:
            with args.out.open("x", encoding="utf-8", newline="\n") as stream:
                stream.write(rendered)
            print(json.dumps({"status": report["status"], "stages": report["verified_supervisor_stages"]}))
        else:
            print(rendered, end="")
        return 0
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
        print(f"River verification failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
