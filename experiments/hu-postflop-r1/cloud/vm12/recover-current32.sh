#!/bin/bash
set -euo pipefail
test "$#" -eq 0
unit=solvers-r1-vm12-current32
package=/opt/r1/current-deployment01
proof=/opt/r1/current32-proof01
meta=/tmp/current32-metadata01
archive=/tmp/current32-proof01.tar.gz
case "$(systemctl show "$unit" --property=ActiveState --value)" in
  inactive|failed) ;;
  *) exit 2 ;;
esac
test "$(systemctl show "$unit" --property=MainPID --value)" = 0
cg=$(systemctl show "$unit" --property=ControlGroup --value)
if test -n "$cg" && test -d "/sys/fs/cgroup$cg"; then
  while IFS= read -r procs; do test -z "$(cat "$procs")"; done < <(find "/sys/fs/cgroup$cg" -name cgroup.procs -type f)
fi
test -d "$proof"
test ! -e "$archive"
test ! -e "$archive.manifest.json"
test ! -e "$archive.sha256"
test ! -e /tmp/current32-recovery01.json
test ! -e /tmp/current32-recovery-check01.json
mkdir "$meta"
date -u --iso-8601=seconds > "$meta/recovery-start.txt"
systemctl show "$unit" > "$meta/service.log"
journalctl -u "$unit" --no-pager > "$meta/journal.log"
cp /opt/r1/current32-start01.json /opt/r1/current32-installation01.json /opt/r1/bootstrap-complete "$meta/"
mkdir "$meta/deployment"
for item in manifest.json install-inputs.py fetch-dependencies.sh run-current32.sh start-current32.py recover-current32.sh bundle-final-proof.py; do
  cp "$package/$item" "$meta/deployment/"
done
for suffix in json stderr.log exit; do
  if test -f "/opt/r1/current32-verification01.$suffix"; then
    cp "/opt/r1/current32-verification01.$suffix" "$meta/wrapper-verification.$suffix"
  fi
done
verification_exit=0
if test -s /opt/r1/current32-verification01.json && test -f /opt/r1/current32-verification01.exit && test "$(cat /opt/r1/current32-verification01.exit)" = 0; then
  cp /opt/r1/current32-verification01.json "$meta/verification.json"
  : > "$meta/verification.stderr.log"
  printf '%s\n' 'Original wrapper check retained; full portable verification follows download.' > "$meta/verification-method.txt"
else
  python3 -B /opt/r1/current-control/current-scaling32/run.py --phase check --out "$proof" \
    --foundation-proof /opt/r1/exact-proof04 > "$meta/verification.json" 2> "$meta/verification.stderr.log" || verification_exit=$?
  printf '%s\n' 'Recovery check because wrapper check was unavailable or failed; original wrapper outputs retained.' > "$meta/verification-method.txt"
fi
printf '%s\n' "$verification_exit" > "$meta/verification-exit.txt"
# Include all deployment controls and raw failed outputs, without rewriting proof.
python3 -B "$package/bundle-final-proof.py" --proof "$proof" --out "$archive" --quiesced \
  --max-bytes 536870912 --extra metadata="$meta" --extra controls=/opt/r1/current-control \
  > /tmp/current32-recovery01.json
python3 -B "$package/bundle-final-proof.py" --check "$archive" > /tmp/current32-recovery-check01.json
chmod 644 "$archive" "$archive.manifest.json" "$archive.sha256"
cat /tmp/current32-recovery01.json /tmp/current32-recovery-check01.json
