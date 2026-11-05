#!/usr/bin/env bash
# Run an idempotent ~/job.sh on an Ubuntu 24.04 EC2 Spot instance launched as docs/development.md §5.5 requires.
# Usage: aws_run.sh <name> <deadline hours> <local out dir> <remote job script> [files...]
# Uploads src.tar (Cargo.toml Cargo.lock crates examples .cargo), job.sh and flat extras.
# Job contract: ~/results is downloaded every 300s; progress.txt contains ALL_DONE only
# after ~/final.tgz is ready, or terminal JOB_FAILED on solver failure. Both delete the instance.
# Requires AWS CLI v2, curl, tar, ssh/scp/ssh-keygen and a default VPC whose default subnets assign public IPv4
# addresses. Windows: run in Git Bash (its SSH uses chmod permissions).
# REGIONS="sa-east-1 us-east-2 us-east-1" lists the candidate regions (those with a 256-vCPU Spot quota; others have 5).
# INSTANCE_TYPES defaults to "c7a.8xlarge c6a.8xlarge c7i.8xlarge c6i.8xlarge" (32 vCPU/64 GiB).
# Launch: get-spot-placement-scores ranks the unblocked regions for INSTANCE_TYPES (ties: the lower current Spot price
# of the cheapest type). In the best region, a launch template of this run (AMI, key, security group, 60 GB gp3 root,
# shutdown = terminate) and one instant EC2 Fleet (price-capacity-optimized) over every type x default subnet start one
# Spot instance; a region whose fleet starts none yields to the next. Besides the describe, tag, terminate, key and
# security-group calls, the IAM user needs ec2:GetSpotPlacementScores, ec2:CreateFleet, ec2:RunInstances,
# ec2:CreateLaunchTemplate and ec2:DeleteLaunchTemplate, and the account the AWSServiceRoleForEC2Fleet role.
# A Spot interruption terminates the instance and an instant fleet does not replace it: the runner relaunches from the
# region ranking and the job restarts on the new instance (results downloaded so far move to
# results.reclaimed-<instance>). With RESUME=1 the runner also uploads those results as ~/resume.tgz (a `results/`
# tree), so a job that restores them (gcp/bench/job_bench.sh) skips its finished steps instead of starting from scratch.
# Spot rule (spot_blocked_until in ../cloud_progress.sh): an interruption within SPOT_QUICK_SECONDS (default 1800) of
# reaching running is a quick reclaim; reclaims of parallel runs within 120 s are one event. After 3 consecutive quick
# reclaim events in a region (any run on this machine; SPOT_RECLAIM_LOG, default ~/.cache/p2-aws/spot-reclaims.log)
# the region is skipped for 8 hours.
# The reusable ~/.ssh/p2-aws.pem key and p2-solver-runner SG are retained; SG SSH ingress
# is restricted to the current checkip.amazonaws.com /32. Every instance (tag RunId) and launch template this
# invocation creates is terminated/deleted on exit, including errors and signals.
set -Eeuo pipefail
export AWS_PAGER=""
export MSYS_NO_PATHCONV=1
source "$(dirname "${BASH_SOURCE[0]}")/../cloud_progress.sh"
[[ $# -ge 4 ]] || { echo "usage: $0 <name> <deadline hours> <local out dir> <remote job script> [files...]" >&2; exit 2; }
name=$1; hours=$2; out=$3; job=$4; shift 4
[[ $name =~ ^[a-zA-Z0-9_-]+$ && $hours =~ ^[1-9][0-9]*$ ]] || { echo 'name must be alphanumeric/-/_; deadline must be positive integer hours' >&2; exit 2; }
[[ -f $job ]] || { echo "missing job: $job" >&2; exit 2; }
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
mkdir -p "$out"; out=$(cd "$out" && pwd)
tmp=$(mktemp -d)
run_id="p2-$(date +%s)-$$-$RANDOM"
instance=; ip=; last_state=; uploaded=0; launched=0; running_since=; period_ok=0
reclaim_log=${SPOT_RECLAIM_LOG:-$HOME/.cache/p2-aws/spot-reclaims.log}
quick_seconds=${SPOT_QUICK_SECONDS:-1800}
read -ra regions <<< "${REGIONS:-sa-east-1 us-east-2 us-east-1}"
region=${regions[0]}; instance_region=
template=$run_id
declare -A templates=()  # regions where this run created its launch template
# Native Windows aws.exe emits CRLF and cannot resolve MSYS /tmp paths.
aws_cli() {
  local arg prefix path
  local -a args=()
  for arg in "$@"; do
    case $arg in
      fileb://*|file://*)
        prefix=${arg%%://*}; path=${arg#*://}
        if command -v cygpath >/dev/null; then path=$(cygpath -m "$path"); fi
        arg="$prefix://$path"
        ;;
    esac
    args+=("$arg")
  done
  aws "${args[@]}" | tr -d '\r'
}
aws_ec2() { aws_cli --region "$region" ec2 "$@"; }
log() { printf '%s %s\n' "$(date -u +%FT%TZ)" "$*" | tee -a "$out/vm.log"; }
record_reclaim() { # KIND RAN_SECONDS
  mkdir -p "$(dirname "$reclaim_log")"
  printf '%s %s region=%s az=%s type=%s kind=%s ran_s=%s name=%s instance=%s\n' "$(date +%s)" "$(date -u +%FT%TZ)" \
    "$region" "$az" "$type" "$1" "$2" "$name" "$instance" >> "$reclaim_log"
  log "spot reclaim log: kind=$1 region=$region ran_s=$2"
}
cleanup() {
  local rc=$? cleanup_failed=0 ids id
  trap - EXIT INT TERM HUP
  set +e
  # Find instances by tag even if a signal/API transport failure lost the create-fleet response.
  for region in "${regions[@]}"; do
    ids=$(aws_ec2 describe-instances --filters "Name=tag:RunId,Values=$run_id" Name=instance-state-name,Values=pending,running,stopping,stopped --query 'Reservations[].Instances[].InstanceId' --output text 2>> "$out/delete.log")
    if [[ $? != 0 ]]; then log "ERROR discovering instances region=$region by RunId"; cleanup_failed=1; fi
    # Cover a discovery failure and tags that are not visible yet.
    if [[ -n $instance && $region == "$instance_region" && " $ids " != *" $instance "* ]]; then ids="$ids $instance"; fi
    for id in $ids; do
      [[ $id == i-* ]] || continue
      if aws_ec2 terminate-instances --instance-ids "$id" >> "$out/delete.log" 2>&1 &&
         aws_ec2 wait instance-terminated --instance-ids "$id" >> "$out/delete.log" 2>&1; then
        log "deleted instance=$id"
      else log "ERROR terminating instance=$id (see delete.log)"; cleanup_failed=1; fi
    done
    [[ -n ${templates[$region]:-} ]] || continue
    if aws_ec2 delete-launch-template --launch-template-name "$template" >> "$out/delete.log" 2> "$tmp/template-error"; then
      log "deleted launch template=$template region=$region"
    elif ! grep -q InvalidLaunchTemplateName.NotFoundException "$tmp/template-error"; then
      cat "$tmp/template-error" >> "$out/delete.log"
      log "ERROR deleting launch template=$template region=$region (see delete.log)"; cleanup_failed=1
    fi
  done
  rm -rf "$tmp"
  (( rc != 0 || cleanup_failed == 0 )) || rc=1
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP
trap 'log "ERROR line=$LINENO exit=$?"' ERR
for cmd in aws curl tar gzip ssh scp ssh-keygen awk sort; do command -v "$cmd" >/dev/null; done
# Use Git Bash's SSH, not Windows OpenSSH's different private-key ACL rules.
ssh_bin=$(command -v ssh); scp_bin=$(command -v scp)
if [[ -x /usr/bin/ssh && -x /usr/bin/scp ]]; then ssh_bin=/usr/bin/ssh; scp_bin=/usr/bin/scp; fi
key=$HOME/.ssh/p2-aws.pem; key_name=p2-solver-runner
mkdir -p "$HOME/.ssh"; chmod 700 "$HOME/.ssh"
if [[ ! -f $key ]]; then
  # Existing regional AWS keys must match this local key when a market is tried.
  ssh-keygen -q -t rsa -b 3072 -m PEM -N '' -f "$key"
fi
chmod 600 "$key"
ssh-keygen -y -f "$key" > "$tmp/key.pub"
public_ip=$(curl --fail --silent --show-error https://checkip.amazonaws.com | tr -d '\r\n')
[[ $public_ip =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'could not determine public IPv4' >&2; exit 1; }
prepare_region() {
if ! aws_ec2 describe-key-pairs --key-names "$key_name" --include-public-key --query 'KeyPairs[0].PublicKey' --output text > "$tmp/aws-key.pub" 2> "$tmp/key-error"; then
  if ! grep -q InvalidKeyPair.NotFound "$tmp/key-error"; then cat "$tmp/key-error" >&2; exit 1; fi
  aws_ec2 import-key-pair --key-name "$key_name" --public-key-material "fileb://$tmp/key.pub" --tag-specifications 'ResourceType=key-pair,Tags=[{Key=Name,Value=p2-solver-runner}]' >> "$out/create.log"
else
  # Imported public keys can contain comments; compare key algorithm and material.
  read -r algorithm material _ < "$tmp/key.pub"
  read -r remote_algorithm remote_material _ < "$tmp/aws-key.pub"
  [[ $algorithm == "$remote_algorithm" && $material == "$remote_material" ]] || { echo 'AWS key pair does not match local private key' >&2; exit 1; }
fi
vpc=$(aws_ec2 describe-vpcs --filters Name=is-default,Values=true --query 'Vpcs[0].VpcId' --output text)
[[ $vpc == vpc-* ]] || { echo 'no default VPC; a public default VPC is required' >&2; exit 1; }
sg=$(aws_ec2 describe-security-groups --filters "Name=vpc-id,Values=$vpc" Name=group-name,Values=p2-solver-runner --query 'SecurityGroups[0].GroupId' --output text)
if [[ $sg != sg-* ]]; then
  sg=$(aws_ec2 create-security-group --group-name p2-solver-runner --description 'P2 solver SSH from runner public IP only' --vpc-id "$vpc" --tag-specifications 'ResourceType=security-group,Tags=[{Key=Name,Value=p2-solver-runner}]' --query GroupId --output text)
fi
# Reconcile ingress, retaining precisely the current /32 SSH rule and no others.
read -ra rules <<< "$(aws_ec2 describe-security-group-rules --filters "Name=group-id,Values=$sg" --query "SecurityGroupRules[?IsEgress==\`false\` && !(IpProtocol=='tcp' && FromPort==\`22\` && ToPort==\`22\` && CidrIpv4=='$public_ip/32')].SecurityGroupRuleId" --output text)"
# A rule revoked moments ago can still be listed (eventual consistency, or a parallel runner revoked it).
if (( ${#rules[@]} )) && ! aws_ec2 revoke-security-group-ingress --group-id "$sg" --security-group-rule-ids "${rules[@]}" >> "$out/create.log" 2> "$tmp/sg-error"; then
  grep -q InvalidSecurityGroupRuleId.NotFound "$tmp/sg-error" || { cat "$tmp/sg-error" >&2; exit 1; }
fi
if ! aws_ec2 authorize-security-group-ingress --group-id "$sg" --protocol tcp --port 22 --cidr "$public_ip/32" >> "$out/create.log" 2> "$tmp/sg-error"; then
  grep -q InvalidPermission.Duplicate "$tmp/sg-error" || { cat "$tmp/sg-error" >&2; exit 1; }
fi
ami=$(aws_cli --region "$region" ssm get-parameter --name /aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id --query Parameter.Value --output text)
root_device=$(aws_ec2 describe-images --image-ids "$ami" --query 'Images[0].RootDeviceName' --output text)
}
# Re-authorize SSH from the current public IP (a changed local IP otherwise strands the run).
# Never fatal: every failure is logged and retried at the next call.
last_refresh=0
refresh_ssh_ingress() {
  local now new_ip stale
  now=$(date +%s)
  (( now - last_refresh >= 240 )) || return 0
  last_refresh=$now
  new_ip=$(curl -s --max-time 10 https://checkip.amazonaws.com 2> /dev/null | tr -d '\r\n' || true)
  if [[ ! $new_ip =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]; then log 'ssh ingress refresh skipped (no public IP)'; return 0; fi
  public_ip=$new_ip
  if ! stale=$(aws_ec2 describe-security-group-rules --filters "Name=group-id,Values=$sg" --query "SecurityGroupRules[?IsEgress==\`false\` && !(IpProtocol=='tcp' && FromPort==\`22\` && ToPort==\`22\` && CidrIpv4=='$public_ip/32')].SecurityGroupRuleId" --output text 2>> "$out/create.log"); then
    log 'ssh ingress refresh failed (see create.log)'; return 0
  fi
  read -ra stale_rules <<< "$stale"
  if (( ${#stale_rules[@]} )); then
    aws_ec2 revoke-security-group-ingress --group-id "$sg" --security-group-rule-ids "${stale_rules[@]}" >> "$out/create.log" 2>&1 || true
  fi
  if ! aws_ec2 authorize-security-group-ingress --group-id "$sg" --protocol tcp --port 22 --cidr "$public_ip/32" >> "$out/create.log" 2> "$tmp/sg-refresh-error"; then
    if ! grep -q InvalidPermission.Duplicate "$tmp/sg-refresh-error"; then
      cat "$tmp/sg-refresh-error" >> "$out/create.log" || true
      log 'ssh ingress refresh failed (see create.log)'; return 0
    fi
  fi
  log "ssh ingress refreshed ip=$public_ip"
}
tar -cf "$tmp/src.tar" -C "$repo" Cargo.toml Cargo.lock crates examples .cargo
cp "$job" "$tmp/job.sh"
uploads=("$tmp/src.tar" "$tmp/job.sh")
for file in "$@"; do
  base=$(basename "$file")
  [[ -f $file && ! -e $tmp/$base ]] || { echo "missing or duplicate/reserved upload name: $file" >&2; exit 2; }
  cp "$file" "$tmp/$base"; uploads+=("$tmp/$base")
done
read -ra types <<< "${INSTANCE_TYPES:-c7a.8xlarge c6a.8xlarge c7i.8xlarge c6i.8xlarge}"
for type in "${types[@]}"; do
  [[ $type =~ ^[a-z0-9]+\.[a-z0-9]+$ ]] || { echo "invalid instance type: $type" >&2; exit 2; }
done
for region in "${regions[@]}"; do
  [[ $region =~ ^[a-z0-9-]+$ ]] || { echo "invalid region: $region" >&2; exit 2; }
done
deadline=$(( $(date +%s) + 10#$hours * 3600 ))
# Rank the unblocked regions by Spot placement score and start one Spot instance in the best one with capacity.
launch_spot() {
local scores score cheapest subnets subnet overrides described attempt
scores=$(aws_cli --region "${regions[0]}" ec2 get-spot-placement-scores --target-capacity 1 --instance-types "${types[@]}" \
  --region-names "${regions[@]}" --query 'SpotPlacementScores[].[Region,Score]' --output text)
: > "$tmp/ranked"
for region in "${regions[@]}"; do
  blocked=$(spot_blocked_until "$reclaim_log" "$region")
  if (( blocked > $(date +%s) )); then
    log "skipping region=$region until $(date -u -d "@$blocked" +%FT%TZ): consecutive quick Spot reclaims ($reclaim_log)"
    continue
  fi
  score=$(awk -v r="$region" '$1 == r { print $2 }' <<< "$scores")
  cheapest=$(aws_ec2 describe-spot-price-history --instance-types "${types[@]}" --product-descriptions 'Linux/UNIX' \
    --start-time "$(date -u +%FT%TZ)" --query 'SpotPriceHistory[].SpotPrice' --output text |
    awk '{ for (i = 1; i <= NF; i++) if (m == "" || $i + 0 < m + 0) m = $i } END { print (m == "" ? 999 : m) }')
  printf '%s %s %s\n' "${score:-0}" "$cheapest" "$region" >> "$tmp/ranked"
done
[[ -s $tmp/ranked ]] || { log 'every region is blocked by quick Spot reclaims'; exit 1; }
LC_ALL=C sort -k1,1nr -k2,2g -o "$tmp/ranked" "$tmp/ranked"
log 'ranked regions: placement-score cheapest-USD/hour region'
while IFS= read -r ranked; do log "$ranked"; done < "$tmp/ranked"
while read -r score cheapest region; do
  prepare_region
  subnets=$(aws_ec2 describe-subnets --filters "Name=vpc-id,Values=$vpc" Name=default-for-az,Values=true \
    --query 'Subnets[?MapPublicIpOnLaunch].SubnetId' --output text)
  overrides=
  for type in "${types[@]}"; do
    for subnet in $subnets; do
      [[ $subnet == subnet-* ]] || continue
      overrides+="${overrides:+,}{\"InstanceType\":\"$type\",\"SubnetId\":\"$subnet\"}"
    done
  done
  [[ -n $overrides ]] || { log "skipping region=$region: no default subnet assigning public IPv4"; continue; }
  if [[ -z ${templates[$region]:-} ]]; then
    templates[$region]=1  # set first: cleanup deletes the template even if a signal cuts the call
    printf '{"ImageId":"%s","KeyName":"%s","SecurityGroupIds":["%s"],"InstanceInitiatedShutdownBehavior":"terminate","BlockDeviceMappings":[{"DeviceName":"%s","Ebs":{"VolumeSize":60,"VolumeType":"gp3","DeleteOnTermination":true}}]}' \
      "$ami" "$key_name" "$sg" "$root_device" > "$tmp/template.json"
    aws_ec2 create-launch-template --launch-template-name "$template" --launch-template-data "file://$tmp/template.json" >> "$out/create.log"
    log "created launch template=$template region=$region ami=$ami"
  fi
  printf '{"Type":"instant","ClientToken":"%s","TargetCapacitySpecification":{"TotalTargetCapacity":1,"DefaultTargetCapacityType":"spot"},"SpotOptions":{"AllocationStrategy":"price-capacity-optimized"},"LaunchTemplateConfigs":[{"LaunchTemplateSpecification":{"LaunchTemplateName":"%s","Version":"$Latest"},"Overrides":[%s]}],"TagSpecifications":[{"ResourceType":"instance","Tags":[{"Key":"Name","Value":"%s"},{"Key":"Runner","Value":"p2-solver-runner"},{"Key":"RunId","Value":"%s"}]}]}' \
    "$run_id-$RANDOM" "$template" "$overrides" "$name" "$run_id" > "$tmp/fleet.json"
  log "requesting instant fleet region=$region placement-score=$score types=${types[*]} subnets=$(wc -w <<< "$subnets")"
  aws_ec2 create-fleet --cli-input-json "file://$tmp/fleet.json" --output json > "$tmp/fleet-out.json"
  cat "$tmp/fleet-out.json" >> "$out/create.log"
  instance=$(awk -F'"' '{ for (i = 2; i <= NF; i += 2) if ($i ~ /^i-[0-9a-f]+$/) { print $i; exit } }' "$tmp/fleet-out.json")
  if [[ $instance == i-* ]]; then instance_region=$region; break; fi
  log "no instance from the fleet in region=$region; errors: $(awk -F'"' '$2 == "ErrorCode" { print $4 }' "$tmp/fleet-out.json" | sort | uniq -c | tr -s ' \n' ' ')"
  (( $(date +%s) < deadline )) || { log 'deadline passed while requesting'; exit 1; }
done < "$tmp/ranked"
[[ -n $instance ]] || { log 'no Spot capacity in the ranked regions'; exit 1; }
# A new instance can be missing from describe calls for a few seconds.
type=; az=
for (( attempt = 0; attempt < 12; attempt++ )); do
  if described=$(aws_ec2 describe-instances --instance-ids "$instance" --query 'Reservations[0].Instances[0].[InstanceType,Placement.AvailabilityZone]' --output text 2>> "$out/create.log"); then
    read -r type az <<< "$described"
    [[ $az != "$region"* ]] || break
  fi
  sleep 5
done
[[ $az == "$region"* ]] || { log "instance=$instance stays undescribable"; exit 1; }
launch_price=$(aws_ec2 describe-spot-price-history --instance-types "$type" --availability-zone "$az" --product-descriptions 'Linux/UNIX' --start-time "$(date -u +%FT%TZ)" --query 'SpotPriceHistory[0].SpotPrice' --output text)
log "using region=$region instance=$instance az=$az type=$type spot-price-usd-per-hour=$launch_price placement-score=$score"
}
# An instant fleet does not replace an interrupted instance: start over from the region ranking. RESUME=1 packs the
# results downloaded so far into the uploads of the new instance.
relaunch() {
  log "instance=$instance interrupted (state=$state); relaunching"
  aws_ec2 terminate-instances --instance-ids "$instance" >> "$out/delete.log"
  if [[ -d $out/results ]]; then
    if [[ ${RESUME:-0} == 1 ]] && tar --force-local -czf "$tmp/resume.tgz" -C "$out" results; then
      [[ " ${uploads[*]} " == *" $tmp/resume.tgz "* ]] || uploads+=("$tmp/resume.tgz")
      log 'packed the downloaded results into resume.tgz'
    fi
    mv "$out/results" "$out/results.reclaimed-$instance"
  fi
  instance=; instance_region=; ip=; last_state=; uploaded=0; launched=0; price_at=0; running_since=
  launch_spot
}
launch_spot
ssh_opts=(-i "$key" -o BatchMode=yes -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=15 -o ServerAliveInterval=30 -o ServerAliveCountMax=3)
remote() { "$ssh_bin" "${ssh_opts[@]}" "ubuntu@$ip" "$@"; }
price_at=0; next_download=0; describe_failures=0
while (( $(date +%s) < deadline )); do
  # A local AWS CLI failure (e.g. out of memory) is not an instance state: retry for up to ~10 minutes.
  if ! described=$(aws_ec2 describe-instances --instance-ids "$instance" --query 'Reservations[0].Instances[0].[State.Name,PublicIpAddress]' --output text 2>> "$out/describe-error.log"); then
    (( ++describe_failures <= 20 )) || { log 'describe-instances kept failing'; exit 1; }
    log "describe-instances failed ($describe_failures/20); retrying"; sleep 30; continue
  fi
  describe_failures=0
  read -r state ip <<< "$described"
  if [[ $state != "$last_state" ]]; then log "instance=$instance status=$state ip=$ip"; last_state=$state; fi
  now=$(date +%s)
  if [[ $state == running ]]; then
    if [[ -z $running_since ]]; then running_since=$now; period_ok=0; fi
    if (( ! period_ok && now - running_since >= quick_seconds )); then
      record_reclaim ok $(( now - running_since )); period_ok=1
    fi
  elif [[ -n $running_since ]]; then
    # The runner never stops or terminates an instance before completion: leaving running is a Spot interruption.
    if (( ! period_ok )); then record_reclaim quick $(( now - running_since )); fi
    running_since=
  fi
  case $state in
    pending) sleep 15; continue ;;
    running) ;;
    stopping|stopped|shutting-down|terminated) relaunch; continue ;;
    *) log "unexpected instance state=$state"; exit 1 ;;
  esac
  if (( ! launched )); then
    if ! remote true > "$out/ssh-wait.log" 2>&1; then refresh_ssh_ingress; sleep 15; continue; fi
    # A failed upload or job start is retried at the next pass.
    if (( ! uploaded )); then
      if ! "$scp_bin" "${ssh_opts[@]}" "${uploads[@]}" "ubuntu@$ip:" > "$out/scp.log" 2>&1; then
        log 'upload failed (see scp.log); retrying'; sleep 15; continue
      fi
      uploaded=1; log 'uploaded source, job and flat extra files'
    fi
    if ! remote 'mkdir -p ~/results; nohup bash ~/job.sh >> ~/results/job.log 2>&1 < /dev/null &' >> "$out/ssh-wait.log" 2>&1; then
      log 'job start failed (see ssh-wait.log); retrying'; sleep 15; continue
    fi
    launched=1; next_download=$(date +%s); log "launched instance=$instance"
  fi
  now=$(date +%s)
  if (( now - price_at >= 300 )); then
    # EC2 bills at the AZ/type Spot market price; history records the latest price
    # effective now (not a final invoice, which also includes EBS/transfer).
    price=$(aws_ec2 describe-spot-price-history --instance-types "$type" --availability-zone "$az" --product-descriptions 'Linux/UNIX' --start-time "$(date -u +%FT%TZ)" --query 'SpotPriceHistory[0].[SpotPrice,Timestamp]' --output text)
    log "spot-price-usd-per-hour type=$type az=$az price=$price"; price_at=$now
  fi
  if (( now >= next_download )); then
    # A bad local copy (e.g. a full local disk truncating the stream) is retried; it never deletes the VM.
    if remote 'tar -czf - -C "$HOME" results' > "$out/res.tgz.tmp" 2> "$out/ssh.log" &&
       gzip -t "$out/res.tgz.tmp" 2>> "$out/ssh.log" && tar --force-local -xzf "$out/res.tgz.tmp" -C "$out" 2>> "$out/ssh.log"; then
      mv "$out/res.tgz.tmp" "$out/res.tgz"
      log 'downloaded results'
      if job_terminal "$out/results/progress.txt"; then
        if job_failed "$out/results/progress.txt"; then
          log 'JOB_FAILED downloaded results; deleting instance'; exit 1
        fi
        if "$scp_bin" "${ssh_opts[@]}" "ubuntu@$ip:final.tgz" "$out/final.tgz.tmp" > "$out/final-scp.log" 2>&1 &&
           gzip -t "$out/final.tgz.tmp" 2>> "$out/final-scp.log"; then
          mv "$out/final.tgz.tmp" "$out/final.tgz"
          log 'ALL_DONE final downloaded'; exit 0
        fi
        log 'final.tgz download failed (see final-scp.log); retrying at next interval'; refresh_ssh_ingress
      fi
    else log 'results download unavailable (see ssh.log); retrying at next interval'; refresh_ssh_ingress; fi
    next_download=$(( now + 300 ))
  fi
  sleep 15
done
log 'deadline passed'
exit 0
