#!/bin/bash
set -euo pipefail
test "$#" -eq 0
unit=solvers-r1-vm13-current-phases
package=/opt/r1/phase-deployment01
proof=/opt/r1/current-phase-proof01
meta=/tmp/current-phase-metadata01
archive=/tmp/current-phase-proof01.tar.gz
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
for path in "$archive" "$archive.manifest.json" "$archive.sha256" /tmp/current-phase-recovery01.json /tmp/current-phase-recovery-check01.json; do
  test ! -e "$path"
done
mkdir "$meta"
date -u --iso-8601=seconds > "$meta/recovery-start.txt"
systemctl show "$unit" > "$meta/service.log"
journalctl -u "$unit" --no-pager > "$meta/journal.log"
cp /opt/r1/current-phase-start01.json /opt/r1/current-phase-launch01.json /opt/r1/current-phase-installation01.json /opt/r1/bootstrap-complete "$meta/"
mkdir "$meta/deployment"
for item in manifest.json install.py fetch.sh run.py start.py recover.sh bundle-final-proof.py; do
  cp "$package/$item" "$meta/deployment/"
done
verification_exit=0
python3 -B "$package/control/current-phases/check_run.py" --out "$proof" \
  > "$meta/verification.json" 2> "$meta/verification.stderr.log" || verification_exit=$?
printf '%s\n' "$verification_exit" > "$meta/verification-exit.txt"
# Preserve all failed outputs too. Cargo targets live outside proof by contract.
python3 -B "$package/bundle-final-proof.py" --proof "$proof" --out "$archive" --quiesced \
  --max-bytes 536870912 --extra metadata="$meta" --extra controls="$package/control" \
  > /tmp/current-phase-recovery01.json
python3 -B "$package/bundle-final-proof.py" --check "$archive" > /tmp/current-phase-recovery-check01.json
chmod 644 "$archive" "$archive.manifest.json" "$archive.sha256"
cat /tmp/current-phase-recovery01.json /tmp/current-phase-recovery-check01.json
