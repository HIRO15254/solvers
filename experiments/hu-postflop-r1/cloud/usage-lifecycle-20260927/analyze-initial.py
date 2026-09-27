"""Offline, additive lifecycle estimate. Never contacts GCP or changes the ledger."""
import argparse
import datetime as dt
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
PROJECT = "solvers-abstraction-20260723"
ZONE = "us-central1-b"
BASE = f"https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}"
SELECTED = {
    "06": ("5209515640504390740", D("4.20")),
    "07": ("841167209049583155", D("3.60")),
    "14": ("5878531675315846175", D("1.90")),
    "15": ("5437217996035927118", D("2.00")),
}


def pin(path):
    raw = path.read_bytes()
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def read(path):
    def pairs(items):
        result = {}
        for k, v in items:
            assert k not in result, "duplicate JSON key"
            result[k] = v
        return result
    return json.loads(path.read_text(encoding="utf-8-sig"), parse_float=D,
                      object_pairs_hook=pairs,
                      parse_constant=lambda s: (_ for _ in ()).throw(ValueError(s)))


def seconds(a, b):
    times = [dt.datetime.fromisoformat(x.replace("Z", "+00:00")) for x in (a, b)]
    assert all(t.utcoffset() is not None for t in times)
    delta = times[1] - times[0]
    return D(delta.days * 86400 + delta.seconds) + D(delta.microseconds) / 1000000


def calculate(lifetime, disk_gib, other, rate, large=None):
    assert lifetime > 0 and disk_gib > 0 and other > 0
    whole = int(((lifetime + 120) / 60).to_integral_value(rounding=ROUND_CEILING))
    costs = {"whole_lifetime_compute": D(whole) / 60 * rate,
             "whole_lifetime_ipv4": D(whole) / 60 * D(".0025"),
             "disk_whole_lifetime": disk_gib * D(whole) / 60 * D(".000137"),
             "one_gib_egress_allowance": D(".30"), "original_other_reserve": other}
    high = None
    if large is not None:
        assert 0 < large <= lifetime
        high = int(((large + 120) / 60).to_integral_value(rounding=ROUND_CEILING))
        costs["overlapping_large_compute"] = D(high) / 60 * D("1.15")
    return whole, high, costs


def held_total(reservations):
    return sum(D(str(r["reserved_usd"])) for r in reservations if not r["reservation_released"])


def command(record, vm_name, verb, machine=None, resource="instances"):
    a = record["argv"]
    assert record["exit_code"] == 0
    assert a[1:4] == ["compute", resource, verb]
    if verb != "list":
        assert a[4] == vm_name
    for key, value in (("project", PROJECT), ("zone", ZONE)):
        if key == "zone" and verb == "list":
            continue
        assert [v for v in a if v.startswith(f"--{key}=")] == [f"--{key}={value}"]
        assert f"--{key}" not in a
    assert [v for v in a if v.startswith("--machine-type")] == ([] if machine is None else ["--machine-type=" + machine])
    assert not any(v.startswith("--termination") for v in a)
    assert seconds(record["started_utc"], record["ended_utc"]) >= 0


def build_report():
    evidence = {}

    def retain(p):
        evidence[p.relative_to(CLOUD).as_posix()] = pin(p)
        return p

    def load(p):
        return read(retain(p))

    def verified(p, expected):
        assert pin(retain(p)) == {k: expected[k] for k in ("bytes", "sha256")}

    def acquisition(directory):
        a = load(directory / "acquisition.json")
        assert a["status"] == "completed" and a["project"] == PROJECT
        verified(directory / "collect.py", a["source"])
        for item in a["inputs"].values():
            p = directory / item["path"]
            assert p.resolve().is_relative_to(directory.resolve())
            verified(p, item)
        for q in a["requests"]:
            assert q["http_status"] == 200 and q["method"] == "GET" and q["page"] == 0
            verified(directory / q["response"]["path"], q["response"])
        return a

    def observed(a, directory, vm, instance):
        selected = [q for q in a["requests"] if q["metric"].endswith("sent_bytes_count")
                    and q.get("vm", vm) == vm]
        assert len(selected) == 1
        q = selected[0]
        raw = load(directory / q["response"]["path"])
        assert not raw.get("nextPageToken")
        series, = raw["timeSeries"]
        assert series["resource"] == {"type": "gce_instance", "labels": {
            "project_id": PROJECT, "instance_id": instance, "zone": ZONE}}
        assert series["metricKind"] == "DELTA" and series["valueType"] == "INT64"
        assert series["metric"]["type"] == q["metric"]
        points = sorted(series["points"], key=lambda p: p["interval"]["startTime"])
        total, last, gaps = 0, None, []
        for p in points:
            start, end = p["interval"]["startTime"], p["interval"]["endTime"]
            assert seconds(start, end) > 0
            if last:
                gap = seconds(last, start)
                assert gap >= 0
                if gap > D(".001"):
                    gaps.append({"seconds": str(gap), "unknown_bytes": None})
            value = int(p["value"]["int64Value"])
            assert value >= 0
            total += value
            last = end
        return {"observed_sent_bytes": total, "points": len(points),
                "internal_gaps": gaps, "unobserved_edge_and_gap_bytes": None}

    budget = load(HERE / "budget-before.json")
    early_dir = CLOUD / "usage-early-20260927"
    early = acquisition(early_dir)
    old = load(early_dir / "report.json")
    assert old["acquisition"] == pin(early_dir / "acquisition.json")
    verified(early_dir / "analyze.py", old["analysis_source"])
    transfers = load(CLOUD / "transfers.json")
    rows = []
    for vm, (instance, hold) in SELECTED.items():
        date = "20260925" if vm in ("06", "07") else "20260927"
        name = f"solvers-r1-{date}-{vm}"
        reservation = next(r for r in budget["reservations"] if r["instance"] == name)
        launch = load(CLOUD / f"launch-r1-{date}-{vm}.json")
        creation = load(CLOUD / f"create-result-r1-{date}-{vm}.json")
        created = creation[0] if isinstance(creation, list) else creation
        assert created["id"] == instance and created["name"] == name
        assert launch["argv"][:4] == ["compute", "instances", "create", name]
        assert f"--project={PROJECT}" in launch["argv"] and f"--zone={ZONE}" in launch["argv"]
        assert "--provisioning-model=SPOT" in launch["argv"]
        disks = created["disks"] if isinstance(created["disks"], list) else [created["disks"]]
        disk, = disks
        disk_uri, instance_uri = f"{BASE}/disks/{name}", f"{BASE}/instances/{name}"
        assert disk["source"] == disk_uri and disk["autoDelete"] is True
        size = int(disk["diskSizeGb"])
        assert size == reservation["disk_gib"]
        started = launch["attempted_at"]
        assert seconds(started, created["creationTimestamp"]) >= 0
        known = 0
        high_window = None
        scp = []
        if vm in ("06", "07"):
            assert "--machine-type=e2-highmem-8" in launch["argv"]
            directory = CLOUD / f"cleanup-vm{vm}"
            before = load(directory / "verified-target.json")
            assert before["id"] == instance and before["name"] == name
            attached, = before["disks"]
            assert attached["source"] == disk_uri and attached["autoDelete"] is True
            operations = load(directory / "operations.json")
            operation, = [v for v in operations if v["operationType"] == "delete" and v["targetId"] == instance]
            recon = load(directory / "reconciliation.json")
            absent = recon["verified_at_utc"]
            for kind in ("instances", "disks", "addresses"):
                assert load(directory / f"{kind}-after.json") == []
            traffic = observed(early, early_dir, vm, instance)
            entries = [v for v in transfers["downloads"] if v["reservation_id"] == reservation["id"]]
            known = sum(v["known_unique_download_bytes"] for v in entries)
            assert known == transfers["totals"]["by_reservation_known_unique_bytes"][reservation["id"]]
            network_scope = "All retained inventory bundles including recovery/supplement; retries and incomplete transfers not fully reconstructed"
            disk_identity = "Same instance numeric ID and sole autoDelete disk URI at creation/predelete; disk numeric ID not recorded"
            disk_id = None
            rate = D(".37")
        else:
            directory = CLOUD / f"vm{vm}"
            usage_dir = CLOUD / f"usage-vm{vm}-20260927"
            a = acquisition(usage_dir)
            assert a["instance_id"] == instance
            prior = load(usage_dir / "report.json")
            assert prior["acquisition"] == pin(usage_dir / "acquisition.json")
            verified(usage_dir / "analyze.py", prior["analysis_source"])
            traffic = observed(a, usage_dir, vm, instance)
            assert "--machine-type=e2-standard-2" in launch["argv"]

            def receipt(stem, success=True):
                r = load(directory / (stem + ".result.json"))
                if success:
                    assert r["exit_code"] == 0
                for stream in ("stdout", "stderr"):
                    verified(directory / (stem + f".{stream}.log"), r[stream])
                return r

            stop = receipt("stop-for-resize01")
            up, down = receipt("resize01"), receipt("resize-recovery01")
            command(stop, name, "stop")
            command(up, name, "set-machine-type", "e2-highcpu-32")
            command(down, name, "set-machine-type", "e2-standard-2")
            for stem, machine in (("resize01", "e2-highcpu-32"), ("resize-recovery01", "e2-standard-2")):
                value = load(directory / (stem + ".stdout.log"))
                value = value[0] if isinstance(value, list) else value
                assert value["id"] == instance and value["name"] == name
                assert value["machineType"] == f"{BASE}/machineTypes/{machine}"
                assert value["scheduling"]["terminationTime"] == created["scheduling"]["terminationTime"]
            assert seconds(stop["started_utc"], up["started_utc"]) >= 0
            assert seconds(up["ended_utc"], down["started_utc"]) > 0
            high_window = {"start": stop["started_utc"], "end": down["ended_utc"],
                           "scope": "Starts before resize up; ends after successful resize down; includes STOP time and overlaps small rate"}
            before = load(directory / "cleanup-disk-before01.stdout.log")
            receipt("cleanup-disk-before01")
            assert before["selfLink"] == disk_uri and before["users"] == [instance_uri]
            assert int(before["sizeGb"]) == size and before["type"] == f"{BASE}/diskTypes/pd-balanced"
            disk_id = before["id"]
            recon = load(directory / "reconciliation.json")
            assert recon["instance_id"] == instance and recon["disk_id"] == disk_id
            operation = load(directory / "cleanup-operation01.stdout.log")
            operation = operation[0] if isinstance(operation, list) else operation
            receipt("cleanup-operation01")
            absent = recon["at_utc"]
            for kind in ("instances", "disks", "addresses"):
                r = receipt(f"cleanup-{kind}-after01")
                command(r, name, "list", resource=kind)
                expected = [f"--filter=name={name}"] if kind != "addresses" else (["--filter=name~solvers-r1"] if vm == "15" else [])
                assert [v for v in r["argv"] if v.startswith("--filter")] == expected
                assert load(directory / f"cleanup-{kind}-after01.stdout.log") == []
                assert seconds(r["ended_utc"], absent) >= 0
            download = load(directory / "download-check.json")
            assert download["status"] == "downloaded_archive_stream_hash_verified"
            known = int(download["bytes"])
            assert download["sha256"] == recon["captured_archive_sha256"]
            # Retain all captured download/upload attempt receipts without reading archive bytes.
            for p in sorted(directory.glob("*.result.json")):
                r = read(p)
                if "scp" in r.get("argv", []):
                    stem = p.name.removesuffix(".result.json")
                    receipt(stem, success=False)
                    scp.append({"path": p.relative_to(CLOUD).as_posix(), "exit_code": r["exit_code"]})
            network_scope = "Verified compressed archive plus every captured SCP attempt; Monitoring includes traffic beyond archive; sidecars/SSH/retries not exact total"
            disk_identity = "Creation attached URI + predelete disk numeric ID/URI and sole instance user + same-ID successful delete and raw empty inventory"
            rate = D(".14")
        assert operation["status"] == "DONE" and not operation.get("error")
        assert operation["operationType"] == "delete" and operation["targetId"] == instance
        assert operation["targetLink"] == instance_uri
        assert seconds(operation["endTime"], absent) >= 0
        assert recon["instances"] == recon["disks"] == recon["reserved_addresses"] == []
        lifetime = seconds(started, absent)
        large = seconds(high_window["start"], high_window["end"]) if high_window else None
        assert large is None or (seconds(started, high_window["start"]) >= 0 and seconds(high_window["end"], absent) >= 0)
        other = D(str(reservation["tax_price_delay_and_other_reserve_usd"]))
        assert other == {"06": D("3.2"), "07": D("2.9"), "14": D(1), "15": D(1)}[vm]
        for field, expected in (("disk_usd_gib_hour", ".000137"), ("ipv4_usd_hour", ".0025"), ("reserved_egress_usd_gib", ".3")):
            assert D(str(reservation[field])) == D(expected)
        assert max(known, traffic["observed_sent_bytes"]) < 1024**3
        whole, high, costs = calculate(lifetime, size, other, rate, large)
        total = sum(costs.values())
        held = D(str(reservation["reserved_usd"]))
        assert total < hold <= held
        rows.append({"vm": vm, "reservation_id": reservation["id"], "instance_id": instance,
            "disk_id": disk_id, "disk_uri": disk_uri, "disk_identity_scope": disk_identity,
            "launch_attempt": started, "created": created["creationTimestamp"], "delete_done": operation["endTime"],
            "absence_verified": absent, "lifetime_seconds": str(lifetime), "rounding_slack_seconds": 120,
            "whole_minutes": whole, "overlapping_high_window": high_window, "overlapping_high_minutes": high,
            "network": {**traffic, "known_verified_compressed_download_bytes": known,
                        "allowance_gib": 1, "accounting_scope": network_scope, "captured_scp_attempts": scp,
                        "unobserved_bytes_not_assumed_zero": True, "formal_traffic_bound": False},
            "costs_usd": {k: str(v) for k, v in costs.items()}, "model_total_usd": str(total),
            "held_before_usd": str(held), "proposed_hold_usd": str(hold), "restore_usd": str(held-hold),
            "extra_margin_above_model_usd": str(hold-total)})
    pricing = CLOUD / "preflight-vm14"
    for source in load(pricing / "pricing-sources.json"):
        for ref in source["retained"]:
            verified(pricing / ref["path"], ref)
    old_total = held_total(budget["reservations"])
    restoration = sum(D(r["restore_usd"]) for r in rows)
    assert old_total == D("39.8") and restoration == D("2.30")
    return {"schema": "r1.lifecycle-unused-resource-proposal/v1", "rows": rows,
        "summary": {"held_before_usd": str(old_total), "restore_usd": str(restoration),
            "proposed_total_held_usd": str(old_total-restoration),
            "proposed_available_usd": str(D(40)-old_total+restoration),
            "selected_original_other_reserves_preserved_usd": str(sum(D(r["costs_usd"]["original_other_reserve"]) for r in rows)),
            "initial_four_vm_other_reserves_unchanged_usd": "12.5"},
        "unchanged": "VM02/05 retain original24h disks and2GiB network; all reservations outside06/07/14/15 unchanged",
        "limits": ["Usage-based conservative estimate, not an invoice or guaranteed ceiling; unknown charges remain unknown",
            "Disk lifetime covers create request through later absence, adds120s then rounds up to minutes; stopped time is fully included",
            "VM06/07 disk numeric ID absent; exact attached resource URI and autoDelete on same numeric VM ID plus retained deletion/absence support finite lifetime",
            "VM14/15 whole life uses0.14/h ceiling for2CPU and overlapping32CPU window additionally1.15/h; no Spot discount or stopped-time subtraction",
            "Each selected VM keeps1GiB egress allowance and all original uncertainty; missing/repeated traffic is not known to fit a mathematical cap",
            "Existing official pricing extracts are byte-verified; no fresh API or price acquisition occurs",
            "No original audit report, lifecycle evidence, or current budget is changed"],
        "actual_billed_usd": None, "guaranteed_cost_ceiling_usd": None, "budget_changed": False,
        "source": pin(Path(__file__)), "inputs": dict(sorted(evidence.items()))}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    result = build_report()
    target = HERE / "report.json"
    if args.check:
        assert read(target) == result, "report differs"
    else:
        with target.open("x", encoding="utf-8", newline="\n") as out:
            json.dump(result, out, indent=2)
            out.write("\n")
    print(json.dumps(result["summary"]))
