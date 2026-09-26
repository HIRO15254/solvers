"""Archive the portable CAS proof after all writers have stopped, excluding duplicates."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tarfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--proof', type=Path, required=True)
    parser.add_argument('--archive', type=Path, required=True)
    args = parser.parse_args()
    root = args.proof.resolve(strict=True)
    index = json.loads((root / 'retention.json').read_text())
    expected = {row['sha256'] for row in index['files'].values()}
    assert all(re.fullmatch(r'[0-9a-f]{64}', name) for name in expected)
    assert {p.name for p in (root / 'payload').iterdir()} == expected
    names = ['plan.json', 'result.json', 'retention.json', 'verification.json']
    names += ['payload/' + name for name in sorted(expected)]
    paths = [root / name for name in names]
    assert all(p.is_file() and not p.is_symlink() for p in paths)
    size = sum(p.stat().st_size for p in paths)
    assert size < 1024**3, 'proof exceeds download reservation'
    assert not args.archive.exists()
    with tarfile.open(args.archive, 'x:gz', compresslevel=6) as archive:
        for name, path in zip(names, paths):
            archive.add(path, arcname='proof/' + name, recursive=False)
    # A completed process record alone does not imply its raw files survived
    # Spot termination. Persist the complete self-contained bundle before
    # declaring it ready for transfer; local verification still gates deletion.
    with args.archive.open('rb') as archive:
        os.fsync(archive.fileno())
    if os.name == 'posix':
        descriptor = os.open(args.archive.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    data = args.archive.read_bytes()
    report = {'archive': str(args.archive), 'bytes': len(data),
              'sha256': hashlib.sha256(data).hexdigest(),
              'original_bytes': size, 'members': len(paths),
              'selected': 'plan/result/retention/verification and every CAS payload; stages are duplicate originals'}
    args.archive.with_suffix(args.archive.suffix + '.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
