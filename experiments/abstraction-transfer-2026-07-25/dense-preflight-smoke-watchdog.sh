#!/usr/bin/env bash
set -euo pipefail

(( $# >= 5 )) || exit 64
limit_bytes=$1
metadata=$2
stdout_log=$3
stderr_log=$4
shift 4

set +e
"$@" >"$stdout_log" 2>"$stderr_log"
child_status=$?
set -e
rss_limit_exceeded=0
watchdog_status=$child_status
if [[ "$child_status" == "130" ]]; then
  rss_limit_exceeded=1
  watchdog_status=75
fi
{
  echo "pid=$$"
  echo "exit_code=$child_status"
  echo "wall_seconds=0"
  echo "peak_rss_bytes=1048576"
  echo "rss_limit_bytes=$limit_bytes"
  echo "rss_limit_exceeded=$rss_limit_exceeded"
  echo "rss_samples=1"
  echo "rss_monitor_error=0"
  echo "forwarded_signal=none"
} >"$metadata"
exit "$watchdog_status"
