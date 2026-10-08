#!/usr/bin/env bash
# J's paired timing on a GCP Spot VM (c2d-highcpu-32, deleted after 2 hours at most) once the project's vCPU quota
# (32 in all regions) is free: no instance in the project, checked every 5 minutes. Results go to
# .cache/p2-trunk/l1-6max/gcp-j/; the VM is deleted at the end.
set -uo pipefail
cd /c/Users/PC_User/orca/workspaces/solvers/hind
v=/c/Users/PC_User/AppData/Local/Temp/claude/C--Users-PC-User-orca-workspaces-solvers-hind/38698140-7a89-40cb-9cf0-0677fcd937cb/scratchpad/vm
out=.cache/p2-trunk/l1-6max/gcp-j
mkdir -p $out
name=p2perf-1
until [ -z "$(gcloud compute instances list --format='value(name)' 2>/dev/null)" ]; do sleep 300; done
echo "quota free $(date -Iseconds)"
git archive --format=tar 46060c6 Cargo.toml Cargo.lock crates examples .cargo > $v/src-i.tar
git archive --format=tar 7b8d704 Cargo.toml Cargo.lock crates examples .cargo > $v/src-j.tar
zone=
for z in europe-west4-b europe-west4-a europe-west4-c; do
  if gcloud compute instances create $name --zone=$z --machine-type=c2d-highcpu-32 --provisioning-model=SPOT \
      --instance-termination-action=DELETE --max-run-duration=2h --image-family=ubuntu-2404-lts-amd64 \
      --image-project=ubuntu-os-cloud --boot-disk-size=30GB >> $out/create.log 2>&1; then
    zone=$z
    break
  fi
done
[ -n "$zone" ] || { echo "create failed"; tail -5 $out/create.log; exit 1; }
echo "created $name in $zone $(date -Iseconds)"
for k in $(seq 1 30); do
  gcloud compute ssh $name --zone=$zone --command=true > $out/ssh-wait.log 2>&1 && break
  sleep 15
done
gcloud compute scp --zone=$zone $v/src-i.tar $v/src-j.tar $v/setup_j.sh $v/run_j.sh $name: > $out/scp.log 2>&1 \
  || { echo "scp failed"; tail -5 $out/scp.log; }
gcloud compute ssh $name --zone=$zone --command='nohup bash ~/setup_j.sh > ~/setup.log 2>&1 < /dev/null &' \
  > $out/start.log 2>&1
echo "started $(date -Iseconds)"
while true; do
  sleep 180
  if gcloud compute ssh $name --zone=$zone \
      --command='tar -czf ~/res.tgz -C ~ results setup.log 2>/dev/null; tail -1 ~/results/progress.txt' \
      > $out/ssh.log 2>&1; then
    gcloud compute scp --zone=$zone $name:res.tgz $out/res.tgz > /dev/null 2>&1 && tar -xzf $out/res.tgz -C $out
    echo "$(date +%T) $(tail -1 $out/ssh.log)"
    grep -q ALL_DONE $out/results/progress.txt 2>/dev/null && break
  elif ! gcloud compute instances list --format='value(name)' 2>/dev/null | grep -q "^$name$"; then
    echo "VM gone $(date -Iseconds)"
    break
  fi
done
gcloud compute instances delete $name --zone=$zone --quiet > $out/delete.log 2>&1
echo "deleted $(date -Iseconds)"
