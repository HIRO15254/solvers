#!/usr/bin/env python3
"""Reproducible, small Multiway Preflop convergence benchmarks.

The runner deliberately uses only the Python standard library.  It generates
canonical-v1 TOML variants from an existing production config, runs one
variant at a time, and keeps the raw config, command logs, run directory,
held-out evaluation, and inspection output together.  It is intended for
local smoke runs and for an externally provisioned batch host; it does not
create cloud resources.

The v1 CLI does not expose a switch that removes ``run.stop``.  Fixed-budget
runs therefore set ``check_every_sweeps`` above the sweep ceiling.  The final
evaluation still runs, but no stop-rule check can fire before the budget ends.
"""

from __future__ import annotations

import argparse
import hashlib
import itertools
import json
import math
import os
import re
import shutil
import shlex
import subprocess
import sys
import time
import tomllib
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Any, Iterable, Sequence


SCHEMA = "solvers.multiway-preflop/v1"
TOOL_SCHEMA = "solvers.multiway-convergence-bench/v1"
ARTIFACT_EVALUATION_NOTE = (
    "solution.mwsol stores preflop nodes only; held-out evaluate therefore "
    "must be interpreted with the source tree's postflop terminal/checkdown "
    "semantics. This runner does not infer a general-tree policy equivalence."
)
PROFILE_EQUIVALENCE_VERIFIED = "verified-all-in-tree"
PROFILE_EQUIVALENCE_UNKNOWN = "unknown-preflop-only-artifact"
CACHE_NOTICE_RE = re.compile(
    r"^ehs2 tables:?\s+(?:loaded|built) in [0-9]+(?:\.[0-9]+)?s$"
)
DEFAULT_SEEDS = (0, 11, 29)
DEFAULT_KINDS = ("range-vector", "single-hand")
DEFAULT_PRUNING = ("none", "regret-based")
DEFAULT_BATCHES = (1, 4)
DEFAULT_DISCOUNTS = ("config", "1000")
DEFAULT_EVAL_SAMPLES = 256
DEFAULT_BR_TRAVERSALS = 2_000
DEFAULT_TIMEOUT = 900.0
MAX_U64 = (1 << 64) - 1


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def evaluation_profile_equivalence(config_text: str) -> str:
    """Classify only the known 3-/6-max 2bb one-action sanity trees.

    A `.mwsol` contains preflop nodes, so arbitrary source configs must not be
    ranked as if their postflop policy were present.  This intentionally
    recognizes a narrow, semantic fixture shape instead of trusting its path
    or filename.
    """

    try:
        config = tomllib.loads(config_text)
        game = config["game"]
        defaults = game["defaults"]
        tree = game["tree"]
        actions = tree["max_aggressive_actions"]
        if (
            set(game) <= {"seat_count", "button", "defaults", "tree", "abstraction", "information"}
            and set(defaults) == {"stack_bb", "range"}
            and set(tree) == {"kind", "allow_limp", "max_aggressive_actions"}
            and game.get("seat_count") in {3, 6}
            and game.get("button") == 0
            and defaults.get("stack_bb") == 2.0
            and defaults.get("range") == "random"
            and tree.get("kind") == "standard"
            and tree.get("allow_limp") is False
            and not tree.get("rules")
            and set(actions) == {"preflop", "flop", "turn", "river"}
            and all(actions[street] == 1 for street in actions)
            and game.get("players") is None
        ):
            return PROFILE_EQUIVALENCE_VERIFIED
    except (KeyError, TypeError, ValueError, tomllib.TOMLDecodeError):
        pass
    return PROFILE_EQUIVALENCE_UNKNOWN


def copy_external_tree_source(
    source_config: Path, config_text: str, variant_root: Path
) -> list[dict[str, str]]:
    """Copy the one supported external v1 input beside a generated config.

    The typed parser resolves `[game.tree] source` relative to the config file
    directory.  Since variants are copied into their own directory, preserve
    that relative layout instead of allowing a missing or host-local path.
    """

    try:
        tree = tomllib.loads(config_text).get("game", {}).get("tree", {})
    except (AttributeError, tomllib.TOMLDecodeError):
        return []
    if not isinstance(tree, dict) or "source" not in tree:
        return []
    reference = tree["source"]
    if not isinstance(reference, str) or not reference:
        raise ValueError("[game.tree].source must be a non-empty relative path")
    relative = Path(reference)
    if relative.is_absolute():
        raise ValueError("absolute [game.tree].source paths are unsupported by the benchmark runner")
    source_path = (source_config.parent / relative).resolve()
    variant_path = (variant_root / relative).resolve()
    try:
        variant_path.relative_to(variant_root.resolve())
    except ValueError as exc:
        raise ValueError("[game.tree].source must stay inside the generated variant directory") from exc
    if not source_path.is_file():
        raise ValueError(f"relative [game.tree].source does not exist: {source_path}")
    variant_path.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source_path, variant_path)
    return [
        {
            "reference": reference,
            "source": str(source_path),
            "copied_to": str(variant_path),
        }
    ]


def external_tree_source_reference(config_text: str) -> str | None:
    try:
        value = tomllib.loads(config_text)
        tree = value.get("game", {}).get("tree", {})
    except (AttributeError, tomllib.TOMLDecodeError):
        return None
    reference = tree.get("source") if isinstance(tree, dict) else None
    return reference if isinstance(reference, str) else None


def parse_csv_values(raw: str, *, name: str) -> list[str]:
    values = [item.strip() for item in raw.split(",") if item.strip()]
    if not values:
        raise ValueError(f"{name} must contain at least one value")
    return values


def parse_int_list(raw: str, *, name: str, positive: bool = False) -> list[int]:
    result: list[int] = []
    for value in parse_csv_values(raw, name=name):
        try:
            parsed = int(value, 10)
        except ValueError as exc:
            raise ValueError(f"{name} contains a non-integer value: {value!r}") from exc
        if positive and parsed <= 0:
            raise ValueError(f"{name} values must be positive: {parsed}")
        result.append(parsed)
    return result


def _section_bounds(lines: list[str], section: str) -> tuple[int, int] | None:
    header = f"[{section}]"
    start = next((i for i, line in enumerate(lines) if line.strip() == header), None)
    if start is None:
        return None
    end = len(lines)
    for i in range(start + 1, len(lines)):
        if re.match(r"^\s*\[+[^]]+\]\s*$", lines[i]):
            end = i
            break
    return start, end


def set_toml_key(text: str, section: str, key: str, value: str) -> str:
    """Set a simple key while preserving the source's formatting/comments.

    The production configs use flat keys in the affected sections.  This
    helper intentionally rejects dotted keys and duplicate assignments rather
    than attempting to be a general TOML writer.
    """

    if "." in key or not re.fullmatch(r"[A-Za-z0-9_-]+", key):
        raise ValueError(f"unsupported TOML key: {key}")
    lines = text.splitlines(keepends=True)
    bounds = _section_bounds(lines, section)
    assignment = f"{key} = {value}\n"
    key_re = re.compile(rf"^\s*{re.escape(key)}\s*=")
    if bounds is None:
        if lines and not lines[-1].endswith(("\n", "\r")):
            lines[-1] += "\n"
        if lines and lines[-1].strip():
            lines.append("\n")
        lines.extend([f"[{section}]\n", assignment])
        return "".join(lines)
    start, end = bounds
    matches = [i for i in range(start + 1, end) if key_re.match(lines[i])]
    if len(matches) > 1:
        raise ValueError(f"section [{section}] contains duplicate key {key}")
    if matches:
        original = lines[matches[0]]
        newline = "\n" if original.endswith("\n") else ""
        comment = ""
        if "#" in original:
            comment = "  " + original.split("#", 1)[1].rstrip("\r\n")
            comment = " #" + comment.strip()
        lines[matches[0]] = f"{key} = {value}{comment}{newline}"
    else:
        lines.insert(start + 1, assignment)
    return "".join(lines)


def remove_toml_key(text: str, section: str, key: str) -> str:
    """Remove a flat key from a section, preserving all other source text."""

    if "." in key or not re.fullmatch(r"[A-Za-z0-9_-]+", key):
        raise ValueError(f"unsupported TOML key: {key}")
    lines = text.splitlines(keepends=True)
    bounds = _section_bounds(lines, section)
    if bounds is None:
        return text
    start, end = bounds
    key_re = re.compile(rf"^\s*{re.escape(key)}\s*=")
    matches = [i for i in range(start + 1, end) if key_re.match(lines[i])]
    if len(matches) > 1:
        raise ValueError(f"section [{section}] contains duplicate key {key}")
    if matches:
        del lines[matches[0]]
    return "".join(lines)


def set_or_add_discount(text: str, discount: str) -> str:
    if discount == "config":
        return text
    if discount == "none":
        # The v1 tagged enum is strict: periodic-only fields are invalid when
        # kind is none, so remove stale fields from the source variant.
        text = remove_toml_key(text, "solver.discount", "every_sweeps")
        text = remove_toml_key(text, "solver.discount", "until_sweeps")
        return set_toml_key(text, "solver.discount", "kind", '"none"')
    try:
        every = int(discount, 10)
    except ValueError as exc:
        raise ValueError(
            f"discount must be config, none, or a positive cadence: {discount!r}"
        ) from exc
    if every <= 0:
        raise ValueError("discount cadence must be positive")
    text = set_toml_key(text, "solver.discount", "kind", '"periodic"')
    text = set_toml_key(text, "solver.discount", "every_sweeps", str(every))
    return text


def fixed_budget_config(
    source: str,
    *,
    seed: int,
    solver_kind: str,
    pruning: str,
    batch: int,
    discount: str,
    sweeps: int,
    max_time: str | None,
) -> str:
    if sweeps <= 0 or sweeps >= MAX_U64:
        raise ValueError("sweeps must be positive and less than 2^64-1")
    if max_time is not None and not re.fullmatch(r"[1-9][0-9]*(?:s|m|h)", max_time):
        raise ValueError("max_time must be a positive duration such as 30s, 2m, or 1h")
    text = source
    text = set_toml_key(text, "solver", "kind", json.dumps(solver_kind))
    text = set_toml_key(text, "solver", "seed", str(seed))
    text = set_toml_key(text, "solver", "batch_sweeps", str(batch))
    text = set_toml_key(text, "solver.pruning", "kind", json.dumps(pruning))
    text = set_toml_key(text, "run", "max_sweeps", str(sweeps))
    # The v1 drive loop's stop rule is evaluated only on this cadence.  A
    # value above the safety ceiling makes the run sweep/time bounded.
    check_every = sweeps + 1
    text = set_toml_key(text, "run.stop", "check_every_sweeps", str(check_every))
    # A sweep-budget variant must not inherit a production time ceiling from
    # the source config.  ``--max-time`` is the explicit opt-in for timed runs.
    text = remove_toml_key(text, "run", "max_time")
    if max_time is not None:
        text = set_toml_key(text, "run", "max_time", json.dumps(max_time))
    return set_or_add_discount(text, discount)


def find_solver(explicit: str | None) -> Path | str:
    if explicit:
        return Path(explicit)
    candidates = [
        Path("target/release/solvers.exe"),
        Path("target/release/solvers"),
        Path("target/debug/solvers.exe"),
        Path("target/debug/solvers"),
    ]
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return "solvers"


def file_sha256(path: Path | str) -> str | None:
    path = Path(path)
    try:
        digest = hashlib.sha256()
        with path.open("rb") as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(block)
        return digest.hexdigest()
    except OSError:
        return None


def git_revision() -> str | None:
    """Read the checked-out revision without spawning git or printing secrets."""

    git = Path(".git")
    if git.is_file():
        try:
            git = Path(git.read_text(encoding="utf-8").strip().split("gitdir:", 1)[-1].strip())
        except OSError:
            return None
    try:
        head = (git / "HEAD").read_text(encoding="utf-8").strip()
    except OSError:
        return None
    if head.startswith("ref: "):
        ref = head[5:]
        try:
            return (git / ref).read_text(encoding="utf-8").strip()
        except OSError:
            try:
                for line in (git / "packed-refs").read_text(encoding="utf-8").splitlines():
                    if line and not line.startswith("#"):
                        oid, name = line.split(" ", 1)
                        if name == ref:
                            return oid
            except (OSError, ValueError):
                return None
        return None
    return head or None


def check_output_collisions(output_root: Path, variants: Iterable[Variant]) -> None:
    """Refuse a rerun that could overwrite any retained benchmark artifact."""

    collisions = []
    for variant in variants:
        variant_root = output_root / "variants" / variant.id
        if variant_root.exists() and any(variant_root.iterdir()):
            collisions.append(str(variant_root))
    if collisions:
        raise FileExistsError(
            "refusing to overwrite existing benchmark variant directories: "
            + ", ".join(collisions)
        )


@dataclass(frozen=True)
class Variant:
    seed: int
    solver_kind: str
    pruning: str
    batch_sweeps: int
    discount: str

    @property
    def id(self) -> str:
        discount = {"config": "config", "none": "none"}.get(self.discount, f"every{self.discount}")
        return (
            f"s{self.seed:04d}-{self.solver_kind}-prune-{self.pruning}"
            f"-batch{self.batch_sweeps}-discount-{discount}"
        )


def variants_from_args(
    seeds: Iterable[int], kinds: Iterable[str], pruning: Iterable[str], batches: Iterable[int], discounts: Iterable[str]
) -> tuple[list[Variant], list[dict[str, str]]]:
    kinds = tuple(kinds)
    pruning = tuple(pruning)
    valid_kinds = {"range-vector", "single-hand"}
    valid_pruning = {"none", "regret-based"}
    unknown_kinds = sorted(set(kinds) - valid_kinds)
    if unknown_kinds:
        raise ValueError(f"unsupported solver kind(s): {', '.join(unknown_kinds)}")
    unknown_pruning = sorted(set(pruning) - valid_pruning)
    if unknown_pruning:
        raise ValueError(f"unsupported pruning kind(s): {', '.join(unknown_pruning)}")
    variants: list[Variant] = []
    invalid: list[dict[str, str]] = []
    for seed, kind, prune, batch, discount in itertools.product(seeds, kinds, pruning, batches, discounts):
        reason = None
        if kind == "single-hand" and prune == "regret-based":
            reason = "current v1 rejects regret-based pruning with single-hand"
        elif batch <= 0:
            reason = "batch_sweeps must be positive"
        if reason:
            invalid.append({"combination": repr([seed, kind, prune, batch, discount]), "reason": reason})
            continue
        variants.append(Variant(seed, kind, prune, batch, discount))
    if not variants:
        raise ValueError("the selected matrix contains no valid variants")
    return variants, invalid


def _write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _run_command(command: Sequence[str], *, stdout: Path, stderr: Path, timeout: float) -> dict[str, Any]:
    started = time.perf_counter()
    timed_out = False
    returncode: int | None = None
    try:
        with stdout.open("w", encoding="utf-8", newline="") as out, stderr.open(
            "w", encoding="utf-8", newline=""
        ) as err:
            completed = subprocess.run(
                list(command), stdout=out, stderr=err, check=False, timeout=timeout
            )
            returncode = completed.returncode
    except subprocess.TimeoutExpired:
        timed_out = True
        returncode = None
    return {
        "command": [str(item) for item in command],
        "returncode": returncode,
        "timed_out": timed_out,
        "wall_seconds": time.perf_counter() - started,
        "stdout": str(stdout),
        "stderr": str(stderr),
    }


def _read_json(path: Path) -> dict[str, Any] | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    return value if isinstance(value, dict) else None


def parse_evaluation_stdout(raw: str) -> dict[str, Any]:
    """Parse the evaluator's JSON after an optional known cache notice.

    Do not search for an arbitrary ``{``: an unexpected prelude or trailing
    output makes the evaluation unusable and must be visible to callers.
    """

    lines = raw.splitlines()
    index = 0
    while index < len(lines):
        line = lines[index].strip()
        if not line or CACHE_NOTICE_RE.fullmatch(line):
            index += 1
            continue
        break
    if index == len(lines):
        raise ValueError("evaluation output contains no JSON document")
    try:
        value = json.loads("\n".join(lines[index:]))
    except json.JSONDecodeError as exc:
        raise ValueError(f"evaluation output JSON is invalid: {exc.msg}") from exc
    if not isinstance(value, dict):
        raise ValueError("evaluation output JSON must be an object")
    required = ("samples", "seats", "deviation_gain_lower_bound", "total_deal_attempts")
    missing = [key for key in required if key not in value]
    if missing:
        raise ValueError("evaluation output is missing required metrics: " + ", ".join(missing))
    if (
        isinstance(value["samples"], bool)
        or not isinstance(value["samples"], int)
        or value["samples"] <= 0
    ):
        raise ValueError("evaluation output samples must be a positive integer")
    if (
        isinstance(value["total_deal_attempts"], bool)
        or not isinstance(value["total_deal_attempts"], int)
        or value["total_deal_attempts"] < 0
    ):
        raise ValueError("evaluation output total_deal_attempts must be a non-negative integer")
    seats = value["seats"]
    deviations = value["deviation_gain_lower_bound"]
    if not isinstance(seats, list) or not seats or not isinstance(deviations, list):
        raise ValueError("evaluation output seats and deviation_gain_lower_bound must be arrays")
    if not 2 <= len(seats) <= 9:
        raise ValueError("evaluation output must contain metrics for 2 through 9 seats")
    if len(seats) != len(deviations):
        raise ValueError("evaluation output seat metrics have inconsistent lengths")
    for name, metrics in (("seats", seats), ("deviation_gain_lower_bound", deviations)):
        for index, metric in enumerate(metrics):
            if not isinstance(metric, dict) or not all(
                key in metric for key in ("mean", "stderr", "ci95")
            ):
                raise ValueError(f"evaluation output {name}[{index}] lacks mean/stderr/ci95")
            if not isinstance(metric["ci95"], list) or len(metric["ci95"]) != 2:
                raise ValueError(f"evaluation output {name}[{index}].ci95 must have two values")
            if any(
                isinstance(metric[key], bool)
                or not isinstance(metric[key], (int, float))
                or not math.isfinite(float(metric[key]))
                for key in ("mean", "stderr")
            ) or any(
                isinstance(endpoint, bool)
                or not isinstance(endpoint, (int, float))
                or not math.isfinite(float(endpoint))
                for endpoint in metric["ci95"]
            ):
                raise ValueError(f"evaluation output {name}[{index}] contains non-finite metrics")
            lower, upper = metric["ci95"]
            mean = float(metric["mean"])
            stderr = float(metric["stderr"])
            if stderr < 0 or lower > upper or not lower <= mean <= upper:
                raise ValueError(f"evaluation output {name}[{index}] has an impossible interval")
            if name == "deviation_gain_lower_bound" and (mean < 0 or lower < 0 or upper < 0):
                raise ValueError(
                    f"evaluation output {name}[{index}] has a negative nonnegative deviation metric"
                )
    return value


def _parse_events(run_dir: Path) -> dict[str, Any]:
    result: dict[str, Any] = {"abstraction_build_seconds": None, "abstraction_cached": None, "events": []}
    path = run_dir / "events.jsonl"
    if not path.is_file():
        return result
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        result["events"].append(event)
        payload = event if isinstance(event, dict) else {}
        message = str(payload.get("message", ""))
        match = re.search(r"ehs2 tables (loaded|built) in ([0-9.]+)s", message)
        if match:
            result["abstraction_cached"] = match.group(1) == "loaded"
            result["abstraction_build_seconds"] = float(match.group(2))
    return result


def _run_one(
    solver: Path | str,
    cache_dir: Path | None,
    source_config: Path,
    output_root: Path,
    variant: Variant,
    *,
    sweeps: int,
    max_time: str | None,
    eval_samples: int,
    eval_seed: int,
    br_traversals: int,
    timeout: float,
    inspect: bool,
    evaluation_solver: Path | str | None = None,
) -> dict[str, Any]:
    variant_root = output_root / "variants" / variant.id
    run_dir = variant_root / "run"
    config_path = variant_root / "config.toml"
    if variant_root.exists() and any(variant_root.iterdir()):
        raise FileExistsError(f"refusing to overwrite existing benchmark variant: {variant_root}")
    variant_root.mkdir(parents=True, exist_ok=True)
    source = source_config.read_text(encoding="utf-8")
    config = fixed_budget_config(
        source,
        seed=variant.seed,
        solver_kind=variant.solver_kind,
        pruning=variant.pruning,
        batch=variant.batch_sweeps,
        discount=variant.discount,
        sweeps=sweeps,
        max_time=max_time,
    )
    profile_equivalence = evaluation_profile_equivalence(config)
    if run_dir.exists() and any(run_dir.iterdir()):
        raise FileExistsError(f"refusing to overwrite existing run directory: {run_dir}")
    if config_path.exists():
        raise FileExistsError(f"refusing to overwrite existing config: {config_path}")
    external_dependencies = copy_external_tree_source(source_config, config, variant_root)
    config_path.write_text(config, encoding="utf-8", newline="\n")
    command = [str(solver), "solve", str(config_path), "--out", str(run_dir)]
    if cache_dir is not None:
        command[1:1] = ["--cache-dir", str(cache_dir)]
    solve_meta = _run_command(
        command,
        stdout=variant_root / "solve.stdout.log",
        stderr=variant_root / "solve.stderr.log",
        timeout=timeout,
    )
    evaluation_meta = None
    inspect_meta = None
    solution = run_dir / "solution.mwsol"
    if not solve_meta["timed_out"] and solve_meta["returncode"] == 0 and solution.is_file():
        # ``--cache-dir`` is a global CLI option, so use it for every phase
        # that may need the abstraction cache (solve and held-out evaluate).
        evaluator = evaluation_solver or solver
        evaluate_command = [str(evaluator)]
        if cache_dir is not None:
            evaluate_command.extend(["--cache-dir", str(cache_dir)])
        evaluate_command.extend(
            [
                "evaluate",
                str(solution),
                "--samples",
                str(eval_samples),
                "--seed",
                str(eval_seed),
                "--br-traversals",
                str(br_traversals),
            ]
        )
        evaluation_meta = _run_command(
            evaluate_command,
            stdout=variant_root / "evaluation.stdout.log",
            stderr=variant_root / "evaluation.stderr.log",
            timeout=timeout,
        )
        if not evaluation_meta["timed_out"] and evaluation_meta["returncode"] == 0:
            try:
                evaluation = parse_evaluation_stdout(
                    (variant_root / "evaluation.stdout.log").read_text(
                        encoding="utf-8", errors="replace"
                    )
                )
                if evaluation["samples"] != eval_samples:
                    raise ValueError(
                        "evaluation output samples do not match requested "
                        f"count ({evaluation['samples']} != {eval_samples})"
                    )
            except (OSError, ValueError) as exc:
                evaluation_parse_error = str(exc)
                evaluation_meta["usable"] = False
                evaluation_meta["parse_error"] = evaluation_parse_error
            else:
                _write_json(variant_root / "evaluation.json", evaluation)
                evaluation_meta["usable"] = True
                evaluation_meta["json"] = str(variant_root / "evaluation.json")
        else:
            evaluation_meta["usable"] = False
        if inspect:
            inspect_command = [str(solver)]
            if cache_dir is not None:
                inspect_command.extend(["--cache-dir", str(cache_dir)])
            # Multiway artifacts are passed as the positional CONFIG argument;
            # ``--sol`` selects the legacy heads-up viewer path and rejects a
            # `.mwsol` file even though it appears in inspect's help.
            inspect_command.extend(["inspect", str(solution), "--view", "summary"])
            inspect_meta = _run_command(
                inspect_command,
                stdout=variant_root / "inspect.stdout.log",
                stderr=variant_root / "inspect.stderr.log",
                timeout=timeout,
            )
    run_result = _read_json(run_dir / "run.json")
    event_info = _parse_events(run_dir)
    if solve_meta["timed_out"]:
        status = "solve-timeout"
    elif solve_meta["returncode"] != 0:
        status = "solve-failed"
    elif not solution.is_file():
        status = "missing-solution"
    elif evaluation_meta is not None and not evaluation_meta.get("usable", False):
        status = "evaluation-failed"
    elif inspect_meta is not None and (
        inspect_meta["timed_out"] or inspect_meta["returncode"] != 0
    ):
        status = "inspect-failed"
    else:
        status = "ok"
    summary = {
        "schema": TOOL_SCHEMA,
        "status": status,
        "evaluation_profile_equivalence": profile_equivalence,
        "artifact_evaluation_note": ARTIFACT_EVALUATION_NOTE,
        "external_tree_source": external_tree_source_reference(config),
        "external_dependencies": external_dependencies,
        "variant": asdict(variant),
        "variant_id": variant.id,
        "source_config": str(source_config),
        "config": str(config_path),
        "run_dir": str(run_dir),
        "config_sha256": sha256_bytes(config.encode("utf-8")),
        "solve": solve_meta,
        "evaluation": evaluation_meta,
        "evaluation_solver": str(evaluation_solver or solver),
        "evaluation_solver_sha256": file_sha256(evaluation_solver or solver)
        if isinstance(evaluation_solver or solver, Path)
        else None,
        "inspect": inspect_meta,
        "run_result": run_result,
        "events": event_info,
        "timing": {
            "wall_seconds": solve_meta["wall_seconds"],
            "abstraction_build_seconds": event_info["abstraction_build_seconds"],
            "solver_reported_elapsed_seconds": (run_result or {}).get("elapsedSecs"),
            "heldout_evaluation_seconds": (evaluation_meta or {}).get("wall_seconds")
            if evaluation_meta
            else None,
            # The current CLI includes in-solve profile evaluation in
            # elapsedSecs and exposes no training-only timer.  Keep this
            # explicit instead of relabelling a mixed measurement.
            "training_seconds": None,
            "training_seconds_status": "unavailable-current-cli-mixed-solve-elapsed",
            "inspect_seconds": (inspect_meta or {}).get("wall_seconds") if inspect_meta else None,
        },
    }
    _write_json(variant_root / "summary.json", summary)
    return summary


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("config", type=Path, help="production v1 TOML input")
    parser.add_argument("--output-root", type=Path, default=Path("runs/benchmarks/multiway-convergence-bench"))
    parser.add_argument("--solver", help="solvers executable; defaults to target/release, then target/debug")
    parser.add_argument(
        "--evaluation-solver",
        help="optional evaluator executable; defaults to --solver (or the discovered solver)",
    )
    parser.add_argument("--cache-dir", type=Path, help="machine-scoped EHS2 cache directory")
    parser.add_argument("--seeds", default=','.join(map(str, DEFAULT_SEEDS)))
    parser.add_argument("--solver-kinds", default=','.join(DEFAULT_KINDS))
    parser.add_argument("--pruning", default=','.join(DEFAULT_PRUNING))
    parser.add_argument("--batches", default=','.join(map(str, DEFAULT_BATCHES)))
    parser.add_argument("--discounts", default=','.join(DEFAULT_DISCOUNTS), help="config, none, or cadence values")
    parser.add_argument("--sweeps", type=int, default=4096)
    parser.add_argument("--max-time", help="optional fixed time ceiling, e.g. 30s or 2m")
    parser.add_argument("--evaluation-samples", type=int, default=DEFAULT_EVAL_SAMPLES)
    parser.add_argument("--evaluation-seed", type=int, default=424242)
    parser.add_argument("--br-traversals", type=int, default=DEFAULT_BR_TRAVERSALS)
    parser.add_argument("--timeout-seconds", type=float, default=DEFAULT_TIMEOUT)
    parser.add_argument("--no-inspect", action="store_true")
    parser.add_argument("--dry-run", action="store_true", help="print the plan without running solver commands")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if not args.config.is_file():
            raise ValueError(f"config does not exist: {args.config}")
        seeds = parse_int_list(args.seeds, name="seeds")
        kinds = parse_csv_values(args.solver_kinds, name="solver-kinds")
        pruning = parse_csv_values(args.pruning, name="pruning")
        batches = parse_int_list(args.batches, name="batches", positive=True)
        discounts = parse_csv_values(args.discounts, name="discounts")
        variants, invalid = variants_from_args(seeds, kinds, pruning, batches, discounts)
        if args.sweeps <= 0 or args.sweeps >= MAX_U64:
            raise ValueError("sweeps must be positive and less than 2^64-1")
        if args.evaluation_samples <= 0 or args.br_traversals <= 0:
            raise ValueError("evaluation-samples and br-traversals must be positive")
        if args.timeout_seconds <= 0:
            raise ValueError("timeout-seconds must be positive")
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    source = args.config.read_bytes()
    solver = find_solver(args.solver)
    evaluation_solver = find_solver(args.evaluation_solver) if args.evaluation_solver else solver
    solver_metadata = {
        "path": str(solver),
        "sha256": file_sha256(solver) if isinstance(solver, Path) else None,
        "git_revision": git_revision(),
    }
    plan = {
        "schema": TOOL_SCHEMA,
        "tool": "multiway_convergence_bench.py",
        "source_config": str(args.config),
        "source_config_sha256": sha256_bytes(source),
        "solver": str(solver),
        "solver_metadata": solver_metadata,
        "evaluation_solver": str(evaluation_solver),
        "evaluation_solver_metadata": {
            "path": str(evaluation_solver),
            "sha256": file_sha256(evaluation_solver)
            if isinstance(evaluation_solver, Path)
            else None,
        },
        "cache_dir": str(args.cache_dir) if args.cache_dir else None,
        "budget": {"sweeps": args.sweeps, "max_time": args.max_time, "stop_disabled_by_cadence": True},
        "evaluation": {
            "samples": args.evaluation_samples,
            "seed": args.evaluation_seed,
            "br_traversals": args.br_traversals,
        },
        "artifact_evaluation_note": ARTIFACT_EVALUATION_NOTE,
        "external_tree_source": external_tree_source_reference(
            args.config.read_text(encoding="utf-8")
        ),
        "evaluation_profile_equivalence": evaluation_profile_equivalence(
            args.config.read_text(encoding="utf-8")
        ),
        "variants": [asdict(item) | {"variant_id": item.id} for item in variants],
        "invalid_combinations": invalid,
    }
    if args.dry_run:
        print(json.dumps(plan, indent=2, sort_keys=True))
        return 0

    args.output_root.mkdir(parents=True, exist_ok=True)
    try:
        check_output_collisions(args.output_root, variants)
    except (OSError, FileExistsError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    _write_json(args.output_root / "plan.json", plan)
    summaries: list[dict[str, Any]] = []
    for variant in variants:
        try:
            summary = _run_one(
                solver,
                args.cache_dir,
                args.config,
                args.output_root,
                variant,
                sweeps=args.sweeps,
                max_time=args.max_time,
                eval_samples=args.evaluation_samples,
                eval_seed=args.evaluation_seed,
                br_traversals=args.br_traversals,
                timeout=args.timeout_seconds,
                inspect=not args.no_inspect,
                evaluation_solver=evaluation_solver,
            )
        except (OSError, ValueError, FileExistsError) as exc:
            summary = {"schema": TOOL_SCHEMA, "variant_id": variant.id, "status": "runner-error", "error": str(exc)}
        summaries.append(summary)
        _write_json(args.output_root / "summary.json", {"plan": plan, "summaries": summaries})
    return 0 if all(item.get("status") == "ok" for item in summaries) else 1


if __name__ == "__main__":
    raise SystemExit(main())
