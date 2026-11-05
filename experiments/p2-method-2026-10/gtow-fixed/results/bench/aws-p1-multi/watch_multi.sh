#!/usr/bin/env bash
# Reattach to p1-multi after the launcher exited on expired credentials: mirror results, fetch final.tgz, then delete.
set -u
export AWS_PAGER="" MSYS_NO_PATHCONV=1
out=../_goal/bench/p1-multi; inst=i-0847ea73b7509486d; req=sir-b31fn6zh; r=us-east-1
ssh_opts=(-i ~/.ssh/p2-aws.pem -o BatchMode=yes -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=15)
log() { echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $*" | tee -a "$out/vm.log"; }
log "reattached watcher instance=$inst"
while :; do
  read -r state ip < <(aws ec2 describe-instances --region $r --instance-ids $inst \
    --query 'Reservations[].Instances[].[State.Name,PublicIpAddress]' --output text | tr -d '\r')
  [[ -n ${state:-} ]] || { log "describe failed"; sleep 60; continue; }
  if [[ $state == running && $ip != None ]]; then
    log "instance=$inst status=running ip=$ip"
    if ssh "${ssh_opts[@]}" ubuntu@$ip 'tar -czf - results' > "$out/res.tgz.tmp" 2>> "$out/ssh.log" && gzip -t "$out/res.tgz.tmp"; then
      tar -xzf "$out/res.tgz.tmp" -C "$out" && log "downloaded results"
    fi
    if grep -q JOB_FAILED "$out/results/progress.txt"; then log "JOB_FAILED"; break; fi
    if grep -q ALL_DONE "$out/results/progress.txt" &&
       scp "${ssh_opts[@]}" ubuntu@$ip:final.tgz "$out/final.tgz.tmp" >> "$out/scp.log" 2>&1 && gzip -t "$out/final.tgz.tmp"; then
      mv "$out/final.tgz.tmp" "$out/final.tgz"; log "ALL_DONE final downloaded"; break
    fi
  else
    log "instance=$inst status=$state"
  fi
  sleep 300
done
aws ec2 cancel-spot-instance-requests --region $r --spot-instance-request-ids $req >> "$out/delete.log" 2>&1 && log "canceled request=$req"
aws ec2 terminate-instances --region $r --instance-ids $inst >> "$out/delete.log" 2>&1 && log "deleted instance=$inst request=$req"
