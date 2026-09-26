#!/bin/bash
set -euo pipefail
unit=solvers-r1-vm11-memory03
case "$(systemctl show "$unit" --property=ActiveState --value)" in
  inactive|failed) ;;
  *) exit 2 ;;
esac
test "$(systemctl show "$unit" --property=MainPID --value)" = 0
meta=/tmp/memory-metadata03
mkdir "$meta"
date -u --iso-8601=seconds > "$meta/recovery-start.txt"
journalctl -u "$unit" --no-pager > "$meta/journal.log"
systemctl show "$unit" > "$meta/service.log"
cp /opt/r1/memory-deployment03.json /tmp/run-memory03.sh /tmp/start-memory03.py /tmp/recover-memory03.sh /tmp/memory-controls03.json "$meta/"
cp /opt/r1/bootstrap-complete "$meta/"
if test -f /opt/r1/memory-verification03.json; then
  cp /opt/r1/memory-verification03.json "$meta/runner-verification.json"
fi
verification_exit=0
if test -s /opt/r1/memory-verification03.json && test "$(systemctl show "$unit" --property=ExecMainStatus --value)" = 0; then
  cp /opt/r1/memory-verification03.json "$meta/verification.json"
  : > "$meta/verification.stderr.log"
  printf '%s\n' 'Original successful wrapper verification retained; portable verification runs after download.' > "$meta/verification-method.txt"
else
  python3 -B /opt/r1/final-control/focused-memory/run.py --phase check \
    --out /opt/r1/focused-memory03 --reference-proof /opt/r1/final-proof02 \
    > "$meta/verification.json" 2> "$meta/verification.stderr.log" || verification_exit=$?
  printf '%s\n' 'Trusted checker run during recovery because successful wrapper verification was unavailable.' > "$meta/verification-method.txt"
fi
printf '%s\n' "$verification_exit" > "$meta/verification-exit.txt"
printf '%s\n' 'Generic final-pipeline packer reports build.json absent. Focused-memory has no separate build.json; its compile stage, native binary and GCC records belong to result.json and CAS. The focused checker defines required evidence.' > "$meta/layout-note.txt"
python3 -B /tmp/bundle-final-proof.py \
  --proof /opt/r1/focused-memory03 --out /tmp/focused-memory03.tar.gz --quiesced \
  --max-bytes 536870912 --extra metadata="$meta" --extra controls=/opt/r1/final-control \
  > /tmp/focused-memory03-recovery.json
python3 -B /tmp/bundle-final-proof.py --check /tmp/focused-memory03.tar.gz \
  > /tmp/focused-memory03-recovery-check.json
chmod 644 /tmp/focused-memory03.tar.gz /tmp/focused-memory03.tar.gz.manifest.json /tmp/focused-memory03.tar.gz.sha256
cat /tmp/focused-memory03-recovery.json /tmp/focused-memory03-recovery-check.json
