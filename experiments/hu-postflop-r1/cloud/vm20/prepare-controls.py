"""Derive local VM20 controls; reuse the exact VM19 deployment, no native/API work."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
BASE = HERE.parent / 'vm19'


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def once(text, old, new):
    assert text.count(old) == 1, old
    return text.replace(old, new)


def main():
    records = {}
    for name in ('capture-command.py', 'launch.py', 'check-download.py', 'split-on-cloud.py'):
        raw = (BASE / name).read_bytes()
        code = raw.decode().replace('VM19', 'VM20')
        if name == 'launch.py':
            code = code.replace('r1-20260927-19', 'r1-20260928-20')
            code = once(code, "GCLOUD = ", "TEMPLATE = HERE.parent / 'vm19'\nGCLOUD = ")
            for item in ('pack-receipt.json', 'source-manifest.json', 'bootstrap.sh'):
                code = code.replace("HERE / '" + item + "'", "TEMPLATE / '" + item + "'")
            code = once(code, "HERE / pack['archive']['path']", "TEMPLATE / pack['archive']['path']")
            code = once(code, "record = {'reservation_id': row['id'],", "record = {'template': '../vm19', 'fresh_instance_and_build': True, 'reservation_id': row['id'],")
        encoded = code.encode()
        with (HERE / name).open('xb') as stream:
            stream.write(encoded)
        records[name] = {'source': '../vm19/' + name, 'before': pin(raw), 'after': pin(encoded)}
    records['deployment'] = {'path': '../vm19/deployment01.tar.gz',
        'bytes': 1373074, 'sha256': 'ad156105b5323c91c4c9e715525e9efdf75ec07acb193db3eb3106581fec466f'}
    records['scope'] = 'New cloud resource and deadlines. Frozen VM19 internal path/unit/schema labels are reused verbatim; old VM19 runtime proof is not an input.'
    with (HERE / 'control-derivation.json').open('x') as stream:
        json.dump(records, stream, indent=2)
        stream.write('\n')


if __name__ == '__main__':
    main()
