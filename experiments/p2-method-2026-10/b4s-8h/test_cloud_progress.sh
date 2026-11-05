#!/usr/bin/env bash
# Exercise exactly the marker helpers sourced by both cloud runners, without a VM.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/cloud_progress.sh"
fixture=$(mktemp)
trap 'rm -f "$fixture"' EXIT
printf '12:00:00 START rc2\n' > "$fixture"
! job_terminal "$fixture"
! job_failed "$fixture"
printf '12:00:01 JOB_FAILED rc2 1\r\n' >> "$fixture"
job_terminal "$fixture"
job_failed "$fixture"
printf 'ALL_DONE\r\n' > "$fixture"
job_terminal "$fixture"
! job_failed "$fixture"
printf 'NOT_ALL_DONE JOB_FAILED_suffix\n' > "$fixture"
! job_terminal "$fixture"
! job_failed "$fixture"
! job_terminal "$fixture.missing"
! job_failed "$fixture.missing"
reclaims=$(mktemp)
trap 'rm -f "$fixture" "$reclaims"' EXIT
event() { printf '%s 2026-10-15T00:00:00Z region=%s az=%sa type=c6a.8xlarge kind=%s ran_s=60 name=t instance=i-0\n' "$1" "$2" "$2" "$3" >> "$reclaims"; }
[[ $(spot_blocked_until "$reclaims.missing" us-east-2) == 0 ]]
event 1000 us-east-2 quick; event 2000 us-east-2 quick; event 2500 sa-east-1 quick
[[ $(spot_blocked_until "$reclaims" us-east-2) == 0 ]]        # two quick reclaims are not enough
event 3000 us-east-2 quick
[[ $(spot_blocked_until "$reclaims" us-east-2) == 31800 ]]    # third: blocked 8 h after the last one
[[ $(spot_blocked_until "$reclaims" sa-east-1) == 0 ]]        # other regions are unaffected
event 4000 us-east-2 quick
[[ $(spot_blocked_until "$reclaims" us-east-2) == 32800 ]]    # a longer streak extends the block
event 5000 us-east-2 ok; event 6000 us-east-2 quick; event 7000 us-east-2 quick
[[ $(spot_blocked_until "$reclaims" us-east-2) == 0 ]]        # an ok run resets the streak
[[ $(SPOT_BLOCK_STREAK=2 spot_blocked_until "$reclaims" us-east-2) == 35800 ]]
event 7030 us-east-2 quick                                    # a parallel run hit by the same reclaim
[[ $(spot_blocked_until "$reclaims" us-east-2) == 0 ]]        # ... is the same event, not a third one
event 8000 us-east-2 quick
[[ $(spot_blocked_until "$reclaims" us-east-2) == 36800 ]]
echo 'cloud progress: running, failure, success, CRLF and exact marker checks passed'
echo 'spot rule: streak threshold, 8 h block, per-region, extension, reset and parallel-merge checks passed'
