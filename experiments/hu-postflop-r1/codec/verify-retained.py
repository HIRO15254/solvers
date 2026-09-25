#!/usr/bin/env python3
"""Verify a relocated codec campaign from exact collector bundle bytes.

No retained executable is run. Original path strings and records are immutable.
Python 3.11+ stdlib; see retained-usage.md. Missing runtime/artifact bytes fail.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import io
import json
from pathlib import Path, PurePosixPath
import re
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
RUNNER_SHA256 = "6c8ca8e49f6c20320d4140a8b1322e0c62b3d37de1b26968e4270970d6eba79f"
MAX_MEMBER = 2 * 1024**3
MAX_TOTAL = 8 * 1024**3
MAX_JSON = 32 * 1024**2
BLOCK = 1024**2
HEX = re.compile(r"[0-9a-f]{64}\Z")


def require(ok, message):
    if not ok:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def document(raw):
    require(len(raw) <= MAX_JSON and b"\0" not in raw, "oversized/NUL JSON evidence")
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key: " + key)
            result[key] = value
        return result
    def invalid(value):
        raise ValueError("nonfinite JSON number: " + value)
    value = json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid)
    # JSON exponents can overflow float without invoking parse_constant.
    json.dumps(value, allow_nan=False)
    return value


def posix(value):
    require(isinstance(value, str) and value.startswith("/") and not value.startswith("//")
            and "\\" not in value and "\0" not in value, "invalid original absolute path")
    require(str(PurePosixPath(value)) == value and ".." not in PurePosixPath(value).parts,
            "noncanonical original absolute path")
    return value


def fileref(value):
    require(isinstance(value, dict) and set(value) == {"path", "bytes", "sha256"}, "invalid FileRef")
    posix(value["path"])
    require(type(value["bytes"]) is int and value["bytes"] >= 0
            and isinstance(value["sha256"], str) and HEX.fullmatch(value["sha256"]), "invalid FileRef identity")
    return value["path"], value["bytes"], value["sha256"]


def local_identity(path):
    path = Path(path).resolve(strict=True)
    require(path.is_file(), "missing local evidence file")
    before = path.stat()
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(BLOCK), b""):
            digest.update(block)
    after = path.stat()
    require((before.st_size, before.st_mtime_ns, before.st_ctime_ns) ==
            (after.st_size, after.st_mtime_ns, after.st_ctime_ns), "local evidence changed while hashing")
    return {"path": str(path), "bytes": after.st_size, "sha256": digest.hexdigest()}


class VMPath(PurePosixPath):
    """Only lexical original-path operations needed by the frozen validator."""
    def resolve(self, strict=False):
        posix(str(self))
        return self


class Evidence:
    def __init__(self):
        # One anonymous local spool bounds RAM and is removed on close. Nothing is
        # extracted using an archive member's name or an original VM path.
        self.spool = tempfile.TemporaryFile(mode="w+b")
        self.entries = {}
        self.paths = {}
        self.bundles = []
        self.total_bytes = 0
        self.links = set()

    def close(self):
        self.spool.close()

    def blocks(self, key):
        offset = self.entries[key]["offset"]
        remaining = key[1]
        while remaining:
            self.spool.seek(offset)
            raw = self.spool.read(min(BLOCK, remaining))
            require(bool(raw), "truncated local evidence spool")
            yield raw
            remaining -= len(raw)
            offset += len(raw)

    def add_bundle(self, label, path, expected):
        require(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", label), "invalid bundle label")
        require(label not in {x["label"] for x in self.bundles}, "duplicate bundle label")
        require(isinstance(expected, str) and HEX.fullmatch(expected), "invalid pinned archive SHA256")
        archive_id = local_identity(path)
        require(archive_id["sha256"] == expected, "archive SHA256 differs")
        path = Path(path)
        side_path, check_path = Path(str(path) + ".manifest.json"), Path(str(path) + ".sha256")
        side_id, check_id = local_identity(side_path), local_identity(check_path)
        side_raw, check_raw = side_path.read_bytes(), check_path.read_bytes()
        require(sha(side_raw) == side_id["sha256"] and sha(check_raw) == check_id["sha256"], "sidecar changed")
        manifest = document(side_raw)
        require(manifest["schema"] == "solvers.r1-retention/v1", "wrong collector schema")
        require(check_raw.decode("utf-8").strip() == expected + "  " + manifest["archive_filename"],
                "checksum sidecar/name differs")
        rows, original_paths = {}, set()
        skipped = 0
        for row in manifest["files"]:
            posix(row["original_path"])
            require(row["original_path"] not in original_paths, "duplicate original path in bundle")
            original_paths.add(row["original_path"])
            fileref({"path": row["original_path"], "bytes": row["bytes"], "sha256": row["sha256"]})
            require(type(row["included"]) is bool, "invalid included flag")
            if not row["included"]:
                skipped += 1
                continue
            member = row["archive_member"]
            require(row["kind"] == "regular" and isinstance(member, str)
                    and re.fullmatch(r"files/[0-9]{8}", member) and member not in rows, "invalid/duplicate member")
            rows[member] = row
        seen = set()
        with tarfile.open(path, "r:gz") as archive:
            for member in archive:
                require(member.isfile() and member.name not in seen and 0 <= member.size <= MAX_MEMBER,
                        "unsafe/duplicate/oversized tar member")
                seen.add(member.name)
                self.total_bytes += member.size
                require(self.total_bytes <= MAX_TOTAL, "evidence exceeds total decoded bound")
                stream = archive.extractfile(member)
                require(stream is not None, "unreadable tar member")
                if member.name == "retention-manifest.json":
                    require(member.size == len(side_raw) and stream.read() == side_raw, "embedded manifest differs")
                    continue
                require(member.name in rows, "unlisted tar member")
                row = rows[member.name]
                require(member.size == row["bytes"], "member length differs")
                self.spool.seek(0, 2)
                offset = self.spool.tell()
                digest, size = hashlib.sha256(), 0
                for block in iter(lambda: stream.read(BLOCK), b""):
                    digest.update(block)
                    size += len(block)
                    self.spool.write(block)
                require(size == row["bytes"] and digest.hexdigest() == row["sha256"], "member SHA/size differs")
                key = (row["original_path"], size, digest.hexdigest())
                location = {"bundle": label, "archive_member": member.name}
                if key in self.entries:
                    previous = b"".join(self.blocks(key)) if size <= BLOCK else None
                    if previous is not None:
                        self.spool.seek(offset)
                        require(previous == self.spool.read(size), "duplicate bytes differ")
                    self.entries[key]["locations"].append(location)
                else:
                    self.entries[key] = {"offset": offset, "locations": [location]}
                    self.paths.setdefault(key[0], []).append(key)
        require(seen == set(rows) | {"retention-manifest.json"}, "missing tar member")
        require(local_identity(path) == archive_id, "archive changed while reading")
        self.bundles.append({"label": label, "archive": archive_id, "manifest": side_id,
                             "checksum": check_id, "verified_payloads": len(rows), "skipped_inventory_rows": skipped})

    def key(self, value):
        if isinstance(value, dict):
            key = fileref(value)
            require(key in self.entries, "required retained identity missing: " + repr(key))
        else:
            path = posix(str(value))
            choices = self.paths.get(path, [])
            require(len(choices) == 1, "missing/ambiguous original path: " + path)
            key = choices[0]
        self.links.add(key)
        return key

    def identity(self, value):
        path, size, digest = self.key(value)
        return {"path": path, "bytes": size, "sha256": digest}

    def raw(self, value, limit=MAX_JSON):
        key = self.key(value)
        require(key[1] <= limit, "requested document exceeds size bound")
        return b"".join(self.blocks(key))

    def read(self, value):
        return document(self.raw(value))

    def same_bytes(self, left, right):
        a, b = self.key(left), self.key(right)
        require(a[1] == b[1], "retained byte lengths differ")
        for aa, bb in zip(self.blocks(a), self.blocks(b), strict=True):
            require(aa == bb, "retained bytes differ")

    def missing_refs(self):
        """Inventory-only preflight; never a successful campaign verification."""
        found = {}
        def walk(value, origin):
            if isinstance(value, dict):
                if set(value) == {"path", "bytes", "sha256"}:
                    key = fileref(value)
                    if key not in self.entries:
                        found.setdefault(key, []).append(origin)
                for child in value.values():
                    walk(child, origin)
            elif isinstance(value, list):
                for child in value:
                    walk(child, origin)
        for key in self.entries:
            if key[0].endswith(("/supervisor.json", "/result.json", "/plan.json", "/attestation.json")):
                walk(self.read({"path": key[0], "bytes": key[1], "sha256": key[2]}), key[0])
        return [{"path": key[0], "bytes": key[1], "sha256": key[2], "referenced_by": sorted(set(origins))}
                for key, origins in sorted(found.items())]


def frozen_runner(evidence):
    path = HERE / "run_codec.py"
    require(local_identity(path)["sha256"] == RUNNER_SHA256, "local frozen runner changed")
    spec = importlib.util.spec_from_file_location("retained_codec_frozen", path)
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    runner.Path = VMPath
    runner.read = evidence.read
    runner.identity = evidence.identity
    runner.same_bytes = evidence.same_bytes
    return runner


def source_archives(evidence, plan):
    """Bind source manifests to tar bytes, allowing only baseline example copy."""
    validation = evidence.read(plan["build"]["validation_file"])
    result = []
    for role, key, index in (("baseline", "baseline", 7), ("candidate", "current", 6)):
        root = PurePosixPath(validation["stages"][index]["cwd"])
        archived = {}
        raw = evidence.raw(plan["build"]["sides"][role]["source"])
        with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
            names = set()
            for member in archive:
                name = PurePosixPath(member.name)
                require(not name.is_absolute() and ".." not in name.parts and "\\" not in member.name
                        and member.name not in names, "unsafe/duplicate source archive member")
                names.add(member.name)
                if member.isdir():
                    continue
                require(member.isfile() and member.size <= MAX_JSON, "invalid source archive member")
                stream = archive.extractfile(member)
                require(stream is not None, "unreadable source archive member")
                data = stream.read()
                require(len(data) == member.size, "truncated source member")
                archived[name.as_posix()] = (len(data), sha(data))
        listed = {}
        for ref in validation["identities"][key]:
            name = PurePosixPath(ref["path"]).relative_to(root).as_posix()
            require(name not in listed, "duplicate source manifest entry")
            listed[name] = (ref["bytes"], ref["sha256"])
        if role == "baseline":
            example = "crates/formats/examples/sol_codec_bench.rs"
            require(example not in archived, "baseline unexpectedly already contains experiment example")
            archived[example] = (plan["example"]["bytes"], plan["example"]["sha256"])
        require(archived == listed, "source archive/file manifest differs: " + role)
        result.append({"role": role, "archive": plan["build"]["sides"][role]["source"],
                       "manifest_files": len(listed), "allowed_extra": "example only" if role == "baseline" else None})
    return result


def instant(value):
    require(isinstance(value, str), "timestamp must be text")
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(result.tzinfo is not None, "timestamp needs timezone")
    return result


def chronology(evidence, plan, state, runner):
    validation = evidence.read(plan["build"]["validation_file"])
    require(instant(validation["ended_utc"]) <= instant(plan["issued_at"]) <= instant(state["started_at"]),
            "build/plan/campaign chronology differs")
    expected = [(case, operation, repetition, side) for case in runner.CASES for operation in runner.OPERATIONS
                for repetition in range(4) for side in
                (("candidate", "baseline") if repetition == 2 else ("baseline", "candidate"))]
    actual = [(r["case"], r["operation"], r["repetition"], r["side"]) for r in state["samples"]]
    require(actual == expected, "sample execution order differs from frozen protocol")
    previous = instant(state["started_at"])
    for row in state["samples"]:
        record = evidence.read(row["record"])
        require(previous <= instant(record["created_at"]) <= instant(record["started_at"])
                <= instant(record["ended_at"]), "sample overlap or timestamp order differs")
        previous = instant(record["ended_at"])
        require(record["stop_reason"] == "completed" and record["errors"] == [] and record["shell"] is False,
                "sample supervisor did not finish normally")
        for key in ("timeout_seconds", "memory_limit_bytes", "min_free_memory_bytes", "disk_reserve_bytes"):
            require(record["limits"][key] == plan["limits"][key], "sample resource limit differs from plan")
    require(previous <= instant(state["ended_at"]) <= dt.datetime.now(dt.timezone.utc), "campaign completion timestamp differs")


def independent_ratios(state, comparison):
    rows = []
    for result in comparison["rows"]:
        case, operation = result["case"], result["operation"]
        pairs = []
        for repetition in (1, 2, 3):
            pair = []
            for side in ("baseline", "candidate"):
                selected = [r for r in state["samples"] if
                            (r["case"], r["operation"], r["repetition"], r["side"]) == (case, operation, repetition, side)]
                require(len(selected) == 1, "independent ratio sample missing")
                timing = selected[0]["timing"]
                pair.append(timing["operation_seconds"] + (timing["open_seconds"] if operation == "read-root" else 0))
            pairs.append(pair)
        baseline, candidate = (sorted(p[i] for p in pairs)[1] for i in (0, 1))
        ratio, faster = candidate / baseline, sum(b < a for a, b in pairs)
        require(result["raw_pairs_seconds"] == pairs and result["baseline_median_seconds"] == baseline
                and result["candidate_median_seconds"] == candidate and result["candidate_over_baseline"] == ratio
                and result["paired_faster_count"] == faster
                and result["improvement_gate_met"] == (ratio <= 0.95 and faster >= 2), "independent ratio differs")
        rows.append(result)
    require(len(rows) == 12, "expected twelve ratios")
    return rows


def verify_campaign(evidence, run_root):
    posix(run_root)
    runner = frozen_runner(evidence)
    state = evidence.read(run_root + "/result.json")
    require(state["run_root"] == run_root, "campaign root mismatch")
    plan = evidence.read(state["plan"])
    require(plan["runner"]["sha256"] == RUNNER_SHA256, "campaign used a different runner")
    runner.verify_plan(plan, live_host=False)
    require(state["plan_sha256"] == plan["plan_sha256"], "campaign plan digest mismatch")
    comparison = runner.analyze(state, plan)
    require(comparison == evidence.read(run_root + "/comparison.json"), "retained comparison differs")
    chronology(evidence, plan, state, runner)
    sources = source_archives(evidence, plan)
    rows = independent_ratios(state, comparison)
    links = [{"path": key[0], "bytes": key[1], "sha256": key[2],
              "locations": evidence.entries[key]["locations"]} for key in sorted(evidence.links)]
    return {"schema": "r1.codec-retained-verification/v1", "status": "passed", "verified_at": dt.datetime.now(dt.timezone.utc).isoformat(),
            "verifier": local_identity(Path(__file__)), "frozen_runner_sha256": RUNNER_SHA256,
            "bundles": evidence.bundles, "plan": state["plan"], "plan_sha256": plan["plan_sha256"],
            "campaign": evidence.identity(run_root + "/result.json"), "comparison": evidence.identity(run_root + "/comparison.json"),
            "host_as_recorded": plan["host"], "required_references_verified": len(links), "required_reference_locations": links,
            "source_archive_bindings": sources, "samples_verified": len(state["samples"]), "build_stages_verified": 8,
            "rows": rows, "correctness": "exact retained bytes", "scope": runner.THRESHOLDS["scope"],
            "limits": ["Does not execute retained code or independently prove the compiler produced a binary.",
                       "Three measured pairs are descriptive; no overall solver speed, saved BR or R1 acceptance follows.",
                       "All raw canonical, runtime, source, binary and input files required by this check must be retained."]}


def named(value):
    require("=" in value, "expected LABEL=VALUE")
    return value.split("=", 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", action="append", required=True, help="LABEL=local archive path")
    parser.add_argument("--sha256", action="append", required=True, help="LABEL=independently pinned archive SHA256")
    parser.add_argument("--run-root", help="unchanged absolute VM campaign path")
    parser.add_argument("--inventory-only", action="store_true", help="report missing direct FileRefs; never a campaign pass")
    parser.add_argument("--out", type=Path, required=True, help="new JSON report; never overwrite")
    args = parser.parse_args()
    pairs, pins = [named(v) for v in args.bundle], [named(v) for v in args.sha256]
    require(len(dict(pairs)) == len(pairs) and len(dict(pins)) == len(pins)
            and set(dict(pairs)) == set(dict(pins)), "bundle/pin labels differ or duplicate")
    require(args.inventory_only != bool(args.run_root), "choose inventory-only or run-root")
    require(not args.out.exists(), "output already exists")
    evidence = Evidence()
    try:
        for label, path in pairs:
            evidence.add_bundle(label, path, dict(pins)[label])
        if args.inventory_only:
            report = {"schema": "r1.codec-retained-inventory/v1", "status": "inventory_only_not_campaign_verified",
                      "bundles": evidence.bundles, "missing_direct_references": evidence.missing_refs()}
        else:
            report = verify_campaign(evidence, args.run_root)
        raw = (json.dumps(report, indent=2, sort_keys=True, allow_nan=False) + "\n").encode()
        with args.out.open("xb") as output:
            output.write(raw)
        print(json.dumps({"status": report["status"], "report": str(args.out)}))
    finally:
        evidence.close()


if __name__ == "__main__":
    main()
