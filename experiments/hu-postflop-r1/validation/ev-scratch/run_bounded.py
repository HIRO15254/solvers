"""Finite Windows validation with a 1 GiB Job cap and 1 GiB commit reserve.

Uses the exact previously calibrated supervisor transformation, with explicit
larger compile limits. Fresh available commit must be at least 2 GiB; each
process is still created suspended, assigned, queried and resumed below normal.
The 512 MiB experiment wrapper and all historical evidence remain unchanged.
This wrapper does not imply that a workload will fit or that time is isolated.
"""
from __future__ import annotations

import hashlib
import importlib.util
import os
from pathlib import Path
import sys

HERE = Path(__file__).resolve()
ROOT = HERE.parents[4]
TRANSFORMER = ROOT / "experiments/hu-postflop-r1/flop-scaling/native-preflight/run_bounded.py"
TRANSFORMER_SHA = "224e91f60ae572f3221196b2d8c76dfadfd4db412cdb67f767b89ad8cd7cb9b8"
JOB_COMMIT_BYTES = 1024 * 1024 * 1024
MIN_AVAILABLE_COMMIT_BYTES = 2 * JOB_COMMIT_BYTES


def main() -> int:
    if os.name != "nt":
        raise RuntimeError("Windows validation wrapper")
    if hashlib.sha256(TRANSFORMER.read_bytes()).hexdigest() != TRANSFORMER_SHA:
        raise ValueError("Pinned supervisor transformation changed")
    spec = importlib.util.spec_from_file_location("r1_fixed_transform", TRANSFORMER)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    source = module.transformed_source(module.BASE.read_bytes())
    namespace = {
        "__name__": "_r1_validation_supervisor", "__file__": str(module.BASE),
        "BOUND_JOB_FLAGS": module.JOB_FLAGS,
        "BOUND_JOB_COMMIT_BYTES": JOB_COMMIT_BYTES,
        "BOUND_MIN_COMMIT_BYTES": MIN_AVAILABLE_COMMIT_BYTES,
        "BOUND_BELOW_NORMAL": module.BELOW_NORMAL,
        "BOUND_PROVENANCE": {
            "wrapper_path": str(HERE), "wrapper_sha256": hashlib.sha256(HERE.read_bytes()).hexdigest(),
            "transformer_path": str(TRANSFORMER), "transformer_sha256": TRANSFORMER_SHA,
            "base_path": str(module.BASE), "base_sha256": module.BASE_SHA256,
            "transformed_source_sha256": hashlib.sha256(source.encode()).hexdigest(),
            "scope": "1 GiB aggregate Job commit, fresh 2 GiB host available commit, below-normal root",
        },
    }
    exec(compile(source, str(module.BASE) + " [bounded validation]", "exec"), namespace)
    return namespace["main"](["--identity-file", str(HERE), "--identity-file", str(TRANSFORMER), *sys.argv[1:]])


if __name__ == "__main__":
    raise SystemExit(main())
