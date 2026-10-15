#!/bin/sh
set -eu
CDPATH= cd -P "$(dirname "$0")"
exec ./solversd --open --runs runs
