"""Archive a terminal proof without executing retained evidence code."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tarfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    root = args.out.resolve(strict=True)
    terminal = root / 'prepare-failure.json'
    if not terminal.exists():
        terminal = root / 'result.json'
    state = json.loads(terminal.read_text())
    assert state['status'] in ('completed', 'failed'), 'proof must be terminal'
    files = [root / name for name in ('plan.json', 'result.json', 'retention.json',
                                     'verification.json', 'prepare-failure.json')
             if (root / name).is_file()]
    files.extend(sorted((root / 'payload').glob('**/*')))
    files = [path for path in files if path.is_file()]
    assert files and sum(path.stat().st_size for path in files) < 256 * 1024**2
    for path in files:
        assert not path.is_symlink() and path.resolve().is_relative_to(root)
        with path.open('rb') as stream:
            os.fsync(stream.fileno())
    archive = root.with_suffix('.tar.gz')
    assert not archive.exists(), 'never replace retained proof'
    with tarfile.open(archive, 'w:gz') as stream:
        for path in files:
            stream.add(path, arcname=path.relative_to(root).as_posix(), recursive=False)
    with archive.open('rb') as stream:
        os.fsync(stream.fileno())
    pin = {'archive': archive.name, 'bytes': archive.stat().st_size,
           'sha256': hashlib.sha256(archive.read_bytes()).hexdigest(),
           'terminal_status': state['status'], 'members': len(files)}
    report = root.with_suffix('.archive.json')
    with report.open('x', encoding='utf-8', newline='\n') as stream:
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


if __name__ == '__main__':
    main()
