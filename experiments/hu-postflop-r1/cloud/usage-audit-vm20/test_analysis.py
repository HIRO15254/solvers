"""Small offline audit invariants; no SDK, network, archives, or native solver."""
from copy import deepcopy
from decimal import Decimal as D
import json
import unittest

import analyze as a
import analyze02 as final


class AuditChecks(unittest.TestCase):
    def test_replay_and_rounding_only_change(self):
        first, last = a.build_report(), final.build_report()
        self.assertEqual(last['cost']['components_usd'], first['cost']['components_usd'])
        self.assertEqual(last['cost']['model_usd'], first['cost']['model_usd'])
        self.assertEqual(last['cost']['proposed_hold_usd'], '1.35')
        self.assertEqual(last['cost']['proposed_restore_usd'], '0.50')
        self.assertEqual(last['budget_snapshot']['proposed_new_available_usd'], '1.35')
        self.assertEqual(last['cost']['network_allowance_gib'], '0.5')
        self.assertEqual(last['cost']['components_usd']['original_uncertainty_reserve'], '1')

    def test_network_ceiling_expands_for_observation_or_payload(self):
        for observed, payload in ((D(536870913), 0), (D(0), 536870913)):
            self.assertEqual(a.calculate(D(60), D(30), D(50), observed, payload)['network_allowance_gib'], '1')
        self.assertEqual(a.calculate(D(60), D(30), D(50), None, 0)['network_allowance_gib'], '0.5')

    def test_invalid_lifecycle_rejected(self):
        for whole, high, disk in ((60, 61, 60), (60, 30, 61), (60, 0, 60)):
            with self.assertRaises(ValueError):
                a.calculate(D(whole), D(high), D(disk), D(0), 0)

    def test_lifecycle_identity_deadline_and_resize_mutations_rejected(self):
        acquisition = a.read(a.HERE / 'acquisition.json')
        data = {name: json.loads((a.HERE / ref['path']).read_bytes())
                for name, ref in acquisition['inputs'].items()
                if name.endswith(('.json', '.stdout.log')) and
                (a.HERE / ref['path']).read_bytes().lstrip().startswith((b'{', b'['))}
        mutations = [
            ('vm20/state32-01.stdout.log', lambda item: item.update(id='wrong')),
            ('vm20/state32-01.stdout.log', lambda item: item['scheduling'].update(terminationTime='2026-09-28T06:14:20Z')),
            ('vm20/cleanup-operations01.stdout.log', lambda item: item.append(deepcopy(next(x for x in item if x['operationType'] == 'setMachineType')))),
            ('vm20/absence-disks01.stdout.log', lambda item: item.append({'name': 'unexpected'})),
        ]
        a.c.verify_lifecycle(data.__getitem__)
        for name, mutate in mutations:
            changed = deepcopy(data)
            mutate(changed[name])
            with self.assertRaises(ValueError):
                a.c.verify_lifecycle(changed.__getitem__)

    def test_duplicate_delta_rejected_and_gap_not_zero(self):
        point = {'interval': {'startTime': '2026-09-28T04:30:00Z', 'endTime': '2026-09-28T04:31:00Z'},
                 'value': {'int64Value': '5'}}
        with self.assertRaises(ValueError):
            a.summarize([point, deepcopy(point)], 'int64Value', a.c.LAUNCH, '2026-09-28T05:02:08Z')
        later = deepcopy(point)
        later['interval'] = {'startTime': '2026-09-28T04:32:00Z', 'endTime': '2026-09-28T04:33:00Z'}
        result = a.summarize([point, later], 'int64Value', a.c.LAUNCH, '2026-09-28T05:02:08Z')
        self.assertEqual(result['observed_sum'], '10')
        self.assertEqual(result['internal_gaps'][0]['seconds'], '60')
        self.assertIsNone(result['internal_gaps'][0]['unobserved_usage'])


if __name__ == '__main__':
    unittest.main()
