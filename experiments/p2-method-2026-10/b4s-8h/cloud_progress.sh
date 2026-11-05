# Terminal progress markers shared by the AWS/GCP experiment runners.
job_terminal() {
  grep -Eq '(^|[[:space:]])(ALL_DONE|JOB_FAILED)([[:space:]]|$)' "$1" 2>/dev/null
}
job_failed() {
  grep -Eq '(^|[[:space:]])JOB_FAILED([[:space:]]|$)' "$1" 2>/dev/null
}
# Spot market rule of the AWS runner (aws_run.sh). Spot interrupting a running instance before it has run
# SPOT_QUICK_SECONDS (default 1800) is a quick reclaim; a running period reaching that length is ok and resets the
# region's streak. Quick reclaims within SPOT_EVENT_MERGE_SECONDS (default 120) of the first one of an event are one
# event (one AWS reclaim of several parallel runs). A region with SPOT_BLOCK_STREAK (default 3) or more consecutive
# quick reclaim events is not used for SPOT_BLOCK_SECONDS (default 28800 = 8 h) after the last of them.
# Reclaim log lines (appended in time order):
# EPOCH ISO region=R az=AZ type=T kind=quick|ok ran_s=N name=NAME instance=ID
spot_blocked_until() { # LOG REGION -> epoch until which REGION is blocked (0 = usable)
  [[ -f $1 ]] || { echo 0; return; }
  awk -v region="region=$2" -v streak_min="${SPOT_BLOCK_STREAK:-3}" -v block="${SPOT_BLOCK_SECONDS:-28800}" \
    -v merge="${SPOT_EVENT_MERGE_SECONDS:-120}" '
    $3 == region && $6 == "kind=ok" { streak = 0 }
    $3 == region && $6 == "kind=quick" { if (!(streak && $1 - first <= merge)) { streak++; first = $1 } last = $1 }
    END { print (streak >= streak_min ? last + block : 0) }' "$1"
}
