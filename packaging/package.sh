#!/usr/bin/env sh
# Assemble one release archive from built binaries and the built Web GUI.
#
#   packaging/package.sh <version> <target> <bin-dir> <ui-dir> <out-dir> [<suffix>]
#
# <bin-dir> holds solvers/solversd (".exe" on Windows targets). The archive is
# solvers-<version>-<target>[-<suffix>].zip on Windows targets and .tar.gz
# elsewhere, with the layout in docs/plans/release-v0.1.jp.md. <suffix> names
# a non-default build such as the x86-64-v2 `compat` archive. Prints the
# archive path.
set -eu

if [ "$#" -lt 5 ] || [ "$#" -gt 6 ]; then
    echo "usage: $0 <version> <target> <bin-dir> <ui-dir> <out-dir> [<suffix>]" >&2
    exit 2
fi
version=$1
target=$2
bin_dir=$3
ui_dir=$4
out_dir=$5
suffix=${6:-}
root=$(cd "$(dirname "$0")/.." && pwd)

case "$target" in
    *windows*) exe=.exe ;;
    *) exe= ;;
esac

name="solvers-$version-$target${suffix:+-$suffix}"
stage="$out_dir/$name"
rm -rf "$stage"
mkdir -p "$stage/ui"

cp "$bin_dir/solvers$exe" "$bin_dir/solversd$exe" "$stage/"
[ -f "$ui_dir/index.html" ] || { echo "missing $ui_dir/index.html" >&2; exit 1; }
cp -R "$ui_dir/." "$stage/ui/"
cp "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$root/CHANGELOG.md" "$stage/"
cp "$root/packaging/README.md" "$stage/README.md"
cp "$root/packaging/README.jp.md" "$stage/README.jp.md"
cp "$root/packaging/THIRD-PARTY-LICENSES.txt" "$stage/"
case "$target" in
    *windows*) cp "$root/packaging/start-gui.cmd" "$stage/" ;;
    *)
        cp "$root/packaging/start-gui.sh" "$stage/"
        chmod 755 "$stage/start-gui.sh" "$stage/solvers" "$stage/solversd"
        ;;
esac

cd "$out_dir"
case "$target" in
    *windows*)
        archive="$name.zip"
        rm -f "$archive"
        if command -v 7z >/dev/null 2>&1; then
            7z a -tzip -bso0 "$archive" "$name" >/dev/null
        else
            powershell -NoProfile -Command "Compress-Archive -Path '$name' -DestinationPath '$archive'"
        fi
        ;;
    *)
        archive="$name.tar.gz"
        tar -czf "$archive" "$name"
        ;;
esac
echo "$out_dir/$archive"
