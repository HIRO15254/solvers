#!/bin/bash
set -euo pipefail
test "$#" -eq 0
unit=solvers-r1-vm14-flat32
package=/opt/r1/flop-flat-cloud32-package
meta=/tmp/flop-flat-cloud32-metadata01
archive=/tmp/flop-flat-cloud32-proof01.tar.gz
case "$(systemctl show "$unit" --property=ActiveState --value)" in inactive|failed) ;; *) exit 2 ;; esac
test "$(systemctl show "$unit" --property=MainPID --value)" = 0
cg=$(systemctl show "$unit" --property=ControlGroup --value)
if test -n "$cg" && test -d "/sys/fs/cgroup$cg"; then
  while IFS= read -r procs; do test -z "$(cat "$procs")"; done < <(find "/sys/fs/cgroup$cg" -name cgroup.procs -type f)
fi
test ! -e "$archive"
test ! -e "$archive.manifest.json"
test ! -e "$archive.sha256"
test ! -e /tmp/flop-flat-cloud32-recovery01.json
mkdir "$meta"
systemctl show "$unit" > "$meta/service.log"
journalctl -u "$unit" --no-pager > "$meta/journal.log"
cp /opt/r1/flop-flat-cloud32-start01.json /opt/r1/bootstrap-complete "$meta/"
date -u --iso-8601=seconds > "$meta/recovered-at.txt"
# Preserve the exact cloud32 directory layout, including partial/failed outputs.
# Incomplete archives have no checksum sidecar and are never overwritten.
python3 -B - "$archive" "$meta" "$package" <<'PY' > /tmp/flop-flat-cloud32-recovery01.json
import gzip,hashlib,io,json,os,stat,sys,tarfile
from pathlib import Path
archive,meta,package=map(Path,sys.argv[1:])
roots={'recovery/metadata':meta,'recovery/package':package}
for name,p in [('proof',Path('/opt/r1/flop-flat-cloud32-proof01')),
               ('recovery/wrapper',Path('/opt/r1/flop-flat-cloud32-wrapper01'))]:
    if p.exists(): roots[name]=p
rows=[]
def signature(p):
    s=p.lstat()
    if not stat.S_ISREG(s.st_mode): raise ValueError('Nonregular input: '+str(p))
    return [s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns,s.st_ctime_ns]
def digest(p):
    h=hashlib.sha256()
    with p.open('rb') as f:
        for b in iter(lambda:f.read(1024*1024),b''): h.update(b)
    return h.hexdigest()
for label,root in roots.items():
    if root.is_symlink(): raise ValueError('Root symlink')
    for p in sorted(root.rglob('*')):
        if p.is_symlink(): raise ValueError('Input symlink')
        if p.is_dir(): continue
        sig=signature(p); rel=p.relative_to(root).as_posix()
        member=rel if label=='proof' else label+'/'+rel
        rows.append({'member':member,'original':str(p),'bytes':sig[2],'sha256':digest(p),'signature':sig})
        if signature(p)!=sig: raise ValueError('Input changed while hashing')
if len({r['member'] for r in rows})!=len(rows) or any(r['member']=='recovery-manifest.json' for r in rows):
    raise ValueError('Reserved or duplicate archive member')
manifest={'schema':'r1.flat-ev-vm14-recovery/v1','scope':'Original bytes, including failed stages; campaign verification is separate',
          'unit_quiescence_checked':True,'proof_present':'proof' in roots,'files':rows,
          'publication':'Only a matching .sha256 sidecar marks a complete archive; partial outputs are retained'}
encoded=(json.dumps(manifest,indent=2)+'\n').encode()
limit=1024**3-len(encoded)-256
class Capped:
    def __init__(self,f): self.f,self.count=f,0
    def write(self,b):
        if self.count+len(b)>limit: raise ValueError('Recovery archive and sidecars exceed1GiB')
        self.count+=len(b); return self.f.write(b)
    def flush(self): self.f.flush()
with archive.open('xb') as raw:
    with gzip.GzipFile(filename='',fileobj=Capped(raw),mode='wb',mtime=0,compresslevel=1) as gz:
        with tarfile.open(fileobj=gz,mode='w|') as tar:
            for row in rows:
                p=Path(row['original'])
                if signature(p)!=row['signature']: raise ValueError('Input changed before archive')
                info=tarfile.TarInfo(row['member']);info.size=row['bytes'];info.mode=0o644;info.mtime=0
                with p.open('rb') as f: tar.addfile(info,f)
                if signature(p)!=row['signature']: raise ValueError('Input changed during archive')
            info=tarfile.TarInfo('recovery-manifest.json');info.size=len(encoded);info.mode=0o644;info.mtime=0
            tar.addfile(info,io.BytesIO(encoded))
# Verify the finished archive without importing or executing retained files.
expected={r['member']:r for r in rows};seen=set()
with tarfile.open(archive,'r:gz') as tar:
    for item in tar:
        if not item.isfile() or item.name in seen: raise ValueError('Invalid archive member')
        seen.add(item.name)
        with tar.extractfile(item) as f:
            if item.name=='recovery-manifest.json':
                if f.read()!=encoded: raise ValueError('Embedded manifest differs')
            else:
                h=hashlib.sha256();count=0
                for b in iter(lambda:f.read(1024*1024),b''): h.update(b);count+=len(b)
                row=expected[item.name]
                if count!=row['bytes'] or h.hexdigest()!=row['sha256']: raise ValueError('Archived content differs')
if seen!=set(expected)|{'recovery-manifest.json'}: raise ValueError('Archive membership differs')
for row in rows:
    if signature(Path(row['original']))!=row['signature']: raise ValueError('Input changed before publication')
with Path(str(archive)+'.manifest.json').open('xb') as f: f.write(encoded)
sha=digest(archive);side=(sha+'  '+archive.name+'\n').encode()
if archive.stat().st_size+len(encoded)+len(side)>1024**3: raise ValueError('Publication exceeds1GiB')
with Path(str(archive)+'.sha256').open('xb') as f: f.write(side)
print(json.dumps({'status':'original_bytes_verified','archive':str(archive),'bytes':archive.stat().st_size,
                  'sha256':sha,'files':len(rows),'proof_present':'proof' in roots,'cloud_mutations':False}))
PY
chmod 644 "$archive" "$archive.manifest.json" "$archive.sha256" /tmp/flop-flat-cloud32-recovery01.json
cat /tmp/flop-flat-cloud32-recovery01.json
