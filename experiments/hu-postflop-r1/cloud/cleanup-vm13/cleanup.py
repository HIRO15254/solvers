"""VM13-only cleanup, default read-only. Caller must quiesce all services first.

Delete requires original --launch-record and every retained attempt as repeated
--evidence EXTRACTED_PROOF ARCHIVE.
Archives need adjacent .manifest.json/.sha256. All extracted bytes and trusted
current-phases terminal verification and archived launch identity must agree.
Never execute retained code.
Never retry an uncertain delete or release reservations. Local proof cannot
establish that the live service is quiescent or that every attempt was supplied.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import importlib.util
import ipaddress
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
PROJECT = "solvers-abstraction-20260723"
ZONE = "us-central1-b"
INSTANCE = "solvers-r1-20260926-13"
BASE = f"https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}"
INSTANCE_LINK = BASE + "/instances/" + INSTANCE
DISK_LINK = BASE + "/disks/" + INSTANCE
GCLOUD = r"C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\bin\gcloud.cmd"
PYTHON = r"C:\Python313\python.exe"
ENV = {"CLOUDSDK_PYTHON": PYTHON, "CLOUDSDK_ENCODING": "utf-8", "PYTHONIOENCODING": "utf-8"}
VERIFY = HERE.parents[1] / "current-phases/check_run.py"
spec = importlib.util.spec_from_file_location("trusted_vm13_recovery", HERE.parent / "bundle-final-proof.py")
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)
require = bundle.require


def now():
    return datetime.now(timezone.utc).isoformat()


def pin(path):
    value, _ = bundle.fingerprint(Path(path))
    return {"path": str(Path(path).resolve()), **value}


def write(path, value):
    with path.open("xb") as stream:
        stream.write(bundle.encode(value))


class Capture:
    def __init__(self, out):
        self.out, self.records = out, []
        self.environment = os.environ.copy()
        self.environment.update(ENV)

    def command(self, label, argv, timeout=90, parse=True, required=True):
        started = now()
        record = {"label": label, "argv": argv, "started_at_utc": started, "timeout_seconds": timeout}
        # Record intent before any action; never retry an uncertain deletion.
        write(self.out / (label + ".command.json"), record)
        try:
            process = subprocess.run(argv, capture_output=True, timeout=timeout, env=self.environment, shell=False)
            stdout, stderr, code = process.stdout, process.stderr, process.returncode
        except subprocess.TimeoutExpired as error:
            stdout, stderr, code = error.stdout or b"", error.stderr or b"", None
            record["error"] = "command timeout; no retry"
        except OSError as error:
            stdout, stderr, code = b"", b"", None
            record["error"] = repr(error)
        for kind, data in (("stdout", stdout), ("stderr", stderr)):
            path = self.out / (label + "." + kind + ".log")
            with path.open("xb") as stream:
                stream.write(data)
            record[kind] = pin(path)
        record.update(exit_code=code, ended_at_utc=now())
        self.records.append(record)
        write(self.out / (label + ".result.json"), record)
        if required:
            require(code == 0, "command failed: " + label)
        if code != 0 or not parse:
            return None
        return bundle.decode(stdout)

    def gcloud(self, label, args, **kwargs):
        output = [] if any(arg.startswith("--format=") for arg in args) else ["--format=json"]
        return self.command(label, [GCLOUD, *args, "--project=" + PROJECT, "--quiet", *output], **kwargs)


def verify_extracted(proof, archive):
    proof = bundle.explicit_path(proof)
    archive = bundle.explicit_path(archive)
    require(proof.is_dir(), "extracted proof directory required")
    report = bundle.check_bundle(archive)
    manifest_path = Path(str(archive) + ".manifest.json")
    manifest_bytes = manifest_path.read_bytes()
    manifest = bundle.decode(manifest_bytes)
    expected = {row["archive_member"]: {k: row[k] for k in ("bytes", "sha256")} for row in manifest["files"]}
    expected["recovery-manifest.json"] = bundle.fingerprint(manifest_path)[0]
    actual = {name: bundle.fingerprint(path)[0] for name, path in bundle.walk(proof)}
    require(actual == expected, "extracted proof exact file set/bytes differs from verified archive")
    return {"directory": str(proof), "archive": pin(archive),
            "sidecars": [pin(manifest_path), pin(Path(str(archive) + ".sha256"))],
            "bundle_check": report}


def validate_launch(path):
    path = bundle.explicit_path(path)
    value = bundle.decode(path.read_bytes())
    require(isinstance(value, list) and len(value) == 1 and isinstance(value[0], dict), "single-instance launch array required")
    row = value[0]
    instance_id = str(row.get("id", ""))
    require(row.get("name") == INSTANCE and instance_id.isdigit() and int(instance_id) > 0, "VM13 launch name/id mismatch")
    for key, expected in (("selfLink", INSTANCE_LINK), ("zone", BASE)):
        require(key not in row or row[key] == expected, "VM13 launch project/zone mismatch")
    require(isinstance(row.get("disks"), list) and len(row["disks"]) == 1 and isinstance(row["disks"][0], dict)
            and row["disks"][0].get("source") == DISK_LINK, "VM13 launch boot disk project/zone/name mismatch")
    return instance_id


def verify_evidence(capture, proof, archive, index, instance_id):
    retained = verify_extracted(proof, archive)
    require(retained["bundle_check"]["retention_issue_count"] == 0, "original retained bytes incomplete")
    verification = capture.command(f"proof-{index}-verification", [PYTHON, "-B", str(VERIFY),
                                    "--out", retained["directory"]], timeout=180)
    require(verification.get("schema") == "r1.current-phases-verification/v1"
            and verification.get("status") in ("completed", "failed")
            and verification.get("payload_integrity") == "verified", "proof is not trusted terminal evidence")
    missing = retained["bundle_check"]["missing_required_files"]
    early = (verification["status"] == "failed" and verification.get("scope") == "failed_prepare"
             and verification.get("provenance_complete") is False)
    require(missing == ["build.json"] or (early and missing == ["plan.json", "build.json"]),
            "unexpected original evidence missing; build.json is the only legacy-required artifact")
    # The bundle's external metadata alias may be deduplicated into payload.
    proof = Path(retained["directory"])
    manifest = bundle.decode(Path(str(archive) + ".manifest.json").read_bytes())
    launch_rows = [row for row in manifest["files"] if row.get("root") == "metadata"
                   and row.get("relative_path") == "current-phase-launch01.json"]
    require(len(launch_rows) == 1, "original VM launch metadata missing or ambiguous")
    launch_path = proof / bundle.safe_member(launch_rows[0]["archive_member"])
    launch = bundle.decode(launch_path.read_bytes())
    require(launch.get("schema") == "r1.current-phases-launch/v1"
            and str(launch.get("instance_id")) == instance_id and launch.get("boot_id")
            and launch.get("unit") == "solvers-r1-vm13-current-phases", "proof VM launch identity differs")
    if (proof / "plan.json").exists():
        plan = bundle.decode((proof / "plan.json").read_bytes())
        require(plan["launch"] == launch, "retained campaign and raw launch metadata disagree")
    return {**retained, "campaign_status": verification["status"],
            "provenance_complete": verification.get("provenance_complete"),
            "archived_launch": pin(launch_path), "boot_id": launch["boot_id"], "instance_id": instance_id}


def validate_instance(instance, instance_id):
    require(instance["name"] == INSTANCE and str(instance["id"]) == instance_id
            and instance["selfLink"] == INSTANCE_LINK and instance["zone"] == BASE, "VM13 identity mismatch")
    disks = instance["disks"]
    require(len(disks) == 1, "VM13 must have exactly one attached disk")
    disk = disks[0]
    require(disk.get("boot") is True and disk.get("autoDelete") is True and disk["source"] == DISK_LINK
            and str(disk["diskSizeGb"]) == "40" and disk.get("type") == "PERSISTENT", "unexpected boot disk; deletion refused")
    return sorted({str(ipaddress.ip_address(item["natIP"]))
                   for interface in instance.get("networkInterfaces", [])
                   for item in interface.get("accessConfigs", []) if item.get("natIP")})


def validate_disk(disk):
    require(disk["name"] == INSTANCE and disk["selfLink"] == DISK_LINK and disk["zone"] == BASE
            and str(disk["sizeGb"]) == "40" and disk["users"] == [INSTANCE_LINK]
            and str(disk["id"]).isdigit(), "disk identity, size or sole owner mismatch")


def execute(args):
    out = Path(args.out).absolute()
    require(args.mode != "delete" or args.evidence, "delete requires explicitly provided verified local evidence")
    launch = bundle.explicit_path(args.launch_record)
    instance_id = validate_launch(launch)
    for proof, archive in args.evidence:
        require(not out.resolve().is_relative_to(Path(proof).resolve()) and out.resolve() != Path(archive).resolve(), "receipt output overlaps proof")
    require(out.resolve() != launch, "receipt output overlaps launch record")
    out.mkdir(parents=True, exist_ok=False)
    bundle.explicit_path(out)
    capture = Capture(out)
    original_launch = pin(launch)
    retained_launch = out / "launch-record.json"
    retained_launch.write_bytes(launch.read_bytes())
    require({k: v for k, v in pin(retained_launch).items() if k != "path"} ==
            {k: v for k, v in original_launch.items() if k != "path"}, "launch changed while copying")
    require(validate_launch(retained_launch) == instance_id, "launch identity changed while copying")
    receipt = {"schema": "r1.vm13-cleanup/v1", "mode": args.mode, "project": PROJECT, "zone": ZONE,
               "instance": INSTANCE, "instance_id": instance_id, "started_at_utc": now(), "environment_overrides": ENV,
               "launch_record": {"original": original_launch, "retained": pin(retained_launch)},
               "billing_usd": None, "reservation_released": False,
               "scope": "Only VM13 and its autoDelete boot disk; resource absence is not a billing assertion",
               "trusted_code": [pin(Path(__file__)), pin(VERIFY), pin(VERIFY.with_name("protocol.json")),
                                pin(VERIFY.with_name("source-pins.json")), pin(VERIFY.with_name("runner.py")),
                                pin(VERIFY.with_name("validate.py")), pin(HERE.parent / "bundle-final-proof.py"),
                                pin(HERE.parents[1] / "showdown-kernel/run.py"), pin(HERE.parents[1] / "exact-mass/run.py")]}
    try:
        receipt["proofs"] = [verify_evidence(capture, proof, archive, i, instance_id)
                             for i, (proof, archive) in enumerate(args.evidence)]
        instance = capture.gcloud("instance-before", ["compute", "instances", "describe", INSTANCE, "--zone=" + ZONE,
            "--format=json(id,name,selfLink,zone,status,disks,networkInterfaces)"])
        addresses = validate_instance(instance, instance_id)
        disk = capture.gcloud("disk-before", ["compute", "disks", "describe", INSTANCE, "--zone=" + ZONE,
            "--format=json(id,name,selfLink,zone,sizeGb,status,users)"])
        validate_disk(disk)
        receipt.update(disk_id=str(disk["id"]), observed_external_addresses=addresses)
        if args.mode == "inspect":
            receipt["status"] = "read_only_identity_verified"
        else:
            # One named instance only, with no disk-policy override or retry.
            capture.gcloud("delete", ["compute", "instances", "delete", INSTANCE, "--zone=" + ZONE], timeout=180, parse=False, required=False)
            results = {}
            reads = {
                "instances-after": ["compute", "instances", "list", "--zones=" + ZONE, "--filter=name=" + INSTANCE,
                                    "--format=json(id,name,selfLink,zone,status)"],
                "disks-after": ["compute", "disks", "list", "--zones=" + ZONE, "--filter=name=" + INSTANCE,
                                "--format=json(id,name,selfLink,zone,sizeGb,status,users)"],
                "addresses-after": ["compute", "addresses", "list", "--filter=" + " OR ".join(["name=" + INSTANCE, *["address=" + ip for ip in addresses]]),
                                    "--format=json(id,name,address,addressType,status,region,users)"],
                "delete-operations-after": ["compute", "operations", "list", "--filter=operationType=delete AND targetId=" + instance_id,
                                          "--format=json(id,name,operationType,targetId,targetLink,zone,status,error,startTime,endTime)"],
            }
            for label, argv in reads.items():
                try:
                    results[label] = capture.gcloud(label, argv, required=False)
                except (ValueError, KeyError) as error:
                    results[label] = {"read_error": repr(error)}
            receipt["readbacks"] = results
            operations = results["delete-operations-after"]
            matching = [row for row in operations if isinstance(row, dict) and row.get("operationType") == "delete"
                        and str(row.get("targetId")) == instance_id and row.get("targetLink") == INSTANCE_LINK
                        and row.get("zone") == BASE and row.get("status") == "DONE" and not row.get("error")] if isinstance(operations, list) else []
            receipt["matching_done_delete_operations"] = matching
            absent = all(results[label] == [] for label in ("instances-after", "disks-after", "addresses-after"))
            receipt["status"] = "deleted_and_absence_verified" if absent and matching else "deletion_not_fully_reconciled"
    except BaseException as error:
        receipt.update(status="failed", error=repr(error))
        raise
    finally:
        receipt.update(ended_at_utc=now(), commands=capture.records)
        write(out / "reconciliation.json", receipt)
    print(json.dumps({"status": receipt["status"], "receipt": str(out / "reconciliation.json")}))
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("inspect", "delete"), default="inspect")
    parser.add_argument("--out", type=Path, required=True, help="new local receipt directory")
    parser.add_argument("--evidence", action="append", nargs=2, default=[], metavar=("PROOF_DIR", "ARCHIVE"))
    parser.add_argument("--launch-record", type=Path, required=True, help="preserved original gcloud create JSON single-instance array")
    args = parser.parse_args()
    try:
        result = execute(args)
        if result["status"] == "deletion_not_fully_reconciled":
            parser.exit(2, "Deletion requires read-only reconciliation; do not retry deletion automatically.\n")
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(2, f"VM13 cleanup stopped: {error}\n")


if __name__ == "__main__":
    main()
