"""Retain a failed source validation, excluding reproducible Cargo outputs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tarfile

parser = argparse.ArgumentParser()
parser.add_argument('--root', type=Path, required=True)
args = parser.parse_args()
root = args.root.resolve(strict=True)
state = json.loads((root / 'validation/result.json').read_text())
assert state['status'] == 'failed'
paths = [root / 'source-candidate.tar.gz', root / 'source-candidate-manifest.json']
paths.extend(sorted(path for path in (root / 'validation').rglob('*') if path.is_file()))
assert sum(path.stat().st_size for path in paths) < 64 * 1024**2
for path in paths:
    assert not path.is_symlink() and path.resolve().is_relative_to(root)
    with path.open('rb') as stream:
        os.fsync(stream.fileno())
archive = root.with_suffix('.validation-failure.tar.gz')
assert not archive.exists()
with tarfile.open(archive, 'w:gz') as stream:
    for path in paths:
        stream.add(path, arcname=path.relative_to(root).as_posix(), recursive=False)
with archive.open('rb') as stream:
    os.fsync(stream.fileno())
pin = {'archive': archive.name, 'bytes': archive.stat().st_size,
       'sha256': hashlib.sha256(archive.read_bytes()).hexdigest(), 'members': len(paths),
       'status': state['status'], 'error': state.get('error')}
with root.with_suffix('.validation-failure.json').open('x', encoding='utf-8', newline='\n') as stream:
    json.dump(pin, stream, indent=2)
    stream.write('\n')
    stream.flush()
    os.fsync(stream.fileno())
directory = os.open(root.parent, os.O_DIRECTORY)
try:
    os.fsync(directory)
finally:
    os.close(directory)
print(json.dumps(pin))
