"""Read both v2 artifacts with the baseline CLI and compare root exports."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def without_wall(value):
    if isinstance(value, dict):
        return {key: without_wall(item) for key, item in value.items() if "wall" not in key.lower()}
    if isinstance(value, list):
        return [without_wall(item) for item in value]
    return value


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("baseline_binary")
    parser.add_argument("old_solution")
    parser.add_argument("new_solution")
    parser.add_argument("result")
    args = parser.parse_args()
    result = {}
    for view in ("summary", "strategy", "ev"):
        outputs = []
        for path in (args.old_solution, args.new_solution):
            command = [str(Path(args.baseline_binary).resolve()), "export", path, view, "--format", "json"]
            output = subprocess.run(command, check=True, capture_output=True).stdout
            if view == "summary":
                output = json.dumps(without_wall(json.loads(output)), sort_keys=True, separators=(",", ":")).encode()
            outputs.append(output)
        if outputs[0] != outputs[1]:
            raise AssertionError(f"{view} differs")
        result[view] = {"equal": True, "compared_bytes": len(outputs[0]), "sha256": hashlib.sha256(outputs[0]).hexdigest(), "excluded_fields": ["wall_secs"] if view == "summary" else []}
    Path(args.result).write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
