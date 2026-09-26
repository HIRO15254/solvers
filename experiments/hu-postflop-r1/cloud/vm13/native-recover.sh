#!/bin/bash
set -euo pipefail
test "$#" -eq 0
unit=solvers-r1-vm13-native-river
package=/opt/r1/native-river-deployment01
proof=/opt/r1/native-river-proof01
meta=/tmp/native-river-metadata01
archive=/tmp/native-river-proof01.tar.gz
case "$(systemctl show "$unit" --property=ActiveState --value)" in inactive|failed) ;; *) exit 2 ;; esac
test "$(systemctl show "$unit" --property=MainPID --value)" = 0
cg=$(systemctl show "$unit" --property=ControlGroup --value)
if test -n "$cg" && test -d "/sys/fs/cgroup$cg"; then
  while IFS= read -r procs; do test -z "$(cat "$procs")"; done < <(find "/sys/fs/cgroup$cg" -name cgroup.procs -type f)
fi
test -d "$proof"
for path in "$archive" "$archive.manifest.json" "$archive.sha256" /tmp/native-river-recovery01.json /tmp/native-river-recovery-check01.json; do test ! -e "$path"; done
mkdir "$meta"
date -u --iso-8601=seconds > "$meta/recovery-start.txt"
systemctl show "$unit" > "$meta/service.log"
journalctl -u "$unit" --no-pager > "$meta/journal.log"
cp /opt/r1/native-river-start01.json "$meta/"
verification_exit=0
python3 -B "$package/run.py" --phase check --control /opt/r1/phase-deployment01/control --out "$proof" \
  > "$meta/verification.json" 2> "$meta/verification.stderr.log" || verification_exit=$?
printf '%s\n' "$verification_exit" > "$meta/verification-exit.txt"
python3 -B /opt/r1/phase-deployment01/bundle-final-proof.py --proof "$proof" --out "$archive" --quiesced \
  --max-bytes 268435456 --extra metadata="$meta" --extra deployment="$package" \
  > /tmp/native-river-recovery01.json
python3 -B /opt/r1/phase-deployment01/bundle-final-proof.py --check "$archive" > /tmp/native-river-recovery-check01.json
chmod 644 "$archive" "$archive.manifest.json" "$archive.sha256"
cat /tmp/native-river-recovery01.json /tmp/native-river-recovery-check01.json
