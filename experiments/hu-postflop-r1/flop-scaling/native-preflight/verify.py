"""Verify retained native-preflight evidence; this does not rerun the solver."""
from __future__ import annotations

import gzip
import hashlib
import json
from pathlib import Path, PureWindowsPath

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
PROOF = HERE / "proof01"


def read(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def check_pin(path: Path, expected: dict) -> None:
    data = path.read_bytes()
    assert len(data) == expected["bytes"], path
    assert hashlib.sha256(data).hexdigest() == expected["sha256"], path


def successful_record(path: Path) -> dict:
    record = read(path)
    assert record["state"] == "completed" and record["child_exit_code"] == 0, path
    assert record["cleanup_complete"] and record["identity_unchanged"], path
    assert not record["errors"] and not record["forced"], path
    assert record["bounded_job_settings"] == {
        "limit_flags": 8704, "job_memory_limit_bytes": 536870912,
        "root_priority_class": 16384, "verified_before_resume": True,
    }, path
    return record


def main() -> None:
    manifest = read(PROOF / "manifest.json")
    for path, expected in manifest["files"].items():
        check_pin(PROOF / path, expected)
    for path, expected in manifest["source_pins"].items():
        check_pin(ROOT / path.replace("\\", "/"), expected)
    binary = gzip.decompress((PROOF / "flop_native_probe.exe.gz").read_bytes())
    expected = manifest["executable_uncompressed"]
    assert len(binary) == expected["bytes"]
    assert hashlib.sha256(binary).hexdigest() == expected["sha256"]

    for name, expected_success in [("below-cap", True), ("above-cap", False)]:
        directory = PROOF / "calibration" / name
        successful_record(directory / "record.json")
        allocation = read(directory / "record.stdout.log")
        assert allocation["allocation_succeeded"] is expected_success
        assert (allocation["winerror"] == 0) is expected_success
    directory = PROOF / "calibration/aggregate-child-cap"
    successful_record(directory / "record.json")
    parent, child = [json.loads(line) for line in (directory / "record.stdout.log").read_text().splitlines()]
    assert parent["parent_allocation_succeeded"] and parent["parent_requested_commit_bytes"] == 300 * 1024**2
    assert child["child_exit"] == 0 and child["child_stderr"] == ""
    allocation = json.loads(child["child_stdout"])
    assert allocation["requested_commit_bytes"] == 256 * 1024**2
    assert not allocation["allocation_succeeded"] and allocation["winerror"] != 0
    # Deliberately do not compare reported PeakJobMemoryUsed with the cap:
    # failed requests returned peaks above it; their accounting cause is unknown.

    failed = read(PROOF / "build01-failed/00-cards.json")
    assert failed["state"] == "failed" and failed["child_exit_code"] == 1
    assert failed["cleanup_complete"] and failed["identity_unchanged"]
    assert PureWindowsPath(failed["resolved_argv"][0]).name.lower() == "rustup.exe"
    old_build = read(PROOF / "build01-failed/build-receipt.json")
    old_script = next(v for p, v in old_build["source_pins"].items() if p.endswith("build_probe.py"))
    check_pin(PROOF / "build01-failed/build_probe.py", old_script)
    for path, expected in old_build["source_pins"].items():
        current = (PROOF / "build01-failed/build_probe.py" if path.endswith("build_probe.py")
                   else ROOT / path.replace("\\", "/"))
        check_pin(current, expected)
    build = read(PROOF / "build02/build-receipt.json")
    assert build["all_stages_passed"] and build["sources_unchanged"] and build["cached_external_unchanged"]
    assert [x["name"] for x in build["stages"]] == ["cards", "engine", "game", "hand_index", "holdem", "probe"]
    for path, expected in build["source_pins"].items():
        check_pin(ROOT / path.replace("\\", "/"), expected)
    for stage in build["stages"]:
        successful_record(PROOF / "build02" / stage["record"])
    assert {k: build["stages"][-1]["artifact"][k] for k in ("bytes", "sha256")} == manifest["executable_uncompressed"]

    static = read(HERE.parent / "fixtures/static-check.json")
    summaries = []
    for case, fixture in static["fixtures"].items():
        check_pin(HERE.parent / "fixtures" / fixture["file"], fixture)
        expected_support = [fixture["ranges"][seat]["positive_root_combos"] for seat in ("oop", "ip")]
        for phase in ("count01", "construct01"):
            record = successful_record(PROOF / phase / f"{case}.json")
            rows = [json.loads(line) for line in (PROOF / phase / f"{case}.stdout.log").read_text().splitlines()]
            assert len(rows) == (1 if phase == "count01" else 2)
            count = rows[0]
            assert count["static_match"] and count["root_support"] == expected_support
            assert count["nodes"] == static["tree"]["nodes"]
            assert count["terminals"] == static["tree"]["terminals"]
            assert count["distinct_showdown_boards"] == static["tree"]["unique_river_board_sets"]
            assert count["f32_arena_bytes"] == fixture["f32_regrets_plus_sums_bytes"]
            assert count["root_combo_ids"] == [fixture["ranges"][s]["global_combo_ids"] for s in ("oop", "ip")]
            if phase == "construct01":
                actual = rows[1]
                assert actual["estimate_and_static_match"] and not actual["solver_storage_allocated"] and not actual["solve_executed"]
                assert actual["storage_elements_per_buffer"] == fixture["storage_elements_per_buffer"]
                assert actual["normalizer"] == {"narrow": 870, "expanded": 8700}[case]
                for action in ("bet", "raise"):
                    assert actual[f"{action}_available_nodes_by_street"] == [
                        static["tree"]["counts"][f"{street}_{action}_available_nodes"] for street in ("flop", "turn", "river")]
                summaries.append({"case": case, "nodes": actual["nodes"], "root_support": expected_support,
                                  "root_os_peak_resident_bytes": record["measurement"]["root_os_peak_resident_bytes"]})
    print(json.dumps({"verified": True, "retained_files": len(manifest["files"]),
                      "native_solve_executed": False, "cases": summaries}, indent=2))


if __name__ == "__main__":
    main()
