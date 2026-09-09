"""Index retained Multiway benchmark inputs without executing experiments.

Config bytes, explicit TOML settings, and recorded research argv are separate
identities. Neither a config nor a prepared manifest proves a successful run.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import tomllib
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HISTORICAL_REF = "db930159e661c2ccd490884d70de2b4092713355"
OUTPUT = "docs/validation/multiway-benchmark-config-index-2026-09-09"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def encoded(value: object) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2, default=str) + "\n"


def git(root: Path, *args: str) -> bytes:
    return subprocess.run(["git", *args], cwd=root, check=True, capture_output=True).stdout


def is_multiway(value: dict) -> bool:
    return "multiway" in str(value.get("schema", "")) or "multiway" in str(value.get("game", {}).get("kind", ""))


def summarize(value: dict) -> dict:
    game, solver, run = (value.get(key, {}) for key in ("game", "solver", "run"))
    return {
        "schema": value.get("schema"),
        "seats": game.get("seat_count", len(game["seats"]) if isinstance(game.get("seats"), list) else None),
        "defaults": game.get("defaults"),
        "utility": value.get("utility"),
        "economics": game.get("economics"),
        "rake": game.get("rake"),
        "tree": {key: game.get("tree", {}).get(key) for key in
                 ("kind", "allow_limp", "max_aggressive_actions", "source")},
        "tree_rule_count": len(game.get("tree", {}).get("rules", [])),
        "tree_rules_sha256": digest(encoded(game.get("tree", {}).get("rules", [])).encode()),
        "abstraction": game.get("abstraction"),
        "information": game.get("information"),
        "solver": solver,
        "run": run,
        "legacy_algorithm": value.get("algorithm"),
        "legacy_limits": value.get("limits"),
    }


def candidate_files(root: Path) -> list[Path]:
    files = set()
    for name in ("examples", "runs", "experiments"):
        directory = root / name
        if directory.exists():
            for path in directory.rglob("*.toml"):
                if any(part.startswith("multiway-convergence-test-") for part in path.parts):
                    continue
                files.add(path)
    return sorted(files)


def research_manifests(root: Path) -> tuple[list[dict], list[dict]]:
    manifests, issues = [], []
    for path in sorted((root / "runs").rglob("*manifest*.json")):
        if path.stat().st_size > 1_000_000:
            continue  # These are not small experiment manifests.
        try:
            value = json.loads(path.read_text(encoding="utf-8-sig"))
        except (ValueError, UnicodeError) as exc:
            issues.append({"path": path.relative_to(root).as_posix(), "issue": str(exc)})
            continue
        if not isinstance(value, dict) or value.get("schema") != "solvers.multiway-algorithm-screen-plan/v1":
            continue
        manifests.append({
            "path": path.relative_to(root).as_posix(), "sha256": digest(path.read_bytes()),
            "preparation_status_only": value.get("status"),
            "source": value.get("source"), "executable": value.get("executable"),
            "common": value.get("common"), "measurement_boundary": value.get("measurement_boundary"),
            "execution_contract": value.get("execution_contract"), "control_gate": value.get("control_gate"),
            "arms": value.get("arms"),
        })
    return manifests, issues


def collect(root: Path, *, historical: bool = True) -> dict:
    entries, configs, excluded, issues = [], {}, [], []

    def add(path: str, raw: bytes, origin: str, *, force: bool = False) -> None:
        try:
            value = tomllib.loads(raw.decode("utf-8-sig"))
        except (ValueError, UnicodeError) as exc:
            issues.append({"path": path, "origin": origin, "sha256": digest(raw), "issue": str(exc)})
            return
        if not value:
            issues.append({"path": path, "origin": origin, "sha256": digest(raw), "issue": "empty TOML; not a recoverable benchmark configuration"})
            return
        if not (force or is_multiway(value)):
            excluded.append(path)
            return
        sha256 = digest(raw)
        entries.append({"path": path, "origin": origin, "sha256": sha256, "bytes": len(raw)})
        configs.setdefault(sha256, {
            "sha256": sha256,
            "parsed_settings_sha256": digest(encoded(value).encode()),
            "summary": summarize(value), "explicit_settings": value,
        })

    for path in candidate_files(root):
        relative = path.relative_to(root).as_posix()
        add(relative, path.read_bytes(), "working-tree", force=relative.startswith("examples/bench_multiway/"))
    if historical:
        paths = git(root, "ls-tree", "-r", "--name-only", HISTORICAL_REF, "experiments").decode().splitlines()
        for path in paths:
            if path.endswith(".toml"):
                add(path, git(root, "show", f"{HISTORICAL_REF}:{path}"), f"git:{HISTORICAL_REF}", force=True)
    manifests, manifest_issues = research_manifests(root)
    counts = Counter(entry["origin"] for entry in entries)
    return {
        "schema": "solvers.multiway-benchmark-inventory/v1",
        "scope": {
            "roots": ["examples", "runs", "experiments"],
            "retained_run_roots": ["runs"],
            "historical_ref": HISTORICAL_REF if historical else None,
            "excluded": ["compiler/build trees", "unit-test temporary fixtures", "cached source copies", "archive copies of already indexed sources", "other solver families"],
            "historical_boundary": "Deleted July research configs are documentation only; no retired backend is restored or authorized for production.",
            "execution_boundary": "Explicit config settings and recorded argv are distinct. Missing fields remain unspecified. Config/manifest presence is not execution or quality evidence.",
        },
        "counts": {"files": len(entries), "unique_config_bytes": len(configs),
                   "unique_explicit_settings": len({item["parsed_settings_sha256"] for item in configs.values()}), "by_origin": dict(counts),
                   "config_issues": len(issues), "research_manifests": len(manifests), "manifest_issues": len(manifest_issues)},
        "files": entries, "configurations": sorted(configs.values(), key=lambda row: row["sha256"]),
        "research_manifests": manifests, "config_issues": issues, "manifest_issues": manifest_issues,
        "excluded_non_multiway_toml": excluded,
    }


def compact(value: object) -> str:
    if value is None:
        return "未指定"
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).replace("|", "\\|")


def markdown(index: dict) -> str:
    counts = index["counts"]
    lines = ["# Multiway ベンチマーク設定の全件索引（2026-09-09）", "",
             "この索引は `tools/multiway_benchmark_inventory.py` で再生成する。条件の解釈は",
             "[総合カタログ](multiway-benchmarks-2026-09-09.md)、[設定系統](multiway-benchmark-families-2026-09-09.md)、",
             "[実行台帳](multiway-benchmark-runs-2026-09-09.md)を参照する。", "",
             f"設定 {counts['files']} ファイル、内容SHA-256で {counts['unique_config_bytes']} 種類。",
             f"明示TOML設定の比較では {counts['unique_explicit_settings']} 種類。既定値やCLI上書きを含む実行上の等価性は表さない。",
             f"復元不能な空/不正TOML等 {counts['config_issues']} 件、研究manifest {counts['research_manifests']} 件。",
             "完全なTOML解釈結果、全SHA-256、記録済みargv・評価条件・source/binary識別は",
             "[JSON索引](multiway-benchmark-config-index-2026-09-09.json)に保存した。", "",
             "これはファイルの存在を示す索引であり、実行成功や現行仕様への適合を認定しない。",
             "省略値には現行既定値を補わない。CLI引数が上書きする条件は研究manifestのargvを確認する。",
             "Julyの削除済み設定は固定Git revisionから読み取った。現行productionへ復活させていない。",
             "`.cache`のsource複製、圧縮archiveの複製、ビルドtree、自動テスト用一時fixture、他solverは除外した。", "",
             "## 設定内容ごとの索引", ""]
    for config in index["configurations"]:
        sha256, summary = config["sha256"], config["summary"]
        lines += [f"### `{sha256[:12]}`", "",
                  f"schema: `{summary['schema']}`。seats: {compact(summary['seats'])}。",
                  f"abstraction: `{compact(summary['abstraction'])}`。",
                  f"solver: `{compact(summary['solver'])}`。",
                  f"run: `{compact(summary['run'])}`。", ""]
        for entry in index["files"]:
            if entry["sha256"] != sha256:
                continue
            path = entry["path"]
            if entry["origin"] == "working-tree":
                lines.append(f"- [{path}](../../{path})")
            else:
                lines.append(f"- Git `{HISTORICAL_REF[:12]}:{path}`（削除済み研究設定）")
        lines.append("")
    lines += ["## 回収・解釈できない入力", ""]
    for issue in index["config_issues"] + index["manifest_issues"]:
        lines.append(f"- `{issue['path']}`: {issue['issue']}")
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="write generated JSON and Markdown")
    parser.add_argument("--check", action="store_true", help="fail if generated files differ from present inputs")
    args = parser.parse_args()
    if args.write and args.check:
        parser.error("choose --write or --check")
    result = collect(ROOT)
    outputs = {ROOT / (OUTPUT + ".json"): encoded(result), ROOT / (OUTPUT + ".md"): markdown(result)}
    for path, content in outputs.items():
        if args.write:
            path.write_text(content, encoding="utf-8", newline="\n")
        if args.check and (not path.is_file() or path.read_text(encoding="utf-8") != content):
            raise SystemExit(f"inventory is stale: {path.relative_to(ROOT)}")
    print(encoded(result["counts"]))


if __name__ == "__main__":
    main()
