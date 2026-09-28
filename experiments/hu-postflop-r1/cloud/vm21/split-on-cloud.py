"""Split the verified persistent recovery archive into durable48MiB transport parts."""
import hashlib
import json
import os
from pathlib import Path

ROOT = Path('/opt/r1')
PREFIX = 'sparse-rank-proof01'


def main():
    archive = ROOT / (PREFIX + '.tar.gz')
    if archive.is_symlink() or not archive.is_file():
        raise ValueError('Regular archive required')
    record = json.loads((ROOT / 'sparse-rank-recovery01.json').read_text())
    if record['status'] != 'original_bytes_verified' or archive.stat().st_size != record['bytes'] or not 0 < record['bytes'] <= 255 * 1024**2:
        raise ValueError('Recovery publication absent')
    if (ROOT / (PREFIX + '.tar.gz.sha256')).read_text().split()[0] != record['sha256']:
        raise ValueError('Completion checksum differs')
    if list(ROOT.glob(PREFIX + '.part*')):
        raise ValueError('Existing transport parts; inspect rather than overwrite')
    combined, total, parts = hashlib.sha256(), 0, []
    with archive.open('rb') as source:
        index = 0
        while True:
            block = source.read(1024 * 1024)
            if not block:
                break
            path = ROOT / f'{PREFIX}.part{index:02d}'
            size, sha = 0, hashlib.sha256()
            with path.open('xb') as target:
                while block:
                    target.write(block); sha.update(block); combined.update(block)
                    size += len(block); total += len(block)
                    if size == 48 * 1024**2:
                        break
                    block = source.read(min(1024 * 1024, 48 * 1024**2 - size))
                target.flush(); os.fsync(target.fileno())
            path.chmod(0o644)
            parts.append({'path': str(path), 'bytes': size, 'sha256': sha.hexdigest()})
            index += 1
    if total != record['bytes'] or combined.hexdigest() != record['sha256']:
        raise ValueError('Split stream differs')
    fd = os.open(ROOT, os.O_RDONLY | os.O_DIRECTORY)
    try: os.fsync(fd)
    finally: os.close(fd)
    with (ROOT / (PREFIX + '.parts.sha256')).open('x') as target:
        for row in parts:
            target.write(row['sha256'] + '  ' + row['path'] + '\n')
        target.flush(); os.fsync(target.fileno())
    fd = os.open(ROOT, os.O_RDONLY | os.O_DIRECTORY)
    try: os.fsync(fd)
    finally: os.close(fd)
    files = list(parts)
    sidecars = [PREFIX + '.tar.gz.manifest.json', PREFIX + '.tar.gz.sha256',
                PREFIX + '.parts.sha256', 'sparse-rank-recovery01.json']
    sidecars += ['sparse-rank-analysis01.' + suffix for suffix in ('json', 'receipt.json', 'stdout.log', 'stderr.log')
                 if (ROOT / ('sparse-rank-analysis01.' + suffix)).exists()]
    for name in sidecars:
        path = ROOT / name
        if path.is_symlink() or not path.is_file() or path.stat().st_size > 1024**2:
            raise ValueError('Sidecar must be regular and at most1MiB')
        raw = path.read_bytes()
        files.append({'path': str(path), 'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()})
    payload = sum(row['bytes'] for row in files)
    if payload > 256 * 1024**2:
        raise ValueError('All intended transfer files exceed256MiB')
    transport = {'schema': 'r1.vm21-transport/v1', 'status': 'durable_transport_parts_verified',
                 'archive_sha256': record['sha256'], 'archive_bytes': record['bytes'],
                 'payload_bytes': payload, 'parts': parts, 'files': files,
                 'control_protocol_reserved_bytes': 64 * 1024**2,
                 'total_egress_envelope_bytes': 320 * 1024**2}
    raw = (json.dumps(transport, indent=2) + '\n').encode()
    if len(raw) > 64 * 1024:
        raise ValueError('Transport descriptor exceeds64KiB control allowance')
    with (ROOT / (PREFIX + '.transport.json')).open('xb') as target:
        target.write(raw); target.flush(); os.fsync(target.fileno())
    print(raw.decode(), end='')


if __name__ == '__main__':
    main()
