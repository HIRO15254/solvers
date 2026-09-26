"""Keep each completed run file as its original bytes in deterministic gzip."""
import gzip
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
RAW = Path('E:/codex-work/solvers/r1-context-adoption-20260926')


def pin(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def main():
    result = json.loads((RAW / 'result.json').read_text())
    assert result['state'] != 'running', 'do not retain an active run'
    assert not (HERE / 'manifest.json').exists(), 'do not overwrite evidence'
    manifest = {'schema': 'r1.context-adoption-retention/v1', 'raw_directory': str(RAW),
                'raw_availability': 'Retained on E: at acquisition; no deletion by this task.', 'files': {}}
    for path in sorted(RAW.rglob('*')):
        if not path.is_file():
            continue
        relative = path.relative_to(RAW).as_posix()
        data = path.read_bytes()
        compressed = gzip.compress(data, compresslevel=6, mtime=0)
        destination = HERE / 'raw' / (relative + '.gz')
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(compressed)
        assert gzip.decompress(destination.read_bytes()) == path.read_bytes()
        manifest['files'][relative] = {'retained_path': destination.relative_to(HERE).as_posix(),
                                       'original_path': str(path), 'original': pin(data), 'gzip': pin(compressed)}
    (HERE / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'raw_files': len(manifest['files']),
                      'gzip_bytes': sum(x['gzip']['bytes'] for x in manifest['files'].values())}))


if __name__ == '__main__':
    main()
