"""Generate the same bounded timing adapter for both EV and flat-EV libraries."""
import difflib
import hashlib
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
FLOP = HERE.parents[1]
SOURCE = FLOP / "native-solve/solve.rs"
SOURCE_SHA = "547ed17705b7784ac656fb8032d859261b9a4639fa49dc0ba4b105cdfe2addd9"
REPLACEMENTS = [
    ("1|2 1|2 NEW_OUTPUT_DIRECTORY", "1|2|4|8|16 1..128 NEW_OUTPUT_DIRECTORY", 2),
    ('assert!((1..=2).contains(&threads), "worker limit is 2");',
     'assert!([1, 2, 4, 8, 16].contains(&threads), "workers");', 1),
    ('assert!((1..=2).contains(&iterations), "iteration limit is 2");',
     'assert!((1..=128).contains(&iterations), "iteration limit is 128");', 1),
]


def pin(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def main():
    original = SOURCE.read_bytes()
    if pin(original)["sha256"] != SOURCE_SHA:
        raise ValueError("Original native adapter differs")
    source = original.decode("utf-8")
    for old, new, count in REPLACEMENTS:
        if source.count(old) != count:
            raise ValueError("Adapter bound anchor differs")
        source = source.replace(old, new)
    formatted = subprocess.run(["rustfmt", "--edition", "2024", "--emit", "stdout"],
                               input=source.encode(), capture_output=True, check=True, timeout=30).stdout
    restored = formatted.decode("utf-8")
    for old, new, count in reversed(REPLACEMENTS):
        if restored.count(new) != count:
            raise ValueError("Formatted bound changed unexpectedly")
        restored = restored.replace(new, old)
    if restored.encode() != original:
        raise ValueError("Changes outside argument bounds/usage")
    patch = "".join(difflib.unified_diff(original.decode().splitlines(True), formatted.decode().splitlines(True),
                                        fromfile="native-solve/solve.rs", tofile="timing/solve.rs")).encode()
    outputs = {"solve.rs": formatted, "adapter.patch": patch}
    for name, data in outputs.items():
        path = HERE / name
        if path.exists() and path.read_bytes() != data:
            raise ValueError(f"Refusing to overwrite different output: {name}")
    for name, data in outputs.items():
        (HERE / name).write_bytes(data)
    provenance = {"source": pin(original), "preparer": pin(Path(__file__).read_bytes()),
                  "generated": {name: pin(data) for name, data in outputs.items()},
                  "formatter": subprocess.check_output(["rustfmt", "--version"], text=True).strip(),
                  "outside_bounds_and_usage_byte_identical": True,
                  "scope": "same fixed-iteration adapter; extended bounds only; no solver or performance execution"}
    (HERE / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(provenance))


if __name__ == "__main__":
    main()
