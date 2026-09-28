"""Download one finite VM21 file set with durable, cumulative precharged byte intents.

Only this wrapper may transfer files from VM21. Failed/uncertain attempts remain
fully charged. 64 MiB is separately reserved for bounded control/protocol traffic;
this is a conservative reservation model, not measured billable wire bytes.
"""
import argparse
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import reserve as r

PREFIX = 'sparse-rank-proof01'
PAYLOAD_CAP = 256 * 1024**2
CONTROL_RESERVE = 64 * 1024**2
TOTAL_CAP = 320 * 1024**2
PART_BYTES = 48 * 1024**2
SIDECARS = [PREFIX + '.tar.gz.manifest.json', PREFIX + '.tar.gz.sha256',
            PREFIX + '.parts.sha256', 'sparse-rank-recovery01.json']
OPTIONAL = {'sparse-rank-analysis01.' + suffix for suffix in ['json', 'receipt.json', 'stdout.log', 'stderr.log']}


def valid_label(label):
    r.need(bool(label) and len(label) <= 48 and all(c.isascii() and (c.isalnum() or c in '-_') for c in label),
           'Short ASCII label required')


def validate_transport(value):
    r.need(value['schema'] == 'r1.vm21-transport/v1' and value['status'] == 'durable_transport_parts_verified',
           'Published transport inventory required')
    r.need(value['control_protocol_reserved_bytes'] == CONTROL_RESERVE
           and value['total_egress_envelope_bytes'] == TOTAL_CAP, 'Transport limits differ')
    size = value['archive_bytes']
    r.need(type(size) is int and 0 < size <= 255 * 1024**2, 'Archive outside bound')
    parts = [f'{PREFIX}.part{i:02d}' for i in range(math.ceil(size / PART_BYTES))]
    files = value['files']
    actual_names = {PurePosixPath(row['path']).name for row in files}
    expected = set(parts + SIDECARS) | (actual_names & OPTIONAL)
    r.need(len(files) == len(expected), 'Unexpected file count')
    found = {}
    for row in files:
        path = PurePosixPath(row['path'])
        r.need(str(path) == row['path'] and path.parent == PurePosixPath('/opt/r1')
               and path.name in expected and path.name not in found, 'Path escape/duplicate/unexpected member')
        count, sha = row['bytes'], row['sha256']
        r.need(type(count) is int and count >= 0 and len(sha) == 64
               and all(c in '0123456789abcdef' for c in sha), 'Invalid file identity')
        if path.name in parts:
            index = parts.index(path.name)
            r.need(count == min(PART_BYTES, size - index * PART_BYTES), 'Part size/order differs')
        else:
            r.need(count <= 1024**2, 'Sidecar exceeds1MiB')
        found[path.name] = row
    r.need(set(found) == expected and sum(row['bytes'] for row in files) == value['payload_bytes']
           and value['payload_bytes'] <= PAYLOAD_CAP, 'Payload total differs/exceeds256MiB')
    r.need(value['parts'] == [found[n] for n in parts], 'Part inventory differs')
    r.need(len(value['archive_sha256']) == 64 and all(c in '0123456789abcdef' for c in value['archive_sha256']),
           'Invalid archive hash')
    return found


def prior_charge(directory):
    total, records = 0, []
    for path in sorted(directory.glob('transfer-*.intent.json')):
        item = r.read(path)
        r.need(item['schema'] == 'r1.vm21-transfer-intent/v1'
               and item['resource'] == r.NAME, 'Unknown transfer history')
        r.need(0 < len(item['files']) <= 14 and all(type(v['bytes']) is int and v['bytes'] >= 0 for v in item['files']),
               'Invalid prior intended file sizes')
        amount = item['charged_payload_bytes']
        r.need(type(amount) is int and amount >= 0
               and amount == sum(row['bytes'] for row in item['files']), 'Invalid prior charge')
        total += amount
        records.append({'path': path.name, 'pin': r.pin(path), 'charged_payload_bytes': amount})
    return total, records


def control_evidence(directory):
    # The remaining commands must have bounded small output. A fixed reserve
    # covers their future output and SSH/SCP protocol overhead, not only these logs.
    rows = []
    for path in sorted([*directory.glob('*.stdout.log'), *directory.glob('*.stderr.log')]):
        r.need(path.is_file() and not path.is_symlink(), 'Nonregular control output')
        count = path.stat().st_size
        r.need(count <= 4 * 1024**2, 'Unbounded control response needs review')
        rows.append({'path': path.name, 'bytes': count})
    total = sum(row['bytes'] for row in rows)
    r.need(total <= 8 * 1024**2, 'Control payload exceeds8MiB sublimit; preserve remaining protocol reserve')
    return {'observed_output_bytes': total, 'output_payload_sublimit_bytes': 8 * 1024**2,
            'reserved_control_protocol_bytes': CONTROL_RESERVE, 'files': rows,
            'meaning': 'Captured application output only; does not measure wire/retransmission/billed bytes'}


def stream_pin(path):
    r.need(path.is_file() and not path.is_symlink(), 'Regular downloaded file required')
    sha, count = hashlib.sha256(), 0
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            count += len(block)
            sha.update(block)
    return {'bytes': count, 'sha256': sha.hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--label', required=True)
    parser.add_argument('--split-label', default='split01')
    parser.add_argument('--files', nargs='+', help='Exact published basenames; omit to transfer the entire set once')
    args = parser.parse_args()
    valid_label(args.label)
    valid_label(args.split_label)
    r.authorized()
    creation = r.read(r.HERE / 'create.receipt.json')
    r.need(creation['exit_code'] == 0 and creation['reservation_id'] == r.ID, 'Successful VM21 creation required')
    stop = r.utc(creation['termination_time'])
    stem = args.split_label
    receipt = r.read(r.HERE / (stem + '.result.json'))
    for channel in ['stdout', 'stderr']:
        r.need(receipt[channel] == r.pin(r.HERE / (stem + '.' + channel + '.log')), 'Split receipt bytes changed')
    r.need(receipt['exit_code'] == 0 and receipt['stdout']['bytes'] <= 64 * 1024, 'Split publication failed/oversized')
    argv = receipt['argv'][1:]
    expected = ['compute', 'ssh', r.NAME, '--project=' + r.PROJECT, '--zone=' + r.ZONE,
                '--command=sudo python3 -B /opt/r1/sparse-rank-package/experiments/hu-postflop-r1/cloud/vm21/split-on-cloud.py',
                '--quiet']
    r.need(argv == expected, 'Split publication scope/command differs')
    manifest = r.read(r.HERE / (stem + '.stdout.log'))
    files = validate_transport(manifest)
    selected = list(files) if args.files is None else args.files
    r.need(bool(selected) and len(set(selected)) == len(selected) and set(selected) <= set(files),
           'Empty/duplicate/unpublished requested file')
    intended = [files[name] for name in selected]
    charge = sum(row['bytes'] for row in intended)
    lock = r.HERE / 'transfer.lock'
    # A crashed owner leaves this lock in place. Inspect before manually releasing;
    # an absent completion receipt never refunds the existing intent.
    with lock.open('x') as stream:
        stream.write(args.label + '\n')
        stream.flush()
        os.fsync(stream.fileno())
    try:
        before, prior = prior_charge(r.HERE)
        controls = control_evidence(r.HERE)
        r.need(before + charge + CONTROL_RESERVE <= TOTAL_CAP, 'Cumulative outbound reservation exceeds320MiB')
        destination = r.HERE / 'downloads' / args.label
        r.need(not (r.HERE / 'downloads').is_symlink() and destination.resolve().is_relative_to(r.HERE.resolve()),
               'Download destination escapes workspace')
        r.need(not destination.exists(), 'New transfer destination required; never overwrite a partial download')
        request = {'schema': 'r1.vm21-transfer-request/v1', 'resource': r.NAME, 'label': args.label,
                   'files': intended, 'destination': str(destination), 'intended_payload_bytes': charge,
                   'prior_payload_bytes': before, 'control_evidence': controls,
                   'note': 'This request is not a byte charge. Each file is charged in its own intent immediately before its single-source SCP.'}
        r.fresh_json(r.HERE / ('transfer-' + args.label + '.request.json'), request)
        destination.mkdir(parents=True)
        verified = []
        for index, row in enumerate(intended):
            label = args.label + '-' + str(index).zfill(2)
            remaining = int((stop - dt.datetime.now(dt.timezone.utc)).total_seconds())
            r.need(remaining > 90, 'Need transfer time plus60 seconds for deletion before original STOP')
            timeout = min(180, remaining - 60)
            before, prior = prior_charge(r.HERE)
            r.need(before + row['bytes'] + CONTROL_RESERVE <= TOTAL_CAP, 'Cumulative outbound reservation exceeds320MiB')
            intent = {'schema': 'r1.vm21-transfer-intent/v1', 'resource': r.NAME,
                      'at_utc': dt.datetime.now(dt.timezone.utc).isoformat(), 'label': label,
                      'transport': r.pin(r.HERE / (stem + '.stdout.log')), 'split_receipt': r.pin(r.HERE / (stem + '.result.json')),
                      'files': [row], 'destination': str(destination), 'charged_payload_bytes': row['bytes'],
                      'prior_payload_bytes': before, 'cumulative_payload_bytes': before + row['bytes'],
                      'total_reserved_outbound_bytes': before + row['bytes'] + CONTROL_RESERVE,
                      'sdk_timeout_seconds': timeout, 'original_stop_utc': stop.isoformat(),
                      'control_evidence': control_evidence(r.HERE), 'prior_intents': prior,
                      'failure_policy': 'This file remains fully charged on failure/uncertainty; unstarted suffix files are not charged; no automatic retry'}
            intent_path = r.HERE / ('transfer-' + label + '.intent.json')
            r.fresh_json(intent_path, intent)
            # One exact remote source supports Windows PSCP without multi-remote assumptions.
            sdk = ['compute', 'scp', r.NAME + ':' + row['path'], str(destination),
                   '--project=' + r.PROJECT, '--zone=' + r.ZONE, '--quiet']
            result = {'schema': 'r1.vm21-transfer-result/v1', 'label': label,
                      'intent': r.pin(intent_path), 'charged_payload_bytes': row['bytes'], 'charged_payload_refund_bytes': 0,
                      'exit_code': None, 'status': 'failed_or_uncertain', 'verified_files': []}
            try:
                done = subprocess.run([sys.executable, '-B', str(r.HERE / 'capture-command.py'), '--label', 'download-' + label,
                                       '--timeout', str(timeout), '--', *sdk], check=False)
                result['exit_code'] = done.returncode
                r.need(done.returncode == 0, 'SDK transfer failed or uncertain')
                path = destination / PurePosixPath(row['path']).name
                actual = stream_pin(path)
                r.need(actual == {key: row[key] for key in ['bytes', 'sha256']}, 'Downloaded identity differs')
                result['verified_files'].append({'path': str(path), **actual})
                result['status'] = 'intended_file_verified'
                verified.append(PurePosixPath(row['path']).name)
            except Exception as error:
                result['error'] = str(error)
                raise
            finally:
                result['ended_utc'] = dt.datetime.now(dt.timezone.utc).isoformat()
                r.fresh_json(r.HERE / ('transfer-' + label + '.result.json'), result)
        r.need(set(p.name for p in destination.iterdir()) == set(selected), 'Unexpected downloaded member')
        total, unused = prior_charge(r.HERE)
        record = {'schema': 'r1.vm21-transfer-completion/v1', 'status': 'all_intended_files_verified',
                  'destination': str(destination), 'verified_basenames': verified,
                  'cumulative_payload_bytes': total, 'reserved_control_protocol_bytes': CONTROL_RESERVE,
                  'control_evidence_after': control_evidence(r.HERE)}
        r.fresh_json(r.HERE / ('transfer-' + args.label + '.completed.json'), record)
        print(json.dumps(record))
    finally:
        lock.unlink()


if __name__ == '__main__':
    main()
