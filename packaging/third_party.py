"""Generate complete release notices from locked package sources, never build code.

The Cargo graph is the union of the four release targets with normal/build edges
and cli/daemon defaults disabled. Build tools/proc macros are retained as a
conservative attribution superset; dev-only dependencies are not traversed.
No network license scraping, guessed copyright text, or generic text fallbacks.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
POLICY = ROOT / "packaging/third-party.json"
OUTPUT = ROOT / "packaging/THIRD-PARTY-LICENSES.txt"
NOTICE_NAME = re.compile(r"^(?:licen[cs]e|notice|copyright|unlicense)(?:$|[-_.])", re.I)


class NoticeError(Exception):
    """An incomplete or non-permissively licensed release input."""


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def checked_expression(expression: str | None, policy: dict, *, font: bool = False) -> str:
    """Parse SPDX AND/OR/parentheses/WITH, rejecting *every* disallowed branch.

    Cargo's historical slash syntax is equivalent to OR. A copyleft alternative
    does not become acceptable just because a permissive alternative exists.
    """
    if not isinstance(expression, str) or not expression.strip():
        raise NoticeError("missing license expression; requires a pinned license review")
    expression = expression.replace("/", " OR ")
    tokens = re.findall(r"[A-Za-z0-9.+-]+|[()]|\S", expression)
    allowed = set(policy["allowed_licenses"])
    if font:
        allowed.update(policy["font_licenses"])
    position = 0

    def atom() -> None:
        nonlocal position
        if position == len(tokens):
            raise NoticeError(f"incomplete license expression: {expression}")
        token = tokens[position]
        position += 1
        if token == "(":
            group()
            if position == len(tokens) or tokens[position] != ")":
                raise NoticeError(f"unclosed license expression: {expression}")
            position += 1
            return
        if token not in allowed:
            raise NoticeError(f"unapproved license {token!r} in {expression!r}")
        if position < len(tokens) and tokens[position] == "WITH":
            position += 1
            if position == len(tokens) or tokens[position] not in policy["allowed_exceptions"]:
                raise NoticeError(f"unapproved license exception: {expression}")
            position += 1

    def conjunction() -> None:
        nonlocal position
        atom()
        while position < len(tokens) and tokens[position] == "AND":
            position += 1
            atom()

    def group() -> None:
        nonlocal position
        conjunction()
        while position < len(tokens) and tokens[position] == "OR":
            position += 1
            conjunction()

    group()
    if position != len(tokens):
        raise NoticeError(f"invalid license expression: {expression}")
    return expression


def relative_file(directory: Path, name: str) -> Path:
    path = (directory / name).resolve()
    if not path.is_relative_to(directory.resolve()) or not path.is_file():
        raise NoticeError(f"missing or escaping notice file: {directory / name}")
    return path


def texts(directory: Path, extra: dict[str, str] | None = None,
          declared: str | None = None, checksums: dict | None = None) -> list[tuple[str, str]]:
    names = {p.name for p in directory.iterdir() if p.is_file() and NOTICE_NAME.match(p.name)}
    names.update(extra or {})
    if declared:
        path = relative_file(directory, declared)
        names.add(path.relative_to(directory.resolve()).as_posix())
    result = []
    for name in sorted(names):
        path = relative_file(directory, name)
        data = path.read_bytes()
        digest = sha256(data)
        if checksums is not None and checksums.get(name) != digest:
            raise NoticeError(f"license source differs from Cargo's checksums: {path}")
        if extra and name in extra and extra[name] != digest:
            raise NoticeError(f"reviewed license text changed: {path}")
        text = data.decode("utf-8-sig").replace("\r\n", "\n").replace("\r", "\n").strip()
        if not text:
            raise NoticeError(f"empty license file: {path}")
        result.append((name, text))
    # Short pointer files are retained, but cannot stand in for actual terms.
    if not any(len(text) >= 200 for _, text in result):
        raise NoticeError(f"no full license text found in {directory}")
    return result


def crate_checksums(directory: Path, package: dict, checksum: str) -> dict[str, str]:
    """Read notices only from sources matching the locked crates.io archive."""
    # Cargo's normal registry cache has archives, unlike cargo vendor output
    # (which supplies .cargo-checksum.json). Support both without trusting a
    # mutable extracted source file or downloading separate license documents.
    vendor = directory / ".cargo-checksum.json"
    if vendor.is_file():
        record = read_json(vendor)
        if record.get("package") != checksum:
            raise NoticeError(f"vendored source checksum mismatch: {directory}")
        return record["files"]
    archive = directory.parent.parent.parent / "cache" / directory.parent.name / (
        f"{package['name']}-{package['version']}.crate")
    if not archive.is_file():
        raise NoticeError(f"missing locked Cargo archive: {archive}; retain Cargo's registry cache")
    if sha256(archive.read_bytes()) != checksum:
        raise NoticeError(f"Cargo archive checksum mismatch: {archive}")
    prefix = f"{package['name']}-{package['version']}/"
    result = {}
    with tarfile.open(archive, "r:gz") as bundle:
        manifest = bundle.extractfile(prefix + "Cargo.toml")
        if manifest is None:
            raise NoticeError(f"missing manifest in {archive}")
        original = tomllib.loads(manifest.read().decode("utf-8"))["package"]
        if original.get("license") != package.get("license"):
            raise NoticeError(f"extracted Cargo license declaration changed: {directory}")
        for member in bundle.getmembers():
            if member.isfile() and member.name.startswith(prefix):
                name = member.name[len(prefix):]
                if NOTICE_NAME.match(Path(name).name):
                    handle = bundle.extractfile(member)
                    if handle is not None:
                        result[name] = sha256(handle.read())
    return result


def cargo_command(arguments: list[str], root: Path, offline: bool) -> str:
    command = ["cargo", *arguments, "--locked", "--color", "never"]
    if offline:
        command.append("--offline")
    # CARGO_TERM_* prevent terminals/CI from changing machine-readable output.
    environment = dict(os.environ, CARGO_TERM_COLOR="never", CARGO_TERM_PROGRESS_WHEN="never")
    result = subprocess.run(command, cwd=root, env=environment, check=True,
                            stdout=subprocess.PIPE, encoding="utf-8")
    return result.stdout


def rust_notices(root: Path, policy: dict, offline: bool) -> list[dict]:
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    locked = {(p["name"], p["version"], p.get("source")): p for p in lock["package"]}
    metadata = json.loads(cargo_command(
        ["metadata", "--format-version", "1", "--no-default-features"], root, offline))
    packages: dict[tuple[str, str], list[dict]] = {}
    for package in metadata["packages"]:
        packages.setdefault((package["name"], package["version"]), []).append(package)
    members = set(metadata["workspace_members"])
    selected: dict[str, dict] = {}
    for target in sorted(policy["targets"]):
        arguments = ["tree", "--no-default-features", "--target", target,
                     "--edges", "normal,build", "--prefix", "none", "--format", "{p}",
                     "--charset", "ascii"]
        for name in policy["rust_roots"]:
            arguments.extend(["--package", name])
        tree = cargo_command(arguments, root, offline)
        for line in tree.splitlines():
            if not line.strip() or line.strip() == "[build-dependencies]":
                continue
            match = re.match(r"^([A-Za-z0-9_-]+) v([^ ]+)(?: |$)", line)
            if not match:
                raise NoticeError(f"unrecognized cargo tree row: {line!r}")
            candidates = packages.get(match.groups(), [])
            if len(candidates) != 1:
                raise NoticeError(f"ambiguous/unresolved Cargo package: {line}")
            package = candidates[0]
            selected[package["id"]] = package
        if not tree.strip():
            raise NoticeError(f"empty Cargo release graph for {target}")
    result = []
    for package in selected.values():
        if package["id"] in members:
            continue  # First-party code ships under LICENSE-MIT / LICENSE-APACHE.
        name, version, source = package["name"], package["version"], package["source"]
        if source != "registry+https://github.com/rust-lang/crates.io-index":
            raise NoticeError(f"unreviewed non-crates.io dependency: {name} {version}: {source}")
        entry = locked.get((name, version, source))
        if not entry or not entry.get("checksum"):
            raise NoticeError(f"missing Cargo.lock checksum: {name} {version}")
        directory = Path(package["manifest_path"]).parent
        checksums = crate_checksums(directory, package, entry["checksum"])
        review = policy["rust_reviews"].get(f"{name}@{version}")
        expression = package.get("license")
        extra = None
        if review:
            if (review["checksum"] != entry["checksum"]
                    or review["declared_license"] != expression):
                raise NoticeError(f"stale pinned license review: {name} {version}")
            expression, extra = review["license"], review["license_files"]
        expression = checked_expression(expression, policy)
        result.append({"ecosystem": "Rust", "name": name, "version": version,
                       "license": expression, "source": source,
                       "checksum": entry["checksum"],
                       "texts": texts(directory, extra, package.get("license_file"), checksums)})
    return result


def resolve_npm(packages: dict, parent: str, name: str) -> str:
    """Resolve a dependency in npm's hoisted/nested node_modules lock layout."""
    directory = parent
    while True:
        candidate = f"{directory}/node_modules/{name}" if directory else f"node_modules/{name}"
        if candidate in packages:
            return candidate
        if not directory:
            raise NoticeError(f"unresolved production npm dependency {name!r} from {parent!r}")
        directory = directory.rsplit("/node_modules/", 1)[0] if "/node_modules/" in directory else ""


def npm_archive(root: Path, entry: dict, offline: bool) -> Path:
    """Fetch exact lockfile tarballs, not mutable installed node_modules files."""
    integrity = entry.get("integrity", "")
    if not integrity.startswith("sha512-") or " " in integrity:
        raise NoticeError("npm notices require a single SHA-512 lockfile integrity value")
    expected = base64.b64decode(integrity.removeprefix("sha512-"), validate=True)
    if len(expected) != 64:
        raise NoticeError("invalid npm SHA-512 integrity value")
    archive = root / ".cache/third-party/npm" / (expected.hex() + ".tgz")
    if archive.is_file():
        data = archive.read_bytes()
    else:
        if offline:
            raise NoticeError(f"missing cached npm archive: {entry['resolved']}; generate once online")
        with urllib.request.urlopen(entry["resolved"], timeout=60) as response:
            data = response.read()
    if hashlib.sha512(data).digest() != expected:
        raise NoticeError(f"npm archive integrity mismatch: {entry['resolved']}")
    if not archive.is_file():
        archive.parent.mkdir(parents=True, exist_ok=True)
        archive.write_bytes(data)
    return archive


def npm_archive_texts(archive: Path, name: str, entry: dict, policy: dict) -> list[tuple[str, str]]:
    result = []
    with tarfile.open(archive, "r:gz") as bundle:
        handle = bundle.extractfile("package/package.json")
        if handle is None:
            raise NoticeError(f"missing npm package.json: {name}")
        manifest = json.loads(handle.read())
        if manifest.get("name") != name or manifest.get("version") != entry["version"]:
            raise NoticeError(f"npm archive identity differs from lockfile: {name}")
        if checked_expression(manifest.get("license"), policy) != checked_expression(entry.get("license"), policy):
            raise NoticeError(f"npm archive license differs from lockfile: {name}")
        for member in sorted(bundle.getmembers(), key=lambda m: m.name):
            relative = member.name.removeprefix("package/")
            if member.isfile() and "/" not in relative and NOTICE_NAME.match(relative):
                handle = bundle.extractfile(member)
                if handle is not None:
                    text = handle.read().decode("utf-8-sig").replace("\r\n", "\n").replace("\r", "\n").strip()
                    result.append((relative, text))
    if not any(len(text) >= 200 for _, text in result):
        raise NoticeError(f"no complete npm license text: {name}")
    return result


def npm_notices(root: Path, policy: dict, offline: bool = False) -> list[dict]:
    web = root / "web"
    lock = read_json(web / "package-lock.json")
    manifest = read_json(web / "package.json")
    if lock.get("lockfileVersion") != 3:
        raise NoticeError("web/package-lock.json must use lockfileVersion 3")
    packages = lock["packages"]
    if manifest.get("dependencies", {}) != packages[""].get("dependencies", {}):
        raise NoticeError("npm production dependencies differ from package-lock.json; run npm ci")
    pending = [resolve_npm(packages, "", name) for name in manifest.get("dependencies", {})]
    visited = set()
    result = []
    while pending:
        location = pending.pop()
        if location in visited:
            continue
        visited.add(location)
        entry = packages[location]
        if entry.get("link") or entry.get("dev"):
            raise NoticeError(f"unexpected link/dev-only package in production graph: {location}")
        for field in ("dependencies", "optionalDependencies", "peerDependencies"):
            for dependency in entry.get(field, {}):
                optional = entry.get("peerDependenciesMeta", {}).get(dependency, {}).get("optional")
                if field == "peerDependencies" and optional:
                    continue
                pending.append(resolve_npm(packages, location, dependency))
        name = location.rsplit("node_modules/", 1)[1]
        expression = checked_expression(entry.get("license"), policy)
        source = entry.get("resolved", "")
        if not source.startswith("https://registry.npmjs.org/") or not entry.get("integrity"):
            raise NoticeError(f"unreviewed npm source or absent integrity: {location}")
        result.append({"ecosystem": "npm", "name": name, "version": entry["version"],
                       "license": expression, "source": source, "checksum": entry["integrity"],
                       "location": location,
                       "texts": npm_archive_texts(npm_archive(root, entry, offline), name, entry, policy)})
    return result


def font_notices(root: Path, policy: dict) -> list[dict]:
    result = []
    configured = set()
    for font in policy["fonts"]:
        assets = []
        for name in sorted(font["assets"]):
            path = relative_file(root, name)
            configured.add(path)
            assets.append(f"{name}: {sha256(path.read_bytes())}")
        notice_texts = []
        for name in sorted(font["license_files"]):
            path = relative_file(root, name)
            text = path.read_text(encoding="utf-8-sig").replace("\r\n", "\n").strip()
            if len(text) < 200:
                raise NoticeError(f"font license text is incomplete: {name}")
            notice_texts.append((name, text))
        if not notice_texts or not assets:
            raise NoticeError(f"font requires assets and license texts: {font['name']}")
        result.append({"ecosystem": "Font", "name": font["name"], "version": font["version"],
                       "license": checked_expression(font["license"], policy, font=True),
                       "source": font["source"], "assets": assets, "texts": notice_texts})
    bundled = set()
    for directory, children, files in os.walk(root / "web"):
        children[:] = sorted(name for name in children if name not in {"node_modules", "dist", ".git"})
        for name in files:
            path = Path(directory) / name
            if path.suffix.lower() in {".woff", ".woff2", ".ttf", ".otf"}:
                bundled.add(path.resolve())
    if bundled != configured:
        raise NoticeError("font inventory mismatch (update packaging/third-party.json): "
                          + ", ".join(sorted(str(p.relative_to(root)) for p in bundled ^ configured)))
    return result


def render(root: Path, policy: dict, notices: list[dict]) -> str:
    lines = ["THIRD-PARTY LICENSES", "====================", "",
             "Generated by: sh packaging/third-party.sh", "",
             "Scope: solvers (cli) and solversd (daemon), --no-default-features;",
             "normal/build dependencies (including proc macros/build tools), not dev-only packages;",
             "the Web GUI's production npm dependency closure and all bundled font assets.",
             "First-party code, including Cryst Design System, is covered by the root licenses.",
             "Rust target union: " + ", ".join(sorted(policy["targets"])),
             "No build timestamp or machine-local paths are included.", ""]
    for name in ("Cargo.lock", "web/package-lock.json", "packaging/third-party.json"):
        lines.append(f"SHA-256 {name}: {sha256((root / name).read_bytes())}")
    for notice in sorted(notices, key=lambda n: (n["ecosystem"], n["name"], n["version"], n.get("location", ""))):
        lines.extend(["", "=" * 78,
                      f"{notice['ecosystem']}: {notice['name']} {notice['version']}",
                      f"License: {notice['license']}", f"Source: {notice['source']}"])
        if "checksum" in notice:
            lines.append(f"Locked source checksum/integrity: {notice['checksum']}")
        if "location" in notice:
            lines.append(f"Package location: {notice['location']}")
        for asset in notice.get("assets", []):
            lines.append(f"Asset SHA-256: {asset}")
        for name, text in notice["texts"]:
            lines.extend(["", f"--- {name} ---", "", text])
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument("--offline", action="store_true", help="require cached Cargo and npm source archives")
    parser.add_argument("--check", action="store_true", help="compare generated notices without writing")
    args = parser.parse_args()
    try:
        policy = read_json(POLICY)
        if policy.get("schema") != 1:
            raise NoticeError("unsupported third-party policy schema")
        notices = rust_notices(ROOT, policy, args.offline)
        notices.extend(npm_notices(ROOT, policy, args.offline))
        notices.extend(font_notices(ROOT, policy))
        data = render(ROOT, policy, notices).encode("utf-8")
        if args.check:
            if not args.output.is_file() or args.output.read_bytes() != data:
                raise NoticeError(f"missing/stale notices: run sh packaging/third-party.sh --output {args.output}")
        else:
            # All inputs are validated before replacing the last complete file.
            args.output.parent.mkdir(parents=True, exist_ok=True)
            temporary = None
            try:
                with tempfile.NamedTemporaryFile(dir=args.output.parent, delete=False) as handle:
                    temporary = Path(handle.name)
                    handle.write(data)
                temporary.replace(args.output)
            finally:
                if temporary is not None:
                    temporary.unlink(missing_ok=True)
        print(f"third-party: {len(notices)} notices {'checked' if args.check else 'written'}: {args.output}")
        return 0
    except (NoticeError, OSError, ValueError, tarfile.TarError, subprocess.CalledProcessError) as error:
        print(f"third-party: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
