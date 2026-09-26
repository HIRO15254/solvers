#!/bin/bash
set -euo pipefail
unit=solvers-r1-vm11-final02
case "$(systemctl show "$unit" --property=ActiveState --value)" in
  inactive|failed) ;;
  *) exit 2 ;;
esac
test "$(systemctl show "$unit" --property=MainPID --value)" = 0
meta=/tmp/final-metadata02
mkdir "$meta"
date -u --iso-8601=seconds > "$meta/recovery-start.txt"
journalctl -u solvers-r1-vm11-final -u "$unit" --no-pager > "$meta/journal.log"
systemctl show "$unit" > "$meta/service.log"
cp /opt/r1/final-deployment02.json /tmp/run-final02.sh /tmp/start-final02.py /tmp/recover-final02.sh "$meta/"
cp /opt/r1/bootstrap-complete "$meta/"
if test -f /opt/r1/final-verification02.json; then
  cp /opt/r1/final-verification02.json "$meta/runner-verification.json"
fi
verification_exit=0
if test -s /opt/r1/final-verification02.json && test "$(systemctl show "$unit" --property=ExecMainStatus --value)" = 0; then
  cp /opt/r1/final-verification02.json "$meta/verification.json"
  : > "$meta/verification.stderr.log"
  printf '%s\n' 'Original successful wrapper verification retained; portable verification runs after download.' > "$meta/verification-method.txt"
else
  python3 -B /opt/r1/final-control/final-pipeline/verify.py \
    --out /opt/r1/final-proof02 > "$meta/verification.json" 2> "$meta/verification.stderr.log" || verification_exit=$?
  printf '%s\n' 'Trusted checker run during recovery because successful wrapper verification was unavailable.' > "$meta/verification-method.txt"
fi
printf '%s\n' "$verification_exit" > "$meta/verification-exit.txt"
python3 -B /tmp/bundle-final-proof.py \
  --proof /opt/r1/final-proof02 --out /tmp/final-proof02.tar.gz --quiesced \
  --max-bytes 536870912 --extra metadata="$meta" --extra controls=/opt/r1/final-control \
  > /tmp/final-proof02-recovery.json
python3 -B /tmp/bundle-final-proof.py --check /tmp/final-proof02.tar.gz \
  > /tmp/final-proof02-recovery-check.json
chmod 644 /tmp/final-proof02.tar.gz /tmp/final-proof02.tar.gz.manifest.json /tmp/final-proof02.tar.gz.sha256
cat /tmp/final-proof02-recovery.json /tmp/final-proof02-recovery-check.json
