"""Create the finite VM12 deployment package from frozen, trusted local inputs."""
from pathlib import Path
import datetime
import gzip
import hashlib
import io
import json
import tarfile

HERE = Path(__file__).resolve().parent
BASE = HERE.parents[1]


def fingerprint(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def main():
    frozen_dir = BASE / 'current-scaling32'
    frozen = json.loads((frozen_dir / 'freeze.json').read_bytes())
    assert frozen['prospective'] and frozen['samples'] == 36 and frozen['workers'] == [1, 16, 32]
    inputs = {}
    for name, pin in frozen['files'].items():
        raw = (frozen_dir / name).read_bytes()
        assert fingerprint(raw) == pin, name
        inputs['controls/current-scaling32/' + name] = raw
    inputs['controls/current-scaling32/freeze.json'] = (frozen_dir / 'freeze.json').read_bytes()
    for name in ['exact-mass', 'showdown-kernel']:
        inputs['controls/' + name + '/run.py'] = (BASE / name / 'run.py').read_bytes()
    inputs['exact-proof04.tar.gz'] = (BASE / 'exact-mass/exact-proof04.tar.gz').read_bytes()
    assert fingerprint(inputs['exact-proof04.tar.gz']) == {
        'bytes': 8231258, 'sha256': 'b7361b4adc579dc87c36e8c9cd37b1c307465db90fa6432eb1bc24d67163da37'}
    inputs['bundle-final-proof.py'] = (HERE.parent / 'bundle-final-proof.py').read_bytes()
    for name in ['install-inputs.py', 'fetch-dependencies.sh', 'run-current32.sh', 'start-current32.py', 'recover-current32.sh']:
        inputs[name] = (HERE / name).read_bytes()
    manifest = {'schema': 'r1.current32-deployment-inputs/v1',
                'created_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                'files': [{'path': name, **fingerprint(raw)} for name, raw in sorted(inputs.items())]}
    manifest_raw = (json.dumps(manifest, indent=2) + '\n').encode()
    with (HERE / 'manifest.json').open('xb') as stream:
        stream.write(manifest_raw)
    inputs['manifest.json'] = manifest_raw
    archive = HERE / 'current32-deployment01.tar.gz'
    with archive.open('xb') as stream:
        with gzip.GzipFile(fileobj=stream, filename='', mode='wb', mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode='w|') as tar:
                for name, raw in sorted(inputs.items()):
                    assert not name.startswith('/') and '..' not in name.split('/')
                    info = tarfile.TarInfo(name)
                    info.size, info.mode, info.mtime = len(raw), 0o644, 0
                    tar.addfile(info, io.BytesIO(raw))
    receipt = {'schema': 'r1.current32-deployment-package/v1', 'archive': archive.name,
               **fingerprint(archive.read_bytes()), 'members': len(inputs),
               'manifest': fingerprint(manifest_raw), 'packer': fingerprint(Path(__file__).read_bytes())}
    with (HERE / 'pack-receipt.json').open('x', encoding='utf-8') as stream:
        json.dump(receipt, stream, indent=2)
        stream.write('\n')
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
