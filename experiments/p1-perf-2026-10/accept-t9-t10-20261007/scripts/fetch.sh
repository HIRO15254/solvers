#!/bin/bash
# Pull VM5 results every 4 minutes until ALL_DONE or the VM disappears.
D=/c/Users/PC_User/orca/workspaces/solvers/cisco/runs/p1-perf/vm6
cd $D
while true; do
  if gcloud compute ssh p1perf-6 --zone=europe-west4-a --command='tar -czf ~/res.tgz -C ~ results setup6.log run6.log 2>/dev/null; cat ~/results/progress.txt | tail -1' > fetch_ssh.log 2>&1; then
    gcloud compute scp --zone=europe-west4-a p1perf-6:res.tgz $D/res.tgz > fetch_scp.log 2>&1 && tar -xzf res.tgz 2>/dev/null
    echo "$(date -u +%T) $(tail -1 fetch_ssh.log)" >> fetch.log
    if grep -q PHASEF_DONE results/progress.txt 2>/dev/null; then echo PHASEF_DONE; break; fi
  else
    echo "$(date -u +%T) ssh failed" >> fetch.log
    if ! gcloud compute instances list --format='value(name)' 2>/dev/null | grep -q p1perf-6; then echo VM_GONE; break; fi
  fi
  sleep 240
done
