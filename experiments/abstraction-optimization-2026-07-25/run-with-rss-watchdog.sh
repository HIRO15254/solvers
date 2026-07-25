#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 5 ]]; then
  echo "usage: $0 LIMIT_BYTES META_PATH STDOUT_PATH STDERR_PATH COMMAND [ARGS...]" >&2
  exit 2
fi

limit_bytes=$1
meta_path=$2
stdout_path=$3
stderr_path=$4
shift 4

case "$limit_bytes" in
  ''|*[!0-9]*)
    echo "LIMIT_BYTES must be a positive integer" >&2
    exit 2
    ;;
esac
if (( limit_bytes == 0 )); then
  echo "LIMIT_BYTES must be positive" >&2
  exit 2
fi
if [[ "$meta_path" == "$stdout_path" ||
      "$meta_path" == "$stderr_path" ||
      "$stdout_path" == "$stderr_path" ]]; then
  echo "META_PATH, STDOUT_PATH, and STDERR_PATH must be distinct" >&2
  exit 2
fi

mkdir -p \
  "$(dirname "$meta_path")" \
  "$(dirname "$stdout_path")" \
  "$(dirname "$stderr_path")"

started_epoch=$(date +%s)
peak_rss_bytes=0
limit_exceeded=0
monitor_error=0
rss_samples=0
rss_sample_failures=0
signal_epoch=0
term_sent=0
kill_sent=0
forwarded_signal=none
child_pid=
metadata_written=0

write_metadata() {
  local exit_code=$1
  local finished_epoch wall_seconds temporary_meta
  finished_epoch=$(date +%s)
  wall_seconds=$((finished_epoch - started_epoch))
  temporary_meta="${meta_path}.tmp.$$"
  {
    echo "pid=${child_pid:-none}"
    echo "exit_code=$exit_code"
    echo "wall_seconds=$wall_seconds"
    echo "peak_rss_bytes=$peak_rss_bytes"
    echo "rss_limit_bytes=$limit_bytes"
    echo "rss_limit_exceeded=$limit_exceeded"
    echo "rss_samples=$rss_samples"
    echo "rss_monitor_error=$monitor_error"
    echo "forwarded_signal=$forwarded_signal"
  } >"$temporary_meta"
  mv -f "$temporary_meta" "$meta_path"
  metadata_written=1
}

forward_signal() {
  local signal=$1
  forwarded_signal=$signal
  if [[ -n "$child_pid" ]] && kill -0 "$child_pid" 2>/dev/null; then
    kill "-$signal" "$child_pid" 2>/dev/null || true
  fi
}

cleanup() {
  local watchdog_status=$?
  trap - EXIT INT TERM HUP
  if [[ -n "$child_pid" ]] && kill -0 "$child_pid" 2>/dev/null; then
    kill -TERM "$child_pid" 2>/dev/null || true
    set +e
    wait "$child_pid"
    set -e
  fi
  if (( metadata_written == 0 )); then
    write_metadata "$watchdog_status"
  fi
}

trap cleanup EXIT
trap 'forward_signal INT' INT
trap 'forward_signal TERM' TERM
trap 'forward_signal HUP' HUP

"$@" >"$stdout_path" 2>"$stderr_path" &
child_pid=$!

while kill -0 "$child_pid" 2>/dev/null; do
  set +e
  rss_kib=$(ps -p "$child_pid" -o rss= 2>/dev/null)
  ps_status=$?
  set -e
  rss_kib=$(printf '%s' "$rss_kib" | tr -d '[:space:]')
  if [[ "$rss_kib" =~ ^[0-9]+$ ]]; then
    rss_samples=$((rss_samples + 1))
    rss_sample_failures=0
    rss_bytes=$((rss_kib * 1024))
    if (( rss_bytes > peak_rss_bytes )); then
      peak_rss_bytes=$rss_bytes
    fi
    if (( rss_bytes > limit_bytes && limit_exceeded == 0 )); then
      limit_exceeded=1
      signal_epoch=$(date +%s)
      kill -INT "$child_pid" 2>/dev/null || true
    fi
  elif (( ps_status != 0 )) || [[ -n "$rss_kib" ]]; then
    rss_sample_failures=$((rss_sample_failures + 1))
    if (( rss_sample_failures >= 3 && monitor_error == 0 )); then
      monitor_error=1
      signal_epoch=$(date +%s)
      kill -INT "$child_pid" 2>/dev/null || true
    fi
  fi
  if (( limit_exceeded == 1 || monitor_error == 1 )); then
    now_epoch=$(date +%s)
    if (( now_epoch - signal_epoch >= 30 && term_sent == 0 )); then
      term_sent=1
      kill -TERM "$child_pid" 2>/dev/null || true
    fi
    if (( now_epoch - signal_epoch >= 45 && kill_sent == 0 )); then
      kill_sent=1
      kill -KILL "$child_pid" 2>/dev/null || true
    fi
  fi
  sleep 1 || true
done

set +e
wait "$child_pid"
exit_code=$?
set -e
write_metadata "$exit_code"

if (( limit_exceeded == 1 )); then
  exit 75
fi
if (( monitor_error == 1 )); then
  exit 70
fi
exit "$exit_code"
