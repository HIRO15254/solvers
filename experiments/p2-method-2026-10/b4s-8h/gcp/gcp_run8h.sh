#!/usr/bin/env bash
# Run a resumable job on one GCP Spot c2d-highcpu-32 VM that is stopped (not deleted) on preemption, restart it
# after a preemption, and delete it when the job is done or the deadline has passed.
# usage: gcp_run8h.sh <name> <deadline hours> <local out dir> <remote job script> [files to upload to ~...]
# The job runs as ~/job.sh, writes small files into ~/results (downloaded every 5 minutes), appends ALL_DONE to
# progress.txt after final.tgz is ready, or JOB_FAILED on solver failure. Either marker deletes the VM immediately.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../cloud_progress.sh"
name=$1; hours=$2; out=$3; job=$4; shift 4
repo=/c/Users/PC_User/orca/workspaces/solvers/hind
mkdir -p "$out"
out=$(cd "$out" && pwd)
tmp=$(mktemp -d)
tar -cf "$tmp/src.tar" -C "$repo" Cargo.toml Cargo.lock crates examples .cargo
cp "$job" "$tmp/job.sh"
for f in "$@"; do cp "$f" "$tmp/"; done
# ZONE=<zone> reuses an existing VM of that name; with WATCH=1 as well, the job already running there is only
# watched (no upload or launch), e.g. after the runner itself was interrupted.
# While the project's 32-vCPU quota is taken by another VM, creation is retried every 5 minutes for up to
# QUOTA_WAIT_HOURS (default 6).
zone=${ZONE:-}
give_up=$(( $(date +%s) + ${QUOTA_WAIT_HOURS:-6} * 3600 ))
while [ -z "$zone" ]; do
  for z in europe-west4-b europe-west4-a europe-west4-c us-central1-a us-central1-b us-central1-c; do
    if gcloud compute instances create "$name" --zone=$z --machine-type=c2d-highcpu-32 --provisioning-model=SPOT \
        --instance-termination-action=STOP --image-family=ubuntu-2404-lts-amd64 \
        --image-project=ubuntu-os-cloud --boot-disk-size=60GB >> "$out/create.log" 2>&1; then
      zone=$z; break
    fi
    tail -3 "$out/create.log" | grep -q CPUS_ALL_REGIONS && break
  done
  [ -n "$zone" ] && break
  [ "$(date +%s)" -lt "$give_up" ] || { echo "create failed"; tail -5 "$out/create.log"; exit 1; }
  sleep 300
done
deadline=$(( $(date +%s) + hours * 3600 ))
echo "using $name in $zone $(date -Iseconds)" | tee -a "$out/vm.log"
wait_ssh() {
  for k in $(seq 1 40); do
    gcloud compute ssh "$name" --zone=$zone --command=true > "$out/ssh-wait.log" 2>&1 && return 0
    sleep 15
  done
  return 1
}
launch() {
  gcloud compute ssh "$name" --zone=$zone \
    --command='mkdir -p ~/results; nohup bash ~/job.sh >> ~/results/job.log 2>&1 < /dev/null &' > "$out/start.log" 2>&1
  echo "launched $(date -Iseconds)" | tee -a "$out/vm.log"
}
if [ -z "${WATCH:-}" ]; then
  wait_ssh
  (cd "$tmp" && gcloud compute scp --zone=$zone ./* "$name": > "$out/scp.log" 2>&1) || { echo "scp failed"; tail -5 "$out/scp.log"; }
  launch
fi
rm -rf "$tmp"
exit_status=0
while true; do
  sleep 300
  status=$(gcloud compute instances describe "$name" --zone=$zone --format='value(status)' 2>/dev/null)
  if [ -z "$status" ]; then
    echo "VM gone $(date -Iseconds)" | tee -a "$out/vm.log"; exit 1
  fi
  if [ "$(date +%s)" -ge "$deadline" ]; then
    echo "deadline passed $(date -Iseconds)" | tee -a "$out/vm.log"; break
  fi
  if [ "$status" = TERMINATED ] || [ "$status" = STOPPED ]; then
    echo "$(date -Iseconds) status $status: restarting" | tee -a "$out/vm.log"
    gcloud compute instances start "$name" --zone=$zone >> "$out/restart.log" 2>&1 && wait_ssh && launch
    continue
  fi
  [ "$status" = RUNNING ] || continue
  if gcloud compute ssh "$name" --zone=$zone \
      --command='tar -czf ~/res.tgz -C ~ results 2>/dev/null; tail -1 ~/results/progress.txt' > "$out/ssh.log" 2>&1; then
    gcloud compute scp --zone=$zone "$name":res.tgz "$out/res.tgz" > /dev/null 2>&1 && tar --force-local -xzf "$out/res.tgz" -C "$out"
    echo "$(date +%T) $(tail -1 "$out/ssh.log")" >> "$out/vm.log"
    if job_terminal "$out/results/progress.txt"; then
      if job_failed "$out/results/progress.txt"; then
        echo "JOB_FAILED $(date -Iseconds)" | tee -a "$out/vm.log"
        exit_status=1
      else
        gcloud compute scp --zone=$zone "$name":final.tgz "$out/final.tgz" > "$out/final-scp.log" 2>&1 || exit_status=1
        echo "final downloaded $(date -Iseconds) $exit_status" | tee -a "$out/vm.log"
      fi
      break
    fi
  fi
done
gcloud compute instances delete "$name" --zone=$zone --quiet > "$out/delete.log" 2>&1 || exit_status=1
echo "deleted $(date -Iseconds)" | tee -a "$out/vm.log"
exit "$exit_status"
