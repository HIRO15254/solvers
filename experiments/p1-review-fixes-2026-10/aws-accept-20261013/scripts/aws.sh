#!/bin/bash
# Creates a c6a.8xlarge Spot instance (us-east-1, self-terminating after 4 h), runs SETUP (default setup.sh),
# fetches results every 3 minutes and terminates the instance.
# usage (repository root): aws.sh OLD_REV NEW_REV S5_REV [SETUP]
set -u
old=$1; new=$2; s5=$3; base=experiments/p1-review-fixes-2026-10/aws-accept-20261013
out=runs/p1rf/aws-accept; mkdir -p $out
export AWS_DEFAULT_REGION=us-east-1 MSYS_NO_PATHCONV=1  # Git Bash would rewrite /aws/... parameter names
for v in old new s5; do
  git archive --format=tar ${!v} Cargo.toml Cargo.lock crates examples .cargo > $out/src-$v.tar
done
tar -czf $out/cfg-accept.tgz -C $base/configs .
cp $base/scripts/${4:-setup.sh} $out/setup.sh
key=$out/p1rf-key.pem
if [ ! -f $key ]; then
  aws ec2 delete-key-pair --key-name p1rf-accept > /dev/null 2>&1
  aws ec2 create-key-pair --key-name p1rf-accept --query KeyMaterial --output text > $key && chmod 600 $key
fi
sg=$(aws ec2 describe-security-groups --group-names p1rf-accept --query 'SecurityGroups[0].GroupId' --output text 2>/dev/null) \
  || sg=$(aws ec2 create-security-group --group-name p1rf-accept --description "p1 acceptance ssh" --query GroupId --output text)
aws ec2 authorize-security-group-ingress --group-id $sg --protocol tcp --port 22 \
  --cidr "$(curl -s https://checkip.amazonaws.com)/32" > /dev/null 2>&1
ami=$(aws ssm get-parameter --name /aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id \
  --query Parameter.Value --output text)
printf '#!/bin/bash\nshutdown -h +240\n' > $out/userdata.sh
id=""
market="--instance-market-options MarketType=spot"
[ "${ONDEMAND:-0}" = 1 ] && market=""  # ONDEMAND=1 when no zone has Spot capacity
for z in us-east-1d us-east-1b us-east-1c us-east-1f us-east-1a; do
  subnet=$(aws ec2 describe-subnets --filters Name=availability-zone,Values=$z Name=default-for-az,Values=true \
    --query 'Subnets[0].SubnetId' --output text)
  id=$(aws ec2 run-instances --image-id $ami --instance-type c6a.8xlarge --key-name p1rf-accept \
    --security-group-ids $sg --subnet-id $subnet $market \
    --instance-initiated-shutdown-behavior terminate --user-data file://$out/userdata.sh \
    --block-device-mappings 'DeviceName=/dev/sda1,Ebs={VolumeSize=60,VolumeType=gp3,DeleteOnTermination=true}' \
    --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value=p1rf-accept}]' \
    --query 'Instances[0].InstanceId' --output text 2>> $out/create.log) && break
  id=""
done
[ -z "$id" ] && { echo "create failed"; exit 1; }
echo "$id" > $out/instance-id
aws ec2 wait instance-running --instance-ids $id
ip=$(aws ec2 describe-instances --instance-ids $id --query 'Reservations[0].Instances[0].PublicIpAddress' --output text)
echo "created $id $z $ip $(date -Iseconds)"
ssh_() { ssh -i $key -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=20 ubuntu@$ip "$@"; }
for k in $(seq 1 30); do ssh_ true && break; sleep 10; done
(cd $out && scp -i p1rf-key.pem -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR \
  src-old.tar src-new.tar src-s5.tar cfg-accept.tgz setup.sh ubuntu@$ip: > scp.log 2>&1) || { echo "scp failed"; tail -5 $out/scp.log; }
ssh_ 'nohup bash ~/setup.sh > ~/setup.log 2>&1 < /dev/null &' > $out/start.log 2>&1
echo "started $(date -Iseconds)"
while true; do
  sleep 180
  if ssh_ 'tar -czf ~/res.tgz -C ~ results setup.log 2>/dev/null; tail -1 ~/results/progress.txt' > $out/ssh.log 2>&1; then
    (cd $out && scp -i p1rf-key.pem -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR \
      ubuntu@$ip:res.tgz res.tgz > /dev/null 2>&1 && tar -xzf res.tgz)
    echo "$(date +%T) $(tail -1 $out/ssh.log)"
    grep -q ALL_DONE $out/results/progress.txt 2>/dev/null && break
  elif [ "$(aws ec2 describe-instances --instance-ids $id --query 'Reservations[0].Instances[0].State.Name' --output text)" != running ]; then
    echo "instance gone $(date -Iseconds)"; break
  fi
done
aws ec2 terminate-instances --instance-ids $id > $out/delete.log 2>&1
aws ec2 wait instance-terminated --instance-ids $id
echo "terminated $(date -Iseconds)"
