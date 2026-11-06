#!/usr/bin/env bash
# Run a job on one GCP Spot c2d-highcpu-32 VM and delete it afterwards.
# usage: gcp_run.sh <name> <max hours> <local out dir> <remote job script> [files to upload...]
# The working tree's sources (crates, examples, Cargo.*, .cargo) are uploaded as src.tar. The remote job script
# runs as ~/job.sh, writes into ~/results and appends ALL_DONE to ~/results/progress.txt when finished.
set -uo pipefail
name=$1; hours=$2; out=$3; job=$4; shift 4
repo=/c/Users/PC_User/orca/workspaces/solvers/hind
mkdir -p "$out"
tmp=$(mktemp -d)
tar -cf "$tmp/src.tar" -C "$repo" Cargo.toml Cargo.lock crates examples .cargo
cp "$job" "$tmp/job.sh"
for f in "$@"; do cp "$f" "$tmp/"; done
zone=
for z in europe-west4-b europe-west4-a europe-west4-c us-central1-a us-central1-b us-central1-c; do
  if gcloud compute instances create "$name" --zone=$z --machine-type=c2d-highcpu-32 --provisioning-model=SPOT \
      --instance-termination-action=DELETE --max-run-duration=${hours}h --image-family=ubuntu-2404-lts-amd64 \
      --image-project=ubuntu-os-cloud --boot-disk-size=40GB >> "$out/create.log" 2>&1; then
    zone=$z; break
  fi
done
[ -n "$zone" ] || { echo "create failed"; tail -5 "$out/create.log"; exit 1; }
echo "created $name in $zone $(date -Iseconds)" | tee -a "$out/vm.log"
for k in $(seq 1 30); do
  gcloud compute ssh "$name" --zone=$zone --command=true > "$out/ssh-wait.log" 2>&1 && break
  sleep 15
done
(cd "$tmp" && gcloud compute scp --zone=$zone ./* "$name": > "$out/scp.log" 2>&1) || { echo "scp failed"; tail -5 "$out/scp.log"; }
gcloud compute ssh "$name" --zone=$zone --command='mkdir -p ~/results; nohup bash ~/job.sh > ~/results/job.log 2>&1 < /dev/null &' \
  > "$out/start.log" 2>&1
echo "started $(date -Iseconds)" | tee -a "$out/vm.log"
while true; do
  sleep 240
  if gcloud compute ssh "$name" --zone=$zone \
      --command='tar -czf ~/res.tgz -C ~ results 2>/dev/null; tail -1 ~/results/progress.txt' > "$out/ssh.log" 2>&1; then
    gcloud compute scp --zone=$zone "$name":res.tgz "$out/res.tgz" > /dev/null 2>&1 && tar -xzf "$out/res.tgz" -C "$out"
    echo "$(date +%T) $(tail -1 "$out/ssh.log")" >> "$out/vm.log"
    grep -q ALL_DONE "$out/results/progress.txt" 2>/dev/null && break
  elif ! gcloud compute instances list --format='value(name)' 2>/dev/null | grep -q "^$name$"; then
    echo "VM gone $(date -Iseconds)" | tee -a "$out/vm.log"
    break
  fi
done
gcloud compute instances delete "$name" --zone=$zone --quiet > "$out/delete.log" 2>&1
echo "deleted $(date -Iseconds)" | tee -a "$out/vm.log"
rm -rf "$tmp"
