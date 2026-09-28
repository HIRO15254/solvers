"""Pure resource guard tests: no SDK, native binary, archive, or ledger mutation."""
import copy
import datetime as dt
from decimal import Decimal
from fractions import Fraction
import unittest
from unittest.mock import patch
import phase
import reserve


class ResourceGuards(unittest.TestCase):
    def setUp(self):
        self.stop = dt.datetime(2026, 9, 28, 12, tzinfo=dt.timezone.utc)
        base = 'https://www.googleapis.com/compute/v1/projects/' + reserve.PROJECT + '/zones/' + reserve.ZONE
        self.state = {'id': '123', 'name': reserve.NAME, 'selfLink': base + '/instances/' + reserve.NAME,
                      'status': 'TERMINATED', 'machineType': base + '/machineTypes/e2-standard-2',
                      'scheduling': {'provisioningModel': 'SPOT', 'instanceTerminationAction': 'STOP',
                                     'automaticRestart': False, 'terminationTime': self.stop.isoformat()}}

    def observed(self, state):
        with patch.object(phase, 'command', return_value=state):
            return phase.observed('pure-test', 'e2-standard-2', self.stop, '123')

    def test_stopped_same_small_instance_allowed(self):
        self.assertEqual(self.observed(self.state), self.state)

    def test_wrong_identity_type_or_running_rejected(self):
        for key, value in [('id', '456'), ('name', 'other'), ('selfLink', 'other'),
                           ('status', 'RUNNING'), ('machineType', self.state['machineType'].replace('standard-2', 'highcpu-32'))]:
            with self.subTest(key=key):
                state = copy.deepcopy(self.state)
                state[key] = value
                with self.assertRaises(ValueError):
                    self.observed(state)

    def test_changed_schedule_rejected(self):
        for key, value in [('provisioningModel', 'STANDARD'), ('instanceTerminationAction', 'DELETE'),
                           ('automaticRestart', True), ('terminationTime', (self.stop + dt.timedelta(seconds=1)).isoformat())]:
            with self.subTest(key=key):
                state = copy.deepcopy(self.state)
                state['scheduling'][key] = value
                with self.assertRaises(ValueError):
                    self.observed(state)

    def test_budget_exact_rational_and_smaller_available_bound(self):
        exact = (Fraction(47, 60) * Fraction('0.06951142') + Fraction(10, 60) * Fraction('.80')
                 + 20 * 24 * Fraction('.000137') + Fraction(320, 1024) * Fraction('.30') + 1)
        total = Decimal(reserve.arithmetic()['total_usd'])
        self.assertLess(abs(total - Decimal(exact.numerator) / Decimal(exact.denominator)), Decimal('1e-25'))
        self.assertLess(exact, Fraction('1.35'))
        self.assertGreater(exact, Fraction('1.30'))
        self.assertEqual(reserve.proposal()['maximum_download_gib'], 0.3125)
        self.assertEqual(reserve.proposal()['maximum_starts'], 3)

    def test_non_utc_and_naive_times_rejected(self):
        for value in ['2026-09-28T12:00:00', '2026-09-28T12:00:00+09:00']:
            with self.assertRaises(ValueError):
                reserve.utc(value)


if __name__ == '__main__':
    unittest.main()
