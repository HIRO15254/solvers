"""Offline resource-envelope follow-up; no API, subprocess, archive or ledger writes.

Generate once with --write; replay with --check. After an authorized ledger
update, pass its immutable pre-update snapshot with --budget PATH.
"""
import argparse
import datetime as dt
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import html
import importlib.util
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
ROOT = CLOUD.parents[2]
PROJECT = "solvers-abstraction-20260723"
ZONE = "us-central1-b"
BASE = f"https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}"
HOLDS = {"08": (D("4.50"), D("3.70")), "09": (D("1.60"), D("1.50")),
         "10": (D("1.80"), D("1.60")), "11": (D("1.80"), D("1.60")),
         "12": (D("1.80"), D("1.70")), "13": (D("1.60"), D("1.40")),
         "14": (D("1.90"), D("1.70")), "15": (D("2.00"), D("1.70"))}


def pin(path):
    raw = path.read_bytes()
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def read(path):
    def pairs(items):
        result = {}
        for key, value in items:
            assert key not in result, "duplicate JSON key"
            result[key] = value
        return result
    return json.loads(path.read_text(encoding="utf-8-sig"), parse_float=D,
                      object_pairs_hook=pairs,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def seconds(start, end):
    times = [dt.datetime.fromisoformat(value.replace("Z", "+00:00")) for value in (start, end)]
    assert all(value.utcoffset() is not None for value in times)
    diff = times[1] - times[0]
    return D(diff.days * 86400 + diff.seconds) + D(diff.microseconds) / 1000000


def minutes(start, end):
    duration = seconds(start, end)
    assert 0 < duration < 86400
    return int(((duration + 120) / 60).to_integral_value(rounding=ROUND_CEILING))


def module(path):
    spec = importlib.util.spec_from_file_location("retained_" + path.parent.name.replace("-", "_"), path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def one(value):
    if isinstance(value, list):
        value, = value
    return value


def build_report(budget_path):
    evidence = {}

    def retain(path):
        path = path.resolve()
        assert path.is_relative_to(ROOT)
        evidence[path.relative_to(ROOT).as_posix()] = pin(path)
        return path

    def load(path):
        return read(retain(path))

    def verify(path, expected):
        assert pin(retain(path)) == {key: expected[key] for key in ("bytes", "sha256")}, str(path)

    budget = read(budget_path)
    assert budget["authorized_limit"] == 40
    held = sum(D(str(row["reserved_usd"])) for row in budget["reservations"] if not row["reservation_released"])
    assert held == D("39.95")
    # Re-run the retained semantic validators, not merely their reported booleans.
    older = CLOUD / "usage-vm08-vm13-20260927"
    prior08 = module(retain(older / "calculate.py")).calculate()
    assert prior08 == load(older / "report.json")
    manifest = load(older / "inputs.json")
    for name, expected in manifest["cloud_files"].items():
        verify(CLOUD / name, expected)
    verify(older / "budget-before.json", manifest["budget_snapshot"])
    prior_dir = CLOUD / "usage-lifecycle-20260927"
    prior_lifecycle = module(retain(prior_dir / "analyze.py")).build_report()
    assert prior_lifecycle == load(prior_dir / "report02.json")
    for name, expected in prior_lifecycle["inputs"].items():
        verify(CLOUD / name, expected)
    legacy = load(CLOUD / "usage-cost-bound-20260927/inputs.json")
    for name, expected in legacy["source_pins"].items():
        # This historical manifest's live-budget path refers to its saved copy.
        path = CLOUD / "usage-cost-bound-20260927/budget-before.json" if name.endswith("/cloud/budget.json") else ROOT / name
        verify(path, expected)
    by_vm = {row["vm"]: row for row in legacy["vms"]}
    acquisition = load(CLOUD / "usage-reconcile-20260927/acquisition.json")
    assert acquisition["status"] == "completed" and acquisition["project"] == PROJECT
    assert acquisition["cloud_mutations"] is False
    verify(CLOUD / "usage-reconcile-20260927/collect.py", acquisition["collector"])

    prices = {}
    for stem, identity, expected in (("compute-e2-standard-2", "e2-standard-2", ".06701142"),
                                   ("compute-e2-highcpu-32", "e2-highcpu-32", ".79152384")):
        path = CLOUD / "preflight-vm18" / f"pricing-{stem}-row.html"
        value = html.unescape(re.sub(r"<[^>]*>", " ", retain(path).read_text(encoding="utf-8")))
        assert identity in value
        row_prices = [D(item) for item in re.findall(r"\$([0-9]+(?:\.[0-9]+)?)", value)]
        assert max(row_prices) == D(expected)
        prices[identity] = D(expected)

    rows = []
    for vm, (old_hold, new_hold) in HOLDS.items():
        date = "20260927" if vm in ("14", "15") else "20260926"
        name = f"solvers-r1-{date}-{vm}"
        records = [row for row in budget["reservations"] if row["instance"] == name]
        assert len(records) == (2 if vm == "08" else 1)
        assert sum(D(str(row["reserved_usd"])) for row in records) == old_hold
        assert all(row["billed_usd"] is None and row["reservation_released"] is False for row in records)
        uncertainty = sum(D(str(row["tax_price_delay_and_other_reserve_usd"])) for row in records)
        assert uncertainty == (2 if vm == "08" else 1)
        base_record = next(row for row in records if not row["id"].endswith("scale32"))
        assert base_record["maximum_download_gib"] == 1 and base_record["disk_gib"] == 40
        assert all(row["project"] == PROJECT and row["zone"] == ZONE for row in records)
        high_minutes = 0
        if vm in ("14", "15"):
            prior = next(row for row in prior_lifecycle["rows"] if row["vm"] == vm)
            identity = prior["instance_id"]
            start, absent = prior["launch_attempt"], prior["absence_verified"]
            whole = minutes(start, absent)
            assert whole == prior["whole_minutes"]
            window = prior["overlapping_high_window"]
            high_minutes = minutes(window["start"], window["end"])
            assert high_minutes == prior["overlapping_high_minutes"]
            traffic = prior["network"]
            rate = prices["e2-standard-2"]
            disk_minutes = whole
            disk_basis = prior["disk_identity_scope"] + "; unchanged prior launch-to-absence bound"
            machine_scope = "Prior validator checks successful same-ID E2-only 2 -> 32 -> 2 lifecycle; small rate covers entire life and high window overlaps."
        else:
            prior = by_vm[vm]
            identity = prior["instance_id"]
            start, absent = prior["request_start"], prior["absence_verified"]
            whole = minutes(start, absent)
            launch = load(CLOUD / f"launch-r1-{date}-{vm}.json")
            created = one(load(CLOUD / f"create-result-r1-{date}-{vm}.json"))
            assert launch["attempted_at"] == start
            assert created["id"] == identity and created["name"] == name
            assert seconds(start, created["creationTimestamp"]) >= 0
            argv = launch["argv"]
            assert argv[:4] == ["compute", "instances", "create", name]
            assert f"--project={PROJECT}" in argv and f"--zone={ZONE}" in argv
            assert "--boot-disk-auto-delete" in argv and "--boot-disk-size=40GB" in argv
            assert "--boot-disk-type=pd-balanced" in argv and "--provisioning-model=SPOT" in argv
            assert not any(arg == "--disk" or arg.startswith(("--disk=", "--create-disk=")) for arg in argv)
            disk, = created["disks"]
            assert disk["autoDelete"] is True and disk["boot"] is True and int(disk["diskSizeGb"]) == 40
            assert disk["source"] == f"{BASE}/disks/{name}"
            rate = D(".14")
            disk_minutes = whole
            disk_basis = "New 40GiB auto-delete boot disk requested in instance create; same disk name/URI and numeric ID at predelete, raw empty named disk inventory after same-ID delete. Launch-to-absence plus120s then whole-minute ceiling; no guessed creation timestamp."
            machine_scope = "4-vCPU rounded regular rate over the entire lifecycle, including stopped intervals."
            if vm == "08":
                old = prior08["vms"][vm]
                assert whole == old["lifecycle"]["rounded_minutes"]
                high_minutes = old["lifecycle"]["32cpu_rounded_minutes"]
                assert high_minutes == minutes(old["lifecycle"]["32cpu_conservative_start"], absent)
                predelete = load(CLOUD / "cleanup-vm08/predelete.json")
                assert predelete["instance_id"] == identity
                assert predelete["machine_type"] == f"{BASE}/machineTypes/e2-highcpu-32"
                disk_minutes = 24 * 60
                disk_basis = "Full original24h disk allowance retained; earlier reconciliation lacks separate raw disk-ID record."
                machine_scope = "Retained validator checks 2 -> 4 -> E2 highcpu32 and same E2 restart; predelete also E2 highcpu32. Whole life charged at4-vCPU rate plus overlapping32-vCPU window from plan before resize through absence. Both original1USD buffers retained."
            elif vm == "09":
                directory = CLOUD.parent / "showdown-kernel"
                cleanup = load(directory / "cleanup.json")
                before = one(load(directory / "vm09-final-disk.json"))
                assert cleanup["instance_id"] == identity and cleanup["instance"] == name
                assert cleanup["checked_at_utc"] == absent
                for kind in ("instances", "disks", "addresses"):
                    assert load(directory / f"{kind}-after-delete.json") == []
            elif vm == "10":
                directory = CLOUD / "cleanup-vm10"
                cleanup = load(directory / "reconciliation.json")
                before = load(directory / "disk-before.json")
                assert cleanup["instance_id"] == identity and cleanup["instance"] == name
                assert cleanup["reconciled_at_utc"] == absent
                for kind in ("instances", "disks", "addresses"):
                    assert load(directory / f"{kind}-after.json") == []
                operations = load(directory / "delete-operations-after.json")
                deletion, = [row for row in operations if row["targetId"] == identity]
                assert deletion["status"] == "DONE" and deletion["operationType"] == "delete" and not deletion.get("error")
                assert seconds(deletion["endTime"], absent) >= 0
            else:
                directory = CLOUD / f"cleanup-vm{vm}/run01"
                cleanup = load(directory / "reconciliation.json")
                before = load(directory / "disk-before.stdout.log")
                assert cleanup["instance_id"] == identity and cleanup["instance"] == name
                assert cleanup["status"] == "deleted_and_absence_verified" and cleanup["ended_at_utc"] == absent
                for kind in ("instances", "disks", "addresses"):
                    assert cleanup["readbacks"][kind + "-after"] == []
                    assert load(directory / f"{kind}-after.stdout.log") == []
                deletion, = cleanup["matching_done_delete_operations"]
                assert deletion["targetId"] == identity and deletion["status"] == "DONE"
                assert deletion["operationType"] == "delete" and not deletion.get("error")
                assert seconds(deletion["endTime"], absent) >= 0
            if vm != "08":
                assert before["id"] == cleanup["disk_id"] and before["name"] == name
                assert before["users"] == [f"{BASE}/instances/{name}"] and int(before["sizeGb"]) == 40
                if "creationTimestamp" in before:
                    assert seconds(start, before["creationTimestamp"]) >= 0
                if "selfLink" in before:
                    assert before["selfLink"] == f"{BASE}/disks/{name}"
                if vm == "12":
                    running = one(load(CLOUD / "vm12/start32-01.stdout.log"))
                    assert running["id"] == identity and running["machineType"] == f"{BASE}/machineTypes/e2-highcpu-32"
                    predelete = load(directory / "instance-before.stdout.log")
                    assert predelete["id"] == identity
                    rate = D("1.15")
                    machine_scope = "Whole lifecycle retains original higher N2 fallback ceiling1.15/h, including initial4CPU phase and stopped intervals. Running record shows E2 highcpu32; predelete query omitted machineType so no actual-machine price reduction is used."
                else:
                    assert "--machine-type=e2-standard-4" in argv
            request, = [row for row in acquisition["requests"] if row.get("vm") == vm and row["metric"].endswith("sent_bytes_count")]
            assert request["http_status"] == 200 and request["page"] == 0 and request["method"] == "GET"
            response_path = CLOUD / "usage-reconcile-20260927" / request["response"]["path"]
            verify(response_path, request["response"])
            response = load(response_path)
            assert not response.get("nextPageToken")
            series, = response["timeSeries"]
            assert series["resource"]["labels"] == {"zone": ZONE, "project_id": PROJECT, "instance_id": identity}
            assert series["metricKind"] == "DELTA" and series["valueType"] == "INT64"
            assert series["metric"]["type"] == request["metric"]
            traffic = module(older / "calculate.py").summarize(series["points"])
            assert D(traffic["observed_sum"]) < 1024**3
        costs = {"whole_lifetime_compute": D(whole) / 60 * rate,
                 "overlapping_highcpu32_compute": D(high_minutes) / 60 * prices["e2-highcpu-32"],
                 "whole_lifetime_ipv4": D(whole) / 60 * D(".0025"),
                 "disk": D(disk_minutes) / 60 * 40 * D(".000137"),
                 "unchanged_one_gib_network_allowance": D(".30"),
                 "unchanged_original_uncertainty": uncertainty}
        total = sum(costs.values())
        assert total <= new_hold < old_hold
        allocation = {record["id"]: str(new_hold) for record in records}
        if vm == "08":
            allocation = {"r1-20260926-08": "2.20", "r1-20260926-08-scale32": "1.50"}
        rows.append({"vm": vm, "instance_id": identity, "launch_attempt": start, "absence_verified": absent,
                     "whole_minutes": whole, "overlapping_high_minutes": high_minutes,
                     "disk_minutes": disk_minutes, "disk_basis": disk_basis, "machine_scope": machine_scope,
                     "monitoring": traffic, "network_missing_usage_unknown": True,
                     "network_formal_cap_proven": False, "network_allowance_gib": 1,
                     "costs_usd": {key: str(value) for key, value in costs.items()}, "modeled_total_usd": str(total),
                     "held_before_usd": str(old_hold), "proposed_hold_usd": str(new_hold),
                     "restore_usd": str(old_hold-new_hold), "margin_above_model_usd": str(new_hold-total),
                     "reservation_allocation_usd": allocation})
    restored = sum(D(row["restore_usd"]) for row in rows)
    return {"schema": "r1.usage-followup-proposal/v1", "source": pin(Path(__file__)),
            "ledger_observation": pin(budget_path), "rows": rows, "input_pins": dict(sorted(evidence.items())),
            "summary": {"authorized_usd": "40", "held_before_usd": str(held), "restore_usd": str(restored),
                        "proposed_held_after_usd": str(held-restored), "proposed_available_usd": str(40-held+restored)},
            "new_api_calls": 0, "native_execution": False, "archive_access": False, "budget_changed": False,
            "billed_usd": None, "guaranteed_invoice_ceiling": False,
            "limits": ["Previously acquired usage and lifecycle only; no new tariff or usage acquisition is claimed.",
                       "Use regular E2 prices from official captured rows, no Spot discount or credits.",
                       "All original uncertainty and1GiB network allowances retained. Gaps and edges remain unknown, not zero.",
                       "Every lifetime includes stopped time and120s extra, rounded up to whole minutes; high windows overlap small-CPU coverage.",
                       "VM08 retains full24h disk; VM09-13 use new-boot-disk request through verified absence, not an invented creation timestamp.",
                       "VM02/05 untouched: historical raw disk-absence evidence is weaker.",
                       "Proposal only. Root must independently review and apply before funds become available."]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--budget", type=Path, default=CLOUD / "budget.json")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = build_report(args.budget)
    raw = (json.dumps(report, indent=2) + "\n").encode()
    path = HERE / "report.json"
    if args.check:
        assert path.read_bytes() == raw
    else:
        with path.open("xb") as stream:
            stream.write(raw)
    print(json.dumps(report["summary"]))
