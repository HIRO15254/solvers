"""Retain one stopped collector acquisition as exact, SHA-deduplicated gzip blobs."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import tarfile


def require(condition, message):
    if not condition:
        raise ValueError(message)


def fingerprint(stream):
    size, digest = 0, hashlib.sha256()
    for chunk in iter(lambda: stream.read(1024 * 1024), b''):
        size += len(chunk)
        digest.update(chunk)
    return {'bytes': size, 'sha256': digest.hexdigest()}


def file_pin(path):
    with Path(path).open('rb') as stream:
        return fingerprint(stream)


def content(value):
    return {key: value[key] for key in ('bytes', 'sha256')}


def read_json(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, 'duplicate JSON key: ' + key)
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def retain(bundle, sidecar, sha_file, output):
    bundle, sidecar, sha_file, output = map(Path, (bundle, sidecar, sha_file, output))
    require(not output.exists(), 'refusing to overwrite a retained acquisition')
    before = file_pin(bundle)
    digest_line = sha_file.read_text(encoding='ascii').strip()
    match = re.fullmatch(r'([0-9a-f]{64})  (.+)', digest_line)
    require(match and match[1] == before['sha256'] and match[2] == bundle.name, 'collector archive SHA sidecar differs')
    sidecar_bytes = sidecar.read_bytes()
    collector = read_json(sidecar_bytes)
    require(collector['schema'] == 'solvers.r1-retention/v1', 'unexpected collector schema')
    require(collector['archive_filename'] == bundle.name, 'wrong collector archive name')
    rows = collector['files']
    require(len({row['original_path'] for row in rows}) == len(rows), 'duplicate original path')
    included = {row['archive_member']: row for row in rows if row['included']}
    require(len(included) == sum(row['included'] for row in rows), 'duplicate included member')
    require(all(row['kind'] == 'regular' for row in included.values()), 'included non-file')
    require(all(re.fullmatch(r'files/\d{8}', name) for name in included), 'unexpected collector member name')
    output.mkdir(parents=True)
    (output / 'blobs').mkdir()
    blobs, originals = {}, {}

    def keep(stream, expected):
        name = 'blobs/' + expected['sha256'] + '.gz'
        destination = output / name
        size, digest = 0, hashlib.sha256()
        if name in blobs:
            require(blobs[name]['decoded'] == expected, 'same SHA with different size')
            # Actual retained duplicate bytes, not only their digest, must match.
            with gzip.open(destination, 'rb') as previous:
                while chunk := stream.read(1024 * 1024):
                    require(previous.read(len(chunk)) == chunk, 'duplicate payload differs')
                    size += len(chunk)
                    digest.update(chunk)
                require(not previous.read(1), 'duplicate payload is shorter')
        else:
            with destination.open('xb') as raw:
                with gzip.GzipFile(fileobj=raw, filename='', mode='wb', mtime=0, compresslevel=6) as compressed:
                    while chunk := stream.read(1024 * 1024):
                        size += len(chunk)
                        digest.update(chunk)
                        compressed.write(chunk)
            blobs[name] = {'encoding': 'gzip', **file_pin(destination), 'decoded': expected}
        require({'bytes': size, 'sha256': digest.hexdigest()} == expected, 'collector payload hash differs')
        return name

    seen, manifest_seen = set(), False
    with tarfile.open(bundle, 'r|gz') as archive:
        for member in archive:
            require(member.isfile(), 'collector contains a non-file member')
            require(member.name not in seen, 'duplicate tar member')
            seen.add(member.name)
            stream = archive.extractfile(member)
            if member.name == 'retention-manifest.json':
                require(not manifest_seen and member.size == len(sidecar_bytes), 'collector manifest size differs')
                require(stream.read() == sidecar_bytes, 'embedded manifest is not original sidecar bytes')
                manifest_seen = True
            else:
                require(member.name in included, 'unexpected/non-included payload in archive')
                row = included[member.name]
                require(member.size == row['bytes'], 'collector member size differs')
                originals[row['original_path']] = keep(stream, content(row))
    require(manifest_seen and seen == {'retention-manifest.json', *included}, 'missing collector member')
    require(file_pin(bundle) == before, 'collector archive changed during retention')
    recovered, missing = [], []
    for row in rows:
        if row['included'] or row['kind'] != 'regular':
            continue
        name = 'blobs/' + row['sha256'] + '.gz'
        if name in blobs and blobs[name]['decoded'] == content(row):
            originals[row['original_path']] = name
            recovered.append(row['original_path'])
        else:
            missing.append(row['original_path'])
    manifest_blob = keep(io.BytesIO(sidecar_bytes), fingerprint(io.BytesIO(sidecar_bytes)))
    sha_bytes = sha_file.read_bytes()
    sha_blob = keep(io.BytesIO(sha_bytes), fingerprint(io.BytesIO(sha_bytes)))
    manifest = {'schema': 'r1.context-linux-retention/v1', 'blobs': blobs, 'originals': originals,
                'collector': {'archive_name': bundle.name, 'archive': before, 'manifest_blob': manifest_blob,
                              'sha256_blob': sha_blob, 'hash_recovered_paths': recovered, 'missing_paths': missing},
                'availability': 'All original aliases resolve to exact retained bytes. Capacity-skipped aliases are recovered only when their collector size/SHA equals an included payload. The original collector archive is identified, not duplicated here.',
                'tool_availability': 'Compiler and Python executables are identity-only. Source archives, frozen benchmark binaries, inputs, runner and raw stage records are required by the portable verifier.'}
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    return {'retained_blobs': len(blobs), 'original_paths': len(originals),
            'hash_recovered_paths': len(recovered), 'missing_unique_paths': len(missing),
            'gzip_bytes': sum(value['bytes'] for value in blobs.values())}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--sha256', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    result = retain(args.bundle, args.manifest or Path(str(args.bundle) + '.manifest.json'),
                    args.sha256 or Path(str(args.bundle) + '.sha256'), args.out)
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
