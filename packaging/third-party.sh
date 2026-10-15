#!/usr/bin/env sh
# Generate release notices without compiling. Requires Cargo and Python 3.11+.
# No tools/npm packages are installed; exact npm license archives are fetched
# with lockfile integrity checks (cached under .cache/third-party/npm).
# Usage: sh packaging/third-party.sh [--output PATH] [--offline] [--check]
# PYTHON may name a Python executable; paths containing spaces are supported.
# Generate on Windows: Cargo resolves build-dependencies for the host, and the
# Windows host graph is the superset (release.yml checks it on windows-latest).
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -n "${PYTHON:-}" ]; then
    exec "$PYTHON" "$root/packaging/third_party.py" "$@"
elif command -v python3 >/dev/null 2>&1 && python3 -c 'import tomllib' >/dev/null 2>&1; then
    exec python3 "$root/packaging/third_party.py" "$@"
elif command -v python >/dev/null 2>&1 && python -c 'import tomllib' >/dev/null 2>&1; then
    exec python "$root/packaging/third_party.py" "$@"
elif command -v py >/dev/null 2>&1 && py -3 -c 'import tomllib' >/dev/null 2>&1; then
    exec py -3 "$root/packaging/third_party.py" "$@"
fi
printf '%s\n' 'third-party: Python 3.11+ is required (or set PYTHON to its executable).' >&2
exit 1
