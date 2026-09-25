#!/bin/bash
# Run after all timing campaigns stop; these are uncertified reference diagnostics.
set -euxo pipefail
cd /opt/r1/candidate-v3
test -f /opt/r1/comparison-complete
python3 - <<'PY'
import hashlib, pathlib
checks = {
 '/home/PC_User/diagnostic-input.tar.gz': '9bfc777776a0b85ec073498d75a9c6d23d35ed949610a3ee83a7c4ca73f83a7a',
 '/home/PC_User/diagnostic017-input.tar.gz': 'da6c44d7d09cf97342a5f82d9698ae4eaa511d21b4eabe3e6b05ccef962264b2',
 '/opt/r1/target/candidate-v3/release/solvers': 'a74e873ba0629c072884edf344ca41fb6ccd291d0a869f4d5e9b75f9dfbb0521',
 '/opt/r1/candidate-v3-source.tar.gz': 'ac5d493a129cd97be9322598867fa3ab5795ba6d3241a93e2719beaa2dd89970',
}
for path, digest in checks.items():
    assert hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest() == digest, path
PY
mkdir /opt/r1/river-diagnostic-inputs
tar --no-same-owner -xzf /home/PC_User/diagnostic-input.tar.gz -C /opt/r1/river-diagnostic-inputs
tar --no-same-owner -xzf /home/PC_User/diagnostic017-input.tar.gz -C /opt/r1/river-diagnostic-inputs
for number in 019 017; do
    if [ "$number" = 019 ]; then
        archive=/home/PC_User/diagnostic-input.tar.gz
        digest=9bfc777776a0b85ec073498d75a9c6d23d35ed949610a3ee83a7c4ca73f83a7a
    else
        archive=/home/PC_User/diagnostic017-input.tar.gz
        digest=da6c44d7d09cf97342a5f82d9698ae4eaa511d21b4eabe3e6b05ccef962264b2
    fi
    python3 /home/PC_User/run-diagnostic.py --case-id HU-R0-$number \
        --input-dir /opt/r1/river-diagnostic-inputs/HU-R0-$number \
        --input-archive "$archive" --input-archive-sha256 "$digest" \
        --binary-sha256 a74e873ba0629c072884edf344ca41fb6ccd291d0a869f4d5e9b75f9dfbb0521 \
        --source-id source-archive-sha256:ac5d493a129cd97be9322598867fa3ab5795ba6d3241a93e2719beaa2dd89970 \
        --output /opt/r1/diagnostic-$number
done
date -u --iso-8601=seconds > /opt/r1/river-diagnostics-complete
