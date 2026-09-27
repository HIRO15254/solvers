"""Existing-evidence E2 lifecycle reevaluation for VM16/17 and additive VM18 rates."""
import argparse
from decimal import Decimal as D, ROUND_CEILING
import importlib.util
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
INPUTS = HERE / "inputs"
PROJECT, ZONE = "solvers-abstraction-20260723", "us-central1-b"
IDS = {16: "1127898822003203169", 17: "2775050120395558750", 18: "3769585733775752220"}
BUDGET_SHA = "dd3d8ae625fd9785f42ee9b2ec96bc54f79dbc143ec96e12bce61c1caea6c535"


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# Reuse only already frozen compact parsers/metric arithmetic. No main() call.
base = load_module("frozen_vm18_audit", INPUTS / "usage-audit-vm18/analyze.py")
read, pin, require, seconds = base.read, base.pin, base.require, base.seconds


def one(value):
    if isinstance(value, list):
        require(len(value) == 1, "Exactly one object required")
        return value[0]
    return value


def calculate(whole_seconds, large_seconds, disk_seconds, small_rate, large_rate, known_bytes, observed, held):
    require(whole_seconds > 0 and 0 < large_seconds <= whole_seconds and disk_seconds > 0, "Invalid duration")
    require(small_rate > 0 and large_rate > 0 and known_bytes >= 0 and (observed is None or observed >= 0), "Invalid rates/traffic")
    rounded = lambda value: int(((value + 120) / 60).to_integral_value(rounding=ROUND_CEILING))
    whole, large, disk = map(rounded, (whole_seconds, large_seconds, disk_seconds))
    known = D(known_bytes) if observed is None else max(D(known_bytes), observed)
    network = max(D(".5"), (known / (1024**3) * 2).to_integral_value(rounding=ROUND_CEILING) / 2)
    costs = {"whole_lifetime_e2_standard_2": D(whole) / 60 * small_rate,
             "overlapping_e2_highcpu_32": D(large) / 60 * large_rate,
             "whole_lifetime_ipv4": D(whole) / 60 * D(".0025"),
             "disk_40gib_lifecycle": D(disk) / 60 * 40 * D(".000137"),
             "network_allowance": network * D(".30"), "original_uncertainty_reserve": D(1)}
    total = sum(costs.values())
    proposed = (total * 10).to_integral_value(rounding=ROUND_CEILING) / 10
    return {"whole_minutes": whole, "overlapping_large_minutes": large, "disk_minutes": disk,
            "ordinary_rates_usd_hour": {"e2-standard-2": str(small_rate), "e2-highcpu-32": str(large_rate)},
            "network_allowance_gib": str(network), "components_usd": {k: str(v) for k, v in costs.items()},
            "total_usd": str(total), "held_before_usd": str(held), "proposed_hold_usd": str(proposed),
            "restore_usd": str(held - proposed) if held > proposed else None,
            "margin_above_model_usd": str(proposed - total)}


def verify_scope(row, resource, verb, target=None, query_filter=None, machine=None):
    argv = row["argv"]
    prefix = ["compute", resource, verb] + ([] if target is None else [target])
    require(row["exit_code"] == 0 and argv[1:1 + len(prefix)] == prefix, "Command identity/success differs")
    required = {"project": PROJECT}
    if target is not None:
        required["zone"] = ZONE
    if query_filter is not None:
        required["filter"] = query_filter
    for key, value in required.items():
        require([v for v in argv if v.startswith("--" + key + "=")] == ["--" + key + "=" + value]
                and "--" + key not in argv, "Command scope differs")
    require([v for v in argv if v.startswith("--machine-type")] == ([] if machine is None else ["--machine-type=" + machine]), "Machine configuration differs")
    require(not any(v.startswith("--termination") for v in argv), "Deadline mutation")
    require(seconds(row["started_utc"], row["ended_utc"]) >= 0, "Command time reversed")


def audit(vm, ledger_row):
    folder, name = f"vm{vm}", f"solvers-r1-20260927-{vm}"
    old = INPUTS / f"usage-audit-vm{vm}"
    acquisition = read(old / "acquisition.json")
    original = read(old / "report.json")
    require(acquisition["status"] == "completed" and acquisition["instance_id"] == IDS[vm] and acquisition["project"] == PROJECT,
            "Wrong/incomplete acquisition")
    require(acquisition["source"] == pin(old / "collect.py") and original["analysis_source"] == pin(old / "analyze.py")
            and original["acquisition"] == pin(old / "acquisition.json"), "Original audit chain differs")
    for ref in [*acquisition["inputs"].values(), *(q["response"] for q in acquisition["requests"])]:
        p = (old / ref["path"]).resolve()
        require(p.is_relative_to(old.resolve()) and pin(p) == {k: ref[k] for k in ("bytes", "sha256")}, "Acquisition input changed")
    def path(suffix):
        key = folder + "/" + suffix
        return old / acquisition["inputs"][key]["path"] if key in acquisition["inputs"] else INPUTS / key
    def raw(suffix):
        return read(path(suffix))
    def cmd(label, verb, machine=None, resource="instances"):
        row = raw(label + ".result.json")
        verify_scope(row, resource, verb, name, machine=machine)
        return row
    launch = read(old / "inputs" / f"launch-r1-20260927-{vm}.json")
    created = one(read(old / "inputs" / f"create-result-r1-20260927-{vm}.json"))
    cleanup, deletion = raw("reconciliation.json"), one(raw("delete-operation01.stdout.log"))
    require(created["id"] == cleanup["instance_id"] == deletion["targetId"] == IDS[vm] and created["name"] == name, "Lifecycle ID differs")
    stop = launch["termination_time"]
    require(launch["argv"][:4] == ["compute", "instances", "create", name]
            and [v for v in launch["argv"] if v.startswith("--machine-type=")] == ["--machine-type=e2-standard-2"]
            and created["scheduling"]["terminationTime"] == stop, "Original launch configuration differs")
    require(deletion == cleanup["delete_operation"] and deletion["status"] == "DONE" and deletion["operationType"] == "delete" and not deletion.get("error"), "Deletion not successful")
    expected_vm = f"https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}/instances/{name}"
    require(deletion["targetLink"] == expected_vm and seconds(deletion["endTime"], cleanup["at_utc"]) >= 0, "Deletion target/time differs")
    verify_scope(raw("delete-operation01.result.json"), "operations", "list", query_filter=f"targetId={IDS[vm]} AND operationType=delete")
    for resource, field in (("instances", "instances"), ("disks", "disks"), ("addresses", "reserved_addresses")):
        row = raw(f"absence-{resource}01.result.json")
        verify_scope(row, resource, "list", query_filter="name~solvers-r1" if resource == "addresses" else "name=" + name)
        require(raw(f"absence-{resource}01.stdout.log") == cleanup[field] == [], "Resource remains present")
        require(seconds(deletion["endTime"], row["started_utc"]) >= 0 and seconds(row["ended_utc"], cleanup["at_utc"]) >= 0, "Absence timestamp differs")
    labels = [("stop-resize01", "stop", None), ("resize01", "set-machine-type", "e2-highcpu-32"),
              ("start32-01", "start", None), ("stop-recovery01", "stop", None),
              ("resize-recovery01", "set-machine-type", "e2-standard-2"), ("start-recovery01", "start", None)]
    if vm == 17:
        labels.append(("start-recovery02", "start", None))
    commands = [cmd(*args) for args in labels]
    for a, b in zip(commands, commands[1:]):
        require(seconds(a["ended_utc"], b["started_utc"]) >= 0, "Configuration commands overlap/reordered")
    mutation_names = {p.name for p in (INPUTS / folder).glob("*.result.json") if read(p)["argv"][1:3] == ["compute", "instances"]
                      and read(p)["argv"][3] in ("start", "set-machine-type")}
    require(mutation_names == {label + ".result.json" for label, verb, _ in labels if verb in ("start", "set-machine-type")}, "Extra mutation needs review")
    high = one(raw("start32-01.stdout.log")) if vm == 16 else raw("state32-01.stdout.log")
    small = raw("identity-before-delete01.stdout.log")
    for value, machine in ((high, "e2-highcpu-32"), (small, "e2-standard-2")):
        # VM17's fixed describe format omitted name; its numeric ID and command target bind it.
        require(value["id"] == IDS[vm] and ("name" not in value or value["name"] == name) and value["status"] == "RUNNING"
                and value["machineType"] == f"https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}/machineTypes/{machine}"
                and value["scheduling"]["terminationTime"] == stop, "Same-ID E2 readback differs")
    cmd("identity-before-delete01", "describe")
    if vm == 17:
        cmd("state32-01", "describe")
        collector = load_module("frozen_vm17_collector", old / "collect.py")
        collector.verify_recovery_exception(raw("recovery-exception01.json"), raw("transfer-state01.stdout.log"),
                                            raw("transfer-state01.result.json"), commands[-1])
        require(cleanup["original_stop_utc"] == stop and cleanup["stop_deadline_extended"] is False and cleanup["recovery_starts_total"] == 4, "Recovery exception differs")
    require(seconds(commands[-1]["ended_utc"], deletion["endTime"]) >= 0, "Delete precedes recovery")
    life = seconds(launch["attempted_at"], cleanup["at_utc"])
    large_start, large_end = commands[0]["started_utc"], commands[4]["ended_utc"]
    require(seconds(launch["attempted_at"], large_start) >= 0 and seconds(large_end, cleanup["at_utc"]) >= 0, "Large window outside lifecycle")
    cmd("disk-before-delete01", "describe", resource="disks")
    disk = raw("disk-before-delete01.stdout.log")
    expected_disk = expected_vm.replace("/instances/", "/disks/")
    require(disk["id"] == cleanup["disk_id"] and disk["name"] == name and disk["sizeGb"] == "40"
            and disk["users"] == [expected_vm] and disk["type"].endswith("/diskTypes/pd-balanced"), "Disk identity/type differs")
    before = one(raw("start-recovery01.stdout.log")) if vm == 16 else raw("identity-before-delete02.stdout.log")
    require(before["id"] == IDS[vm], "Predelete disk attachment ID differs")
    for attached in (created["disks"], before["disks"]):
        require(len(attached) == 1 and attached[0]["source"] == expected_disk and attached[0]["autoDelete"] is True
                and attached[0]["diskSizeGb"] == "40", "Disk source/autoDelete differs")
    if vm == 16:
        disk_start, disk_basis = disk["creationTimestamp"], "recorded creationTimestamp"
        require(seconds(launch["attempted_at"], disk_start) >= 0, "Disk predates launch")
    else:
        pre = raw("prelaunch-disks01.result.json")
        verify_scope(pre, "disks", "list", query_filter="name:solvers-r1")
        require(raw("prelaunch-disks01.stdout.log") == [] and seconds(pre["ended_utc"], launch["attempted_at"]) >= 0, "Prelaunch disk absence differs")
        disk_start, disk_basis = pre["started_utc"], "upper envelope from prelaunch empty-disk query start; creationTimestamp unknown"
    metrics = {}
    for query in acquisition["requests"]:
        label = query["label"]
        require(label in ("sent", "uptime") and label not in metrics and query["http_status"] == 200 and query["method"] == "GET", "Unexpected metric query")
        response = read(old / query["response"]["path"])
        require(not response.get("nextPageToken") and len(response.get("timeSeries", [])) <= 1, "Metric incomplete/dimensional change")
        points = []
        for series in response.get("timeSeries", []):
            require(series["resource"] == {"type": "gce_instance", "labels": {"zone": ZONE, "project_id": PROJECT, "instance_id": IDS[vm]}}
                    and series["metric"]["labels"]["instance_name"] == name and series["metric"]["type"] == query["metric"]
                    and series["metricKind"] == "DELTA" and series["valueType"] == ("INT64" if label == "sent" else "DOUBLE"), "Metric identity/schema differs")
            points.extend(series.get("points", []))
        metrics[label] = base.summarize(points, "int64Value" if label == "sent" else "doubleValue")
        require(metrics[label]["observed_sum"] == original["metrics"][label]["observed_sum"], "Original observed total differs")
    require(set(metrics) == {"sent", "uptime"}, "Metric missing")
    downloads = []
    for p in sorted((INPUTS / folder).glob("*.result.json")):
        row = read(p)
        if row["argv"][1:3] == ["compute", "scp"] and any(v.startswith(name + ":") for v in row["argv"][3:-1]):
            remote = [v for v in row["argv"] if v.startswith(name + ":")]
            # Upload has the remote as last nonflag operand; its local source exists before it.
            operands = [v for v in row["argv"][3:] if not v.startswith("--")]
            if not operands or not operands[0].startswith(name + ":"):
                continue
            log = (p.with_name(p.name.removesuffix(".result.json") + ".stdout.log")).read_text()
            values = [int(m.group(1)) for line in log.splitlines() if (m := re.search(r"\|\s*(\d+) kB\s*\|.*\|\s*100%\s*$", line))]
            downloads.append({"receipt": p.name, "exit_code": row["exit_code"], "completed_payload_allowance_bytes": sum((v + 1) * 1024 for v in values)})
    if vm == 16:
        download = raw("download-check.json")
        require(all(d["exit_code"] == 0 for d in downloads) and len(downloads) == 3, "New download outcome needs review")
        require(download["status"] == "downloaded_archive_stream_hash_verified" and download["sha256"] == cleanup["captured_archive_sha256"]
                and download["bytes"] == sum(p["bytes"] for p in download["parts"]), "Verified archive receipt differs")
        traffic = max(download["bytes"], sum(d["completed_payload_allowance_bytes"] for d in downloads))
        failure_policy = "No failed download recorded; successful payload rounding and full0.5GiB allowance retained"
    else:
        final = read(INPUTS / "vm17/recovery02/download-check.json")
        fragments = raw("transfer-interruption-check.json")
        first, = [json.loads(line) for line in path("recovery-state01.stdout.log").read_text().splitlines() if line.startswith("{")]
        require(first["status"] == "original_bytes_verified", "Initial recovery status differs")
        require(final["status"] == "downloaded_archive_stream_hash_verified" and final["sha256"] == cleanup["captured_archive_sha256"]
                and final["bytes"] == sum(p["bytes"] for p in final["parts"]), "Final recovery receipt differs")
        failed = {d["receipt"] for d in downloads if d["exit_code"] != 0}
        require(failed == {"proof-download01.result.json", "proof-download02.result.json", "sidecars-download01.result.json"}, "Unexpected failed/retry set")
        require(first["bytes"] >= sum(p["bytes"] for p in fragments.values()), "Initial transfer fragments exceed archive")
        # Charge the entire interrupted archive, entire requested retry part, and1MiB for rejected sidecars.
        extra = first["bytes"] + final["parts"][0]["bytes"] + 1024**2
        successful = sum(d["completed_payload_allowance_bytes"] for d in downloads if d["exit_code"] == 0)
        traffic = max(successful, final["bytes"]) + extra
        failure_policy = "Interrupted first archive charged in full; failed part00 retry charged as full50,331,648B; rejected sidecars add1MiB; all later successful downloads added. No missing traffic filled with zero."
    price_root = old / "inputs" / f"preflight-vm{vm}"
    rates = []
    for machine in ("e2-standard-2", "e2-highcpu-32"):
        row = (price_root / f"pricing-compute-{machine}-row.html").read_text()
        header = (price_root / f"pricing-compute-{machine}-header.html").read_text()
        region = (price_root / f"pricing-compute-{machine}-region.html").read_text()
        require(machine in row and "Iowa" in region and "Default" in header, "Ordinary price/region column differs")
        rates.append(D(re.search(r"\$([0-9.]+)", row).group(1)))
    require(rates == [D(".06701142"), D(".79152384")], "Acquired E2 ordinary prices differ")
    for key, ceiling in (("disk-balanced", ".000137"), ("network-spot-ipv4", ".0025")):
        require(D(re.search(r"\$([0-9.]+)", (price_root / f"pricing-{key}-row.html").read_text()).group(1)) <= D(ceiling), "Fee exceeds retained ceiling")
    require("$0.12 / 1 gibibyte" in (price_root / "pricing-network-asia-egress-row.html").read_text(), "Egress price differs")
    arithmetic = calculate(life, seconds(large_start, large_end), seconds(disk_start, cleanup["at_utc"]), *rates, traffic,
                           D(metrics["sent"]["observed_sum"]) if metrics["sent"]["available"] else None, D(str(ledger_row["reserved_usd"])))
    return {"vm": vm, "instance_id": IDS[vm], "original_report": pin(old / "report.json"),
            "lifecycle": {"launch": launch["attempted_at"], "delete_done": deletion["endTime"], "absence": cleanup["at_utc"],
                          "whole_seconds": str(life), "large_start": large_start, "large_end": large_end,
                          "large_seconds": str(seconds(large_start, large_end)), "disk_id": disk["id"],
                          "disk_start_bound": disk_start, "disk_start_basis": disk_basis, "disk_creation_recorded": disk.get("creationTimestamp"),
                          "disk_seconds_bound": str(seconds(disk_start, cleanup["at_utc"])), "original_stop": stop},
            "metrics": metrics, "downloads": downloads, "traffic_payload_allowance_bytes": traffic,
            "failure_transfer_policy": failure_policy, "arithmetic": arithmetic,
            "actual_billed_usd": None, "unobserved_egress_bytes": None}


def build_report():
    refs = {}
    for manifest_name, source in (("inputs-manifest.json", "prepare-inputs.py"), ("additional-vm18-inputs.json", "add-vm18.py")):
        manifest = read(HERE / manifest_name)
        require(manifest["source"] == pin(HERE / source), "Snapshot source differs")
        for name, ref in manifest["inputs"].items():
            path = (HERE / ref["path"]).resolve()
            require(path.is_relative_to(INPUTS.resolve()) and pin(path) == {k: ref[k] for k in ("bytes", "sha256")}, "Snapshot input differs")
            require(name not in refs, "Duplicate snapshot key")
            refs[name] = ref
    budget = read(INPUTS / "budget.json")
    require(pin(INPUTS / "budget.json")["sha256"] == BUDGET_SHA, "Current ledger snapshot differs")
    for vm in (16, 17):
        for p in (INPUTS / f"vm{vm}").glob("*.result.json"):
            r = read(p)
            for stream in ("stdout", "stderr"):
                require(r[stream] == pin(p.with_name(p.name.removesuffix(".result.json") + "." + stream + ".log")), "SDK stream differs")
    rows = {}
    for vm, amount in ((16, "1.9"), (17, "1.8"), (18, "1.5")):
        rows[vm], = [r for r in budget["reservations"] if r["id"] == f"r1-20260927-{vm}"]
        require(D(str(rows[vm]["reserved_usd"])) == D(amount) and rows[vm]["billed_usd"] is None and rows[vm]["reservation_released"] is False
                and rows[vm]["tax_price_delay_and_other_reserve_usd"] == 1 and D(str(rows[vm]["maximum_download_gib"])) == D(".5"), "Held amount/reserves differ")
    reports = [audit(vm, rows[vm]) for vm in (16, 17)]
    old18 = read(INPUTS / "usage-audit-vm18/report.json")
    require(base.build_report() == old18, "Frozen VM18 audit replay differs")
    # VM18 source already checks lifecycle, E2 types, receipt pins, ordinary row prices, and all transfers.
    fee = read(INPUTS / "usage-audit-vm18/inputs/preflight-vm18/cost-proposal.json")["arithmetic"]["prices_usd"]
    l = old18["lifecycle"]
    calculation = calculate(D(l["whole_seconds"]), D(l["large_window_seconds"]), D(l["disk_seconds_to_absence"]),
                            D(fee["compute-e2-standard-2"]), D(fee["compute-e2-highcpu-32"]), old18["known_payload_allowance_bytes"],
                            D(old18["metrics"]["sent"]["observed_sum"]), D("1.5"))
    require((calculation["whole_minutes"], calculation["overlapping_large_minutes"], calculation["disk_minutes"]) == (34, 13, 34), "VM18 windows changed")
    reports.append({"vm": 18, "instance_id": IDS[18], "original_report": pin(INPUTS / "usage-audit-vm18/report.json"),
                    "change_scope": "Ordinary E2 compute rates only; frozen original report/reader and all lifecycle windows/reserves unchanged",
                    "lifecycle": l, "arithmetic": calculation, "actual_billed_usd": None, "unobserved_egress_bytes": None})
    restored = sum(D(r["arithmetic"]["restore_usd"]) for r in reports)
    return {"schema": "r1.vm16-vm17-plus-vm18-e2-lifecycle-proposal/v1", "source": pin(Path(__file__)),
            "budget_before": pin(INPUTS / "budget.json"), "input_manifests": [pin(HERE / n) for n in ("inputs-manifest.json", "additional-vm18-inputs.json")],
            "input_files": len(refs), "input_bytes": sum(r["bytes"] for r in refs.values()), "vms": reports,
            "proposal": {"held_before_usd": "39", "restore_usd": str(restored), "held_after_usd": str(D(39) - restored),
                         "available_after_usd": str(D(1) + restored), "applied": False},
            "actual_billed_usd": None, "guaranteed_cost_ceiling_usd": None, "budget_changed": False,
            "limits": ["Every whole, overlapping large, and disk window adds120s before minute ceiling; stopped periods and overlapping small base remain charged.",
                       "Recorded E2-only commands and same-ID configuration readbacks justify ordinary E2 rates, not Spot discounts. SDK evidence is not exhaustive Cloud Audit Logs.",
                       "All original1USD per-VM uncertainty and0.5GiB network allowances remain. Missing metric intervals/edges are unknown and not subtracted.",
                       "VM17 disk creation timestamp is absent; earlier confirmed empty-disk inventory bounds its lifetime conservatively. No creation timestamp is fabricated.",
                       "Existing snapshots/reports and large archives remain untouched. A usage-based reservation proposal, not invoice data or a guaranteed maximum bill."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = build_report()
    data = (json.dumps(report, indent=2) + "\n").encode()
    if args.check:
        require((HERE / "report.json").read_bytes() == data, "Report differs")
    else:
        with (HERE / "report.json").open("xb") as stream:
            stream.write(data)
    print(json.dumps({"vms": [{"vm": r["vm"], "arithmetic": r["arithmetic"]} for r in report["vms"]], "proposal": report["proposal"]}, indent=2))


if __name__ == "__main__":
    main()
