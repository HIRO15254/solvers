"""Pure transfer bounds; no remote call or real archive is opened."""
import copy
import hashlib
import json
import unittest
from unittest.mock import patch
from pathlib import Path
import download as d


class TransferBounds(unittest.TestCase):
    def setUp(self):
        part = {'path': '/opt/r1/' + d.PREFIX + '.part00', 'bytes': 2, 'sha256': '0' * 64}
        files = [part] + [{'path': '/opt/r1/' + n, 'bytes': 1, 'sha256': '1' * 64} for n in d.SIDECARS]
        self.transport = {'schema': 'r1.vm21-transport/v1', 'status': 'durable_transport_parts_verified',
                          'archive_sha256': '0' * 64, 'archive_bytes': 2, 'payload_bytes': 6,
                          'parts': [part], 'files': files, 'control_protocol_reserved_bytes': d.CONTROL_RESERVE,
                          'total_egress_envelope_bytes': d.TOTAL_CAP}

    def test_exact_small_inventory_allowed(self):
        self.assertEqual(len(d.validate_transport(self.transport)), 5)

    def test_duplicate_and_escaping_members_rejected(self):
        for path in ['/opt/r1/../sparse-rank-proof01.part00', '/tmp/sparse-rank-proof01.part00',
                     '/opt/r1/' + d.SIDECARS[0]]:
            value = copy.deepcopy(self.transport)
            value['files'][0]['path'] = path
            with self.assertRaises(ValueError):
                d.validate_transport(value)

    def test_part_size_hash_and_total_changes_rejected(self):
        for key, value in [('bytes', 3), ('sha256', 'x' * 64), ('bytes', -1)]:
            altered = copy.deepcopy(self.transport)
            altered['files'][0][key] = value
            with self.assertRaises(ValueError):
                d.validate_transport(altered)
        altered = copy.deepcopy(self.transport)
        altered['payload_bytes'] += 1
        with self.assertRaises(ValueError):
            d.validate_transport(altered)

    def test_empty_analysis_log_allowed_but_unknown_output_rejected(self):
        value = copy.deepcopy(self.transport)
        value['files'].append({'path': '/opt/r1/sparse-rank-analysis01.stderr.log', 'bytes': 0, 'sha256': 'e' * 64})
        self.assertEqual(len(d.validate_transport(value)), 6)
        value['files'][-1]['path'] = '/opt/r1/arbitrary.log'
        with self.assertRaises(ValueError):
            d.validate_transport(value)

    def test_failed_and_uncertain_intents_both_remain_charged(self):
        class Directory:
            def glob(self, unused):
                return [Path('transfer-complete.intent.json'), Path('transfer-uncertain.intent.json')]
        row = {'schema': 'r1.vm21-transfer-intent/v1', 'resource': d.r.NAME,
               'charged_payload_bytes': 48 * 1024**2, 'files': [{'bytes': 48 * 1024**2}]}
        with patch.object(d.r, 'read', return_value=row), patch.object(d.r, 'pin', return_value={'bytes': 1, 'sha256': 'a' * 64}):
            total, rows = d.prior_charge(Directory())
        self.assertEqual(total, 96 * 1024**2)
        self.assertEqual(len(rows), 2)
        self.assertGreater(total + 200 * 1024**2 + d.CONTROL_RESERVE, d.TOTAL_CAP)

    def test_zero_byte_sidecar_stream_and_intent_roundtrip(self):
        import io
        empty = hashlib.sha256(b'').hexdigest()
        value = copy.deepcopy(self.transport)
        value['files'].append({'path': '/opt/r1/sparse-rank-analysis01.stderr.log', 'bytes': 0, 'sha256': empty})
        parsed = d.validate_transport(json.loads(json.dumps(value)))
        row = parsed['sparse-rank-analysis01.stderr.log']
        with patch.object(Path, 'is_file', return_value=True), patch.object(Path, 'is_symlink', return_value=False), patch.object(Path, 'open', return_value=io.BytesIO(b'')):
            self.assertEqual(d.stream_pin(Path('known-empty-sidecar')), {'bytes': row['bytes'], 'sha256': row['sha256']})
        class Directory:
            def glob(self, unused):
                return [Path('transfer-empty.intent.json')]
        intent = {'schema': 'r1.vm21-transfer-intent/v1', 'resource': d.r.NAME,
                  'charged_payload_bytes': 0, 'files': [row]}
        with patch.object(d.r, 'read', return_value=json.loads(json.dumps(intent))), patch.object(d.r, 'pin', return_value={'bytes': 1, 'sha256': 'a' * 64}):
            total, records = d.prior_charge(Directory())
        self.assertEqual(total, 0)
        self.assertEqual(len(records), 1)


if __name__ == '__main__':
    unittest.main()
