"""Additive VM17 usage estimate; immutable original report and ledger stay unchanged."""
import argparse
import datetime as dt
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
NAME = "solvers-r1-20260927-17"
INSTANCE = "2775050120395558750"
PROJECT = "solvers-abstraction-20260723"
ZONE = "us-central1-b"
STOP = "2026-09-27T06:38:26Z"
ORIGINAL = "6081ad544311746f5637aff717435b84612225c2ed29166ed3db57061f1f89ec"
ACQUISITION = "189d8fa74ec20ced574d29b44b7973a0902019e81941c55aad6676d368b1cdf1"


def pin(path):
    raw = path.read_bytes()
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"), parse_float=D)


def stamp(value):
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    assert result.utcoffset() is not None, "UTC offset required"
    return result


def seconds(a, b):
    delta = stamp(b) - stamp(a)
    return D(delta.days * 86400 + delta.seconds) + D(delta.microseconds) / 1000000


def command(record, verb, machine=None):
    argv = record["argv"]
    assert record["exit_code"] == 0
    assert argv[1:5] == ["compute", "instances", verb, NAME]
    for key, value in (("project", PROJECT), ("zone", ZONE)):
        assert [a for a in argv if a.startswith("--" + key + "=")] == ["--" + key + "=" + value]
        assert "--" + key not in argv
    types = [a for a in argv if a.startswith("--machine-type")]
    assert types == ([] if machine is None else ["--machine-type=" + machine])
    assert not any(a.startswith("--termination") for a in argv)
    assert stamp(record["started_utc"]) <= stamp(record["ended_utc"])


def identity(value, machine):
    assert value["id"] == INSTANCE
    assert value.get("name", NAME) == NAME
    assert value["machineType"] == f"https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}/machineTypes/{machine}"
    assert value["status"] == "RUNNING"
    assert value["scheduling"]["terminationTime"] == STOP
    assert value["scheduling"]["provisioningModel"] == "SPOT"
    assert value["scheduling"]["automaticRestart"] is False


def calculate(lifetime, large_window, known_transfer, observed_sent):
    assert lifetime > 0 and 0 < large_window <= lifetime
    assert known_transfer >= 0 and observed_sent >= 0
    whole = int(((lifetime + 120) / 60).to_integral_value(rounding=ROUND_CEILING))
    large = int(((large_window + 120) / 60).to_integral_value(rounding=ROUND_CEILING))
    # Do not infer total egress from incomplete Monitoring. Preserve at least 0.5 GiB.
    traffic = max(D(known_transfer), D(observed_sent))
    network = max(D(".5"), (traffic / D(1024**3) * 2).to_integral_value(rounding=ROUND_CEILING) / 2)
    disk_hours = max(D(24), (lifetime / 3600).to_integral_value(rounding=ROUND_CEILING))
    costs = {"whole_lifetime_small_compute": D(whole) / 60 * D(".14"),
             "overlapping_large_window_compute": D(large) / 60 * D("1.15"),
             "whole_lifetime_ipv4": D(whole) / 60 * D(".0025"),
             "40gib_disk_at_least_24h": 40 * disk_hours * D(".000137"),
             "network_allowance": network * D(".30"),
             "original_uncertainty_reserve": D(1)}
    total = sum(costs.values())
    hold = (total * 10).to_integral_value(rounding=ROUND_CEILING) / 10
    return {"whole_minutes": whole, "overlapping_large_minutes": large,
            "network_allowance_gib": str(network), "disk_allowance_hours": str(disk_hours),
            "components_usd": {k: str(v) for k, v in costs.items()},
            "total_usd": str(total), "rounded_hold_usd": str(hold),
            "restore_usd": str(D(2) - hold) if hold < 2 else None,
            "margin_above_model_usd": str(hold - total)}


def build_report():
    evidence = {}

    def load(path):
        evidence[str(path.relative_to(CLOUD)).replace("\\", "/")] = pin(path)
        return read(path)

    original = load(HERE / "report.json")
    acquisition = load(HERE / "acquisition.json")
    assert pin(HERE / "report.json")["sha256"] == ORIGINAL
    assert pin(HERE / "acquisition.json")["sha256"] == ACQUISITION
    assert acquisition["status"] == "completed" and acquisition["instance_id"] == INSTANCE
    assert acquisition["source"] == pin(HERE / "collect.py")
    assert original["analysis_source"] == pin(HERE / "analyze.py")
    for ref in [*acquisition["inputs"].values(), *(q["response"] for q in acquisition["requests"])]:
        path = HERE / ref["path"]
        assert path.resolve().is_relative_to(HERE.resolve())
        assert pin(path) == {key: ref[key] for key in ("bytes", "sha256")}
    assert len(acquisition["requests"]) == 2 and all(q["http_status"] == 200 for q in acquisition["requests"])

    def raw(name):
        if name in acquisition["inputs"]:
            return load(HERE / acquisition["inputs"][name]["path"])
        return load(CLOUD / name)

    def receipt(label, verb, machine=None):
        record = raw(f"vm17/{label}.result.json")
        command(record, verb, machine)
        output = CLOUD / f"vm17/{label}.stdout.log"
        evidence[f"vm17/{label}.stdout.log"] = pin(output)
        assert record["stdout"] == pin(output)
        return record

    launch = raw("launch-r1-20260927-17.json")
    created, = raw("create-result-r1-20260927-17.json")
    assert created["id"] == INSTANCE and created["name"] == NAME
    assert launch["argv"][:4] == ["compute", "instances", "create", NAME]
    assert [a for a in launch["argv"] if a.startswith("--machine-type=")] == ["--machine-type=e2-standard-2"]
    assert launch["termination_time"] == created["scheduling"]["terminationTime"] == STOP
    records = [receipt("stop-resize01", "stop"), receipt("resize01", "set-machine-type", "e2-highcpu-32"),
               receipt("start32-01", "start"), receipt("stop-recovery01", "stop"),
               receipt("resize-recovery01", "set-machine-type", "e2-standard-2"),
               receipt("start-recovery01", "start"), receipt("start-recovery02", "start")]
    for a, b in zip(records, records[1:]):
        assert stamp(a["ended_utc"]) <= stamp(b["started_utc"])
    high = receipt("state32-01", "describe")
    small = receipt("identity-before-delete01", "describe")
    identity(raw("vm17/state32-01.stdout.log"), "e2-highcpu-32")
    identity(raw("vm17/identity-before-delete01.stdout.log"), "e2-standard-2")
    assert stamp(records[2]["ended_utc"]) <= stamp(high["started_utc"]) <= stamp(high["ended_utc"]) <= stamp(records[3]["started_utc"])
    assert stamp(records[5]["ended_utc"]) <= stamp(small["started_utc"]) <= stamp(small["ended_utc"]) <= stamp(records[6]["started_utc"])
    assert stamp(records[6]["ended_utc"]) < stamp(STOP)
    cleanup = raw("vm17/reconciliation.json")
    operation, = raw("vm17/delete-operation01.stdout.log")
    assert cleanup["instance_id"] == operation["targetId"] == INSTANCE
    assert operation == cleanup["delete_operation"] and operation["status"] == "DONE" and not operation.get("error")
    assert operation["operationType"] == "delete"
    assert cleanup["instances"] == cleanup["disks"] == cleanup["reserved_addresses"] == []
    lifetime = seconds(launch["attempted_at"], cleanup["at_utc"])
    assert lifetime == D(original["lifecycle"]["launch_to_absence_seconds"])
    assert stamp(records[6]["ended_utc"]) <= stamp(operation["endTime"]) <= stamp(cleanup["at_utc"])
    large_start, large_end = records[0]["started_utc"], records[4]["ended_utc"]
    large_window = seconds(large_start, large_end)
    assert stamp(launch["attempted_at"]) <= stamp(large_start) < stamp(large_end) <= stamp(cleanup["at_utc"])

    prices = raw("preflight-vm17/cost-proposal.json")["arithmetic"]["prices_usd"]
    assert D(prices["compute-e2-standard-2"]) == D(".06701142") < D(".14")
    assert max(D(prices[k]) for k in ("compute-e2-highcpu-32", "compute-n2-highcpu-32")) <= D("1.15")
    assert D(prices["disk-balanced"]) <= D(".000137")
    assert D(prices["network-spot-ipv4"]) <= D(".0025")
    assert D(prices["network-asia-egress-paid-first-tier"]) <= D(".30")
    fragments = raw("vm17/transfer-interruption-check.json")
    download = raw("vm17/recovery02/download-check.json")
    assert download["status"] == "downloaded_archive_stream_hash_verified"
    assert sum(x["bytes"] for x in download["parts"]) == download["bytes"]
    known = sum(x["bytes"] for x in fragments.values()) + download["bytes"]
    assert known == 237332422
    observed = D(original["metrics"]["sent"]["observed_sum"])
    arithmetic = calculate(lifetime, large_window, known, observed)
    assert arithmetic["whole_minutes"] == original["lifecycle"]["rounded_lifetime_minutes"]
    return {"schema": "r1.vm17-additive-tiered-usage-proposal/v1", "source": pin(Path(__file__)),
            "instance_id": INSTANCE, "inputs": evidence, "original_report_unchanged": True,
            "original_model_total_usd": original["modeled_total_usd"], "original_proposal_available": original["proposal"]["available"],
            "lifecycle": {"launch_attempt": launch["attempted_at"], "absence_verified": cleanup["at_utc"],
                          "whole_seconds": str(lifetime), "large_window_start": large_start, "large_window_end": large_end,
                          "large_window_seconds": str(large_window), "slack_seconds_added_to_each_window": 120,
                          "stopped_periods_subtracted": False, "small_base_subtracted_from_large_window": False},
            "arithmetic": arithmetic, "known_downloaded_archive_and_fragment_bytes": known,
            "monitoring_sent_observed_bytes": str(observed), "unobserved_egress_bytes": None,
            "actual_billed_usd": None, "guaranteed_cost_ceiling_usd": None, "budget_changed": False,
            "proposal_applied": False,
            "limits": ["Full lifetime small-VM base plus a conservatively wider overlapping 32-vCPU interval; stopped periods and both 120-second margins remain charged.",
                       "Successful fixed-target SDK resize receipts and same-ID machine-type readbacks establish the recorded configuration sequence; this is not an exhaustive Cloud Audit Logs inventory.",
                       "The recovery restart changes no machine type; original STOP is unchanged. No invoice or guaranteed bill ceiling is inferred.",
                       "Monitoring 99,729,092 bytes is incomplete and below known downloads. The 0.5-GiB minimum allowance includes room for logs, sidecars, SSH and protocol traffic; missing traffic is not imputed as zero.",
                       "The original one-dollar uncertainty reserve and at least 24 hours of one 40-GiB disk remain. No Spot discount, credit or free tier is used."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = build_report()
    encoded = (json.dumps(report, indent=2) + "\n").encode()
    output = HERE / "tiered-report.json"
    if args.check:
        assert output.read_bytes() == encoded
    else:
        with output.open("xb") as stream:
            stream.write(encoded)
    print(json.dumps(report["arithmetic"], indent=2))


if __name__ == "__main__":
    main()
