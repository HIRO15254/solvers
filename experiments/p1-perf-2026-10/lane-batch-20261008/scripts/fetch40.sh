#!/bin/bash
# Pull VM9 results every 4 minutes until the marker given as $1 appears or the VM disappears.
M=${1:-VM41_ALL_DONE}
D=/c/Users/PC_User/orca/workspaces/solvers/cisco/runs/p1-perf/vm9
Z=europe-west4-b
cd $D
while true; do
  if gcloud compute ssh p1perf-9 --zone=$Z --command='tar -czf ~/res.tgz -C ~ results setup40.log 2>/dev/null; tail -1 ~/results/progress.txt 2>/dev/null; tail -2 ~/setup40.log' > fetch_ssh.log 2>&1; then
    gcloud compute scp --zone=$Z p1perf-9:res.tgz $D/res.tgz > fetch_scp.log 2>&1 && tar -xzf res.tgz 2>/dev/null
    echo "$(date -u +%T) $(tail -3 fetch_ssh.log | tr '\n' ' ')" >> fetch.log
    if grep -q "$M" results/progress.txt 2>/dev/null; then echo "$M"; break; fi
  else
    echo "$(date -u +%T) ssh failed" >> fetch.log
    if ! gcloud compute instances list --format='value(name)' 2>/dev/null | grep -q p1perf-9; then echo VM_GONE; break; fi
  fi
  sleep 240
done
