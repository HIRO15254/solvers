"""Bind VM07's completed validation to a new codec measurement attestation.

The original validation record is preserved. Freeze/run independently checks
the resulting references before any new measurement.
"""
import importlib.util
from pathlib import Path

root = Path('/opt/r1')
script = root / 'codec-tools07/run_codec.py'
spec = importlib.util.spec_from_file_location('r1_codec', script)
codec = importlib.util.module_from_spec(spec)
spec.loader.exec_module(codec)
validation = root / 'codec-build07/result.json'
result = codec.read(validation)
codec.require(result['status'] == 'passed', 'source07 build not passed')
source = root / 'current'
record = {
    'schema': 'r1.codec-build-attestation/v1', 'status': 'completed',
    'issued_at': codec.now(),
    'settings': {'profile': 'release',
                 'rustflags': '-C target-cpu=native (.cargo/config.toml); no env override',
                 'target': 'x86_64-unknown-linux-gnu'},
    'host': codec.host(),
    'compiler': codec.identity(root / 'rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin/rustc'),
    'example': codec.identity(source / 'crates/formats/examples/sol_codec_bench.rs'),
    'validation_file': codec.identity(validation),
    'validation_stages': [codec.identity(validation.parent / f'{index:02d}-{stage["name"]}/supervisor.json')
                          for index, stage in enumerate(result['stages'])],
    'sides': {
        'baseline': {
            'revision': '88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0',
            'source': codec.identity(root / 'codec-baseline-source.tar.gz'),
            'binary': codec.identity(root / 'target/codec-baseline/release/examples/sol_codec_bench'),
        },
        'candidate': {
            'revision': '88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0',
            'source': codec.identity(root / 'source-07.tar.gz'),
            'binary': codec.identity(root / 'target/codec-current/release/examples/sol_codec_bench'),
            'source_note': 'Dirty source07 snapshot on the stated base commit; the archive pins changes.',
        },
    },
}
codec.require(record['sides']['baseline']['source']['sha256'] ==
              '3de4afef13082dca9e17c38c9cc5f5d1734855f017d812ecb04e9cdbe0f7da27', 'baseline archive changed')
codec.require(record['sides']['candidate']['source']['sha256'] ==
              'a6d346a033cf90f68adf131c7a14f5a754417cf0d952e1008d5736e7e9d6dd2a', 'source07 archive changed')
codec.write(root / 'codec-build-attestation07.json', record, exclusive=True)
print('Created separate source07 codec build attestation; freeze must validate it before measurement.')
