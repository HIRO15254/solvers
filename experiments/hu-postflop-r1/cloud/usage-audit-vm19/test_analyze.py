"""Tiny metadata/arithmetic fixtures only: no native work, API, archive or budget writes."""
import copy
from decimal import Decimal as D
import importlib.util
import json
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location('vm19_analyze', HERE / 'analyze.py')
a = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(a)


class AuditTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.acquisition = json.loads((HERE / 'acquisition.json').read_bytes())
        cls.inputs = {name: json.loads((HERE / ref['path']).read_bytes())
                      for name, ref in cls.acquisition['inputs'].items()
                      if name.endswith('.json') or name.endswith('.stdout.log') and name.startswith('vm19/') and name.split('/')[-1].startswith(('state2-', 'resume-state', 'recovery-state', 'cleanup-operations', 'absence-', 'disk-state'))}
        cls.metrics = {q['response']['path']: json.loads((HERE / q['response']['path']).read_bytes())
                       for q in cls.acquisition['requests']}

    def test_immutable_report_replays(self):
        report = a.build_report()
        self.assertEqual(report['cost']['proposed_restore_usd'], '0.55')
        self.assertEqual(report['budget_snapshot']['held_usd'], '39.95')
        self.assertFalse(report['budget_mutations'])

    def test_separate_windows_disk_full_lifecycle(self):
        cost = a.calculate(D('2436.085089'), D('393.598136'), D('70657.603303'), D('6050550'), 4668141)
        self.assertEqual((cost['original_window_minutes'], cost['recovery_window_minutes'], cost['disk_minutes']), (43, 9, 1180))
        self.assertEqual(cost['proposed_hold_usd'], '1.3')
        self.assertEqual(cost['components_usd']['original_uncertainty_reserve'], '1')

    def test_large_egress_increases_allowance(self):
        cost = a.calculate(D(2400), D(400), D(70000), D(1024**3), 100)
        self.assertEqual(cost['network_allowance_gib'], '1')
        self.assertGreater(D(cost['proposed_hold_usd']), D('1.3'))

    def test_unknown_observation_is_not_zero(self):
        result = a.summarize([], 'int64Value', a.c.LAUNCH, a.c.RECOVERY_STOP)
        self.assertIsNone(result['observed_sum'])
        self.assertIsNone(result['unobserved_usage'])

    def assert_bad_lifecycle(self, change):
        inputs = copy.deepcopy(self.inputs)
        change(inputs)
        with self.assertRaises(ValueError):
            a.c.verify_lifecycle(inputs.__getitem__)

    def test_wrong_instance_rejected(self):
        self.assert_bad_lifecycle(lambda x: x['vm19/recovery-state02.stdout.log'].__setitem__('id', 'other'))

    def test_unamended_recovery_deadline_rejected(self):
        self.assert_bad_lifecycle(lambda x: x['vm19/recovery-only-amendment.json'].__setitem__('recovery_stop_utc', a.c.ORIGINAL_STOP))

    def test_present_disk_rejected(self):
        self.assert_bad_lifecycle(lambda x: x['vm19/absence-disks01.stdout.log'].append({'name': a.c.INSTANCE_NAME}))

    def test_extra_resize_rejected(self):
        self.assert_bad_lifecycle(lambda x: x['vm19/cleanup-operations01.stdout.log'].append({'operationType': 'setMachineType'}))

    def assert_bad_metric(self, change):
        metrics = copy.deepcopy(self.metrics)
        change(metrics['vm19-sent-00.json'])
        with self.assertRaises(ValueError):
            a.verify_metrics(self.acquisition, lambda ref: metrics[ref['path']])

    def test_wrong_metric_zone_rejected(self):
        self.assert_bad_metric(lambda x: x['timeSeries'][0]['resource']['labels'].__setitem__('zone', 'other'))

    def test_wrong_metric_unit_rejected(self):
        self.assert_bad_metric(lambda x: x.__setitem__('unit', 's'))

    def test_missing_metric_page_rejected(self):
        self.assert_bad_metric(lambda x: x.__setitem__('nextPageToken', 'unfetched'))

    def test_negative_metric_rejected(self):
        self.assert_bad_metric(lambda x: x['timeSeries'][0]['points'][0]['value'].__setitem__('int64Value', '-1'))

    def test_repeated_metric_interval_rejected(self):
        self.assert_bad_metric(lambda x: x['timeSeries'][0]['points'][0].__setitem__('interval', copy.deepcopy(x['timeSeries'][0]['points'][1]['interval'])))


if __name__ == '__main__':
    unittest.main()
