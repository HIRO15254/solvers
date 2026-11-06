#!/usr/bin/env bash
# Poll a job VM every 4 minutes, download ~/results into <out>/results, delete the VM after ALL_DONE or when it is gone.
# usage: gcp_watch.sh <name> <zone> <local out dir>
set -uo pipefail
name=$1; zone=$2; out=$3
while true; do
  sleep 240
  if gcloud compute ssh "$name" --zone=$zone \
      --command='tar -czf ~/res.tgz -C ~ results 2>/dev/null; tail -1 ~/results/progress.txt' > "$out/ssh.log" 2>&1; then
    gcloud compute scp --zone=$zone "$name":res.tgz "$out/res.tgz" > /dev/null 2>&1 && tar --force-local -xzf "$out/res.tgz" -C "$out"
    echo "$(date +%T) $(tail -1 "$out/ssh.log")" >> "$out/vm.log"
    grep -q ALL_DONE "$out/results/progress.txt" 2>/dev/null && break
  elif ! gcloud compute instances list --format='value(name)' 2>/dev/null | grep -q "^$name$"; then
    echo "VM gone $(date -Iseconds)" | tee -a "$out/vm.log"
    break
  fi
done
gcloud compute instances delete "$name" --zone=$zone --quiet > "$out/delete.log" 2>&1
echo "deleted $(date -Iseconds)" | tee -a "$out/vm.log"
