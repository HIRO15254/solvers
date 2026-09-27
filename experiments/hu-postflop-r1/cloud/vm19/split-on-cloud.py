"""Split the verified persistent recovery archive into durable48MiB transport parts."""
import hashlib
import json
import os
from pathlib import Path

ROOT = Path('/opt/r1')
PREFIX = 'flop-cpu-profile-proof01'


def main():
    archive = ROOT / (PREFIX + '.tar.gz')
    record = json.loads((ROOT / 'flop-cpu-profile-recovery01.json').read_text())
    if record['status'] != 'original_bytes_verified' or archive.stat().st_size != record['bytes']:
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
    print(json.dumps({'status': 'durable_transport_parts_verified', 'archive_sha256': record['sha256'], 'parts': parts}))


if __name__ == '__main__':
    main()
