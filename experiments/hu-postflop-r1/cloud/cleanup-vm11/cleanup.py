"""VM11-only cleanup, default read-only. Never run while evidence writers exist.

Delete mode requires --evidence EXTRACTED_PROOF ARCHIVE for each retained attempt.
The archive, .manifest.json and .sha256 must be local; the extracted bytes and a
terminal completed/failed trusted final-pipeline verification must all agree.
Optional --focused-evidence EXTRACTED ARCHIVE REFERENCE_PROOF additionally checks
focused-memory evidence against a completed reference among those final proofs.
No retained code is executed. No budget/reservation or other resource is changed.
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
INSTANCE = "solvers-r1-20260926-11"
INSTANCE_ID = "570856080701499920"
BASE = f"https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}"
INSTANCE_LINK = BASE + "/instances/" + INSTANCE
DISK_LINK = BASE + "/disks/" + INSTANCE
GCLOUD = r"C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\bin\gcloud.cmd"
PYTHON = r"C:\Python313\python.exe"
ENV = {"CLOUDSDK_PYTHON": PYTHON, "CLOUDSDK_ENCODING": "utf-8", "PYTHONIOENCODING": "utf-8"}
VERIFY = HERE.parents[1] / "final-pipeline/verify.py"
FOCUSED = HERE.parents[1] / "focused-memory/run.py"
spec = importlib.util.spec_from_file_location("trusted_vm11_recovery", HERE.parent / "bundle-final-proof.py")
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


def verify_evidence(capture, proof, archive, index):
    retained = verify_extracted(proof, archive)
    proof = Path(retained["directory"])
    verification = capture.command(f"proof-{index}-verification", [PYTHON, "-B", str(VERIFY), "--out", str(proof)], timeout=180)
    require(verification.get("schema") == "r1.final-pipeline-verification/v1"
            and verification.get("status") in ("completed", "failed")
            and verification.get("payload_integrity") == "verified", "proof is not trusted terminal evidence")
    return {**retained, "campaign_status": verification["status"]}


def verify_focused_evidence(capture, proof, archive, reference, index, final_proofs):
    reference = bundle.explicit_path(reference)
    require(any(Path(row["directory"]) == reference and row["campaign_status"] == "completed"
                for row in final_proofs), "focused reference must be completed, validated final evidence")
    retained = verify_extracted(proof, archive)
    verification = capture.command(f"focused-proof-{index}-verification",
                                   [PYTHON, "-B", str(FOCUSED), "--phase", "check", "--out", retained["directory"],
                                    "--reference-proof", str(reference)], timeout=180)
    require(verification.get("schema") == "r1.focused-memory-verification/v1"
            and verification.get("status") in ("completed", "failed")
            and verification.get("payload_integrity") == "verified", "focused proof is not trusted terminal evidence")
    return {**retained, "reference_directory": str(reference), "campaign_status": verification["status"]}


def validate_instance(instance):
    require(instance["name"] == INSTANCE and str(instance["id"]) == INSTANCE_ID
            and instance["selfLink"] == INSTANCE_LINK and instance["zone"] == BASE, "VM11 identity mismatch")
    disks = instance["disks"]
    require(len(disks) == 1, "VM11 must have exactly one attached disk")
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
    focused = getattr(args, "focused_evidence", [])
    require(args.mode != "delete" or args.evidence, "delete requires explicitly provided verified local evidence")
    for proof, archive in args.evidence:
        require(not out.resolve().is_relative_to(Path(proof).resolve()) and out.resolve() != Path(archive).resolve(), "receipt output overlaps proof")
    for proof, archive, reference in focused:
        require(not out.resolve().is_relative_to(Path(proof).resolve()) and out.resolve() != Path(archive).resolve()
                and not out.resolve().is_relative_to(Path(reference).resolve()), "receipt output overlaps focused proof/reference")
    out.mkdir(parents=True, exist_ok=False)
    bundle.explicit_path(out)
    capture = Capture(out)
    receipt = {"schema": "r1.vm11-cleanup/v1", "mode": args.mode, "project": PROJECT, "zone": ZONE,
               "instance": INSTANCE, "instance_id": INSTANCE_ID, "started_at_utc": now(), "environment_overrides": ENV,
               "billing_usd": None, "reservation_released": False,
               "scope": "Only VM11 and its autoDelete boot disk; resource absence is not a billing assertion",
               "trusted_code": [pin(Path(__file__)), pin(VERIFY), pin(VERIFY.with_name("run.py")),
                                pin(HERE.parent / "bundle-final-proof.py"),
                                pin(HERE.parents[1] / "showdown-kernel/run.py"), pin(HERE.parents[1] / "exact-mass/run.py")]}
    try:
        receipt["proofs"] = [verify_evidence(capture, proof, archive, i) for i, (proof, archive) in enumerate(args.evidence)]
        if focused:
            receipt["trusted_code"] += [pin(FOCUSED.with_name(name)) for name in ("run.py", "protocol.json", "native_rss.c", "calibration.py")]
            receipt["focused_proofs"] = [verify_focused_evidence(capture, proof, archive, reference, i, receipt["proofs"])
                                         for i, (proof, archive, reference) in enumerate(focused)]
        instance = capture.gcloud("instance-before", ["compute", "instances", "describe", INSTANCE, "--zone=" + ZONE,
            "--format=json(id,name,selfLink,zone,status,disks,networkInterfaces)"])
        addresses = validate_instance(instance)
        disk = capture.gcloud("disk-before", ["compute", "disks", "describe", INSTANCE, "--zone=" + ZONE,
            "--format=json(id,name,selfLink,zone,sizeGb,status,users)"])
        validate_disk(disk)
        receipt.update(disk_id=str(disk["id"]), observed_external_addresses=addresses)
        if args.mode == "inspect":
            receipt["status"] = "read_only_identity_verified"
        else:
            # No --delete-disks/--keep-disks override: retain the verified autoDelete policy.
            capture.gcloud("delete", ["compute", "instances", "delete", INSTANCE, "--zone=" + ZONE], timeout=180, parse=False, required=False)
            results = {}
            reads = {
                "instances-after": ["compute", "instances", "list", "--zones=" + ZONE, "--filter=name=" + INSTANCE],
                "disks-after": ["compute", "disks", "list", "--zones=" + ZONE, "--filter=name=" + INSTANCE],
                "addresses-after": ["compute", "addresses", "list", "--filter=" + " OR ".join(["name=" + INSTANCE, *["address=" + ip for ip in addresses]])],
                "delete-operations-after": ["compute", "operations", "list", "--filter=operationType=delete AND targetId=" + INSTANCE_ID],
            }
            # Keep every readback even if another one fails. No second delete.
            for label, argv in reads.items():
                try:
                    results[label] = capture.gcloud(label, argv, required=False)
                except (ValueError, KeyError) as error:
                    results[label] = {"read_error": repr(error)}
            receipt["readbacks"] = results
            operations = results["delete-operations-after"]
            matching = [row for row in operations if isinstance(row, dict) and row.get("operationType") == "delete"
                        and str(row.get("targetId")) == INSTANCE_ID and row.get("targetLink") == INSTANCE_LINK
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
    parser.add_argument("--focused-evidence", action="append", nargs=3, default=[],
                        metavar=("PROOF_DIR", "ARCHIVE", "REFERENCE_PROOF"))
    args = parser.parse_args()
    try:
        result = execute(args)
        if result["status"] == "deletion_not_fully_reconciled":
            parser.exit(2, "Deletion requires read-only reconciliation; do not retry deletion automatically.\n")
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(2, f"VM11 cleanup stopped: {error}\n")


if __name__ == "__main__":
    main()
