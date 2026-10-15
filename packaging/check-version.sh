#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    printf 'Usage: %s <tag>\n' "$0" >&2
    exit 1
fi

root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
version=$(awk '
    /^\[workspace\.package\]$/ { workspace_package = 1; next }
    /^\[/ { workspace_package = 0 }
    workspace_package && /^[[:space:]]*version[[:space:]]*=/ {
        value = $3
        gsub(/"/, "", value)
        print value
        exit
    }
' "$root/Cargo.toml")

if [ -z "$version" ]; then
    printf 'Cannot read workspace version from %s/Cargo.toml\n' "$root" >&2
    exit 1
fi
if [ "$1" != "v$version" ]; then
    printf 'Release tag %s does not match workspace version v%s\n' "$1" "$version" >&2
    exit 1
fi
printf 'Release tag %s matches workspace version %s\n' "$1" "$version"
