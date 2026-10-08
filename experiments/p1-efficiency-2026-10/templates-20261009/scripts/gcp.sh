#!/bin/bash
# Creates a c2d-highcpu-32 Spot VM, runs setup.sh, fetches results, deletes the VM.
# usage: gcp.sh SRC_REV  (run from the repository root)
set -u
rev=$1; name=p1eff-1; camp=experiments/p1-efficiency-2026-10/templates-20261009
out=runs/p1eff/gcp-templates; mkdir -p $out
git archive --format=tar $rev Cargo.toml Cargo.lock crates examples .cargo > $out/src.tar
tar -czf $out/cfg.tgz -C $camp/configs .
cp $camp/scripts/setup.sh $out/setup.sh
# The project has a global 32-vCPU quota: wait until no other VM runs.
until [ -z "$(gcloud compute instances list --format='value(name)' 2>/dev/null)" ]; do sleep 300; done
echo "quota free $(date -Iseconds)"
zone=""
for z in us-central1-a us-central1-b us-central1-c europe-west1-b europe-west4-a europe-west4-c; do
  if gcloud compute instances create $name --zone=$z --machine-type=c2d-highcpu-32 --provisioning-model=SPOT \
      --instance-termination-action=DELETE --max-run-duration=3h --image-family=ubuntu-2404-lts-amd64 \
      --image-project=ubuntu-os-cloud --boot-disk-size=30GB >> $out/create.log 2>&1; then
    zone=$z; break
  fi
done
[ -z "$zone" ] && { echo "create failed"; tail -5 $out/create.log; exit 1; }
echo "created $zone $(date -Iseconds)"
for k in $(seq 1 30); do
  gcloud compute ssh $name --zone=$zone --command=true > $out/ssh-wait.log 2>&1 && break
  sleep 15
done
(cd $out && gcloud compute scp --zone=$zone src.tar cfg.tgz setup.sh $name: > scp.log 2>&1) || { echo "scp failed"; tail -5 $out/scp.log; }
gcloud compute ssh $name --zone=$zone --command='nohup bash ~/setup.sh > ~/setup.log 2>&1 < /dev/null &' > $out/start.log 2>&1
echo "started $(date -Iseconds)"
while true; do
  sleep 180
  if gcloud compute ssh $name --zone=$zone \
      --command='tar -czf ~/res.tgz -C ~ results setup.log 2>/dev/null; tail -1 ~/results/progress.txt' > $out/ssh.log 2>&1; then
    (cd $out && gcloud compute scp --zone=$zone $name:res.tgz res.tgz > /dev/null 2>&1 && tar -xzf res.tgz)
    echo "$(date +%T) $(tail -1 $out/ssh.log)"
    grep -q ALL_DONE $out/results/progress.txt 2>/dev/null && break
  elif ! gcloud compute instances list --format='value(name)' 2>/dev/null | grep -q "^$name$"; then
    echo "VM gone $(date -Iseconds)"; break
  fi
done
gcloud compute instances delete $name --zone=$zone --quiet > $out/delete.log 2>&1
echo "deleted $(date -Iseconds)"
