"""Re-encode the zstd chunk frames of a v3 `.sol` with other settings (scratch).

usage: size_proto.py SOL [SAMPLE_EVERY]
Byte shuffle splits each decoded chunk into its u16 low-byte and high-byte
planes (blocks and entry headers are even-length, so pairs stay aligned).
"""

import json
import struct
import sys
import time

import numpy as np
import zstandard

SOL = sys.argv[1]
EVERY = int(sys.argv[2]) if len(sys.argv) > 2 else 1
HEADER_LEN = 8 + 2 + 32 + 8 + 8 + 8 + 8 + 32 + 32

data = open(SOL, "rb").read()
metadata_len, directory_offset = struct.unpack_from("<QQ", data, 50)
pos = HEADER_LEN + metadata_len


def shuffle(raw):
    a = np.frombuffer(raw, np.uint8)
    if len(a) % 2:
        return raw
    return a.reshape(-1, 2).T.tobytes()


VARIANTS = {
    "l1": (1, False),
    "l3": (3, False),
    "l1-shuffle": (1, True),
    "l3-shuffle": (3, True),
    "l9-shuffle": (9, True),
    "l19-shuffle": (19, True),
}
totals = {k: {"bytes": 0, "enc_s": 0.0, "dec_s": 0.0} for k in VARIANTS}
stored = decoded = 0
index = 0
dctx = zstandard.ZstdDecompressor()
while pos < directory_offset:
    dobj = dctx.decompressobj()
    window = data[pos : min(directory_offset, pos + (8 << 20))]
    raw = dobj.decompress(window)
    frame_len = len(window) - len(dobj.unused_data)
    if index % EVERY == 0:
        stored += frame_len
        decoded += len(raw)
        for name, (level, shuf) in VARIANTS.items():
            t = time.perf_counter()
            src = shuffle(raw) if shuf else raw
            enc = zstandard.ZstdCompressor(level=level, write_content_size=True).compress(src)
            t2 = time.perf_counter()
            dctx.decompress(enc)
            t3 = time.perf_counter()
            totals[name]["bytes"] += len(enc)
            totals[name]["enc_s"] += t2 - t
            totals[name]["dec_s"] += t3 - t2
    pos += frame_len
    index += 1

print(json.dumps({
    "sol": SOL, "chunks": index, "sample_every": EVERY,
    "sampled_stored_bytes": stored, "sampled_decoded_bytes": decoded,
    "variants": {
        k: {
            "ratio_vs_stored": round(v["bytes"] / stored, 4),
            "enc_mb_s": round(decoded / 1e6 / v["enc_s"], 1),
            "dec_mb_s": round(decoded / 1e6 / v["dec_s"], 1),
        }
        for k, v in totals.items()
    },
}, indent=1))
