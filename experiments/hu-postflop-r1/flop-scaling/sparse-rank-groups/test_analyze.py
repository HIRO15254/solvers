"""Small synthetic screen checks; never read a retained archive or start native code."""
import copy
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('sparse_test_analysis', HERE / 'analyze.py')
a = importlib.util.module_from_spec(spec)
spec.loader.exec_module(a)


def rows():
    return [dict(r, status='completed') for r in a.run.schedule()]


def measurements(row):
    factor = .9 if row['arm'] == 'candidate' else 1
    wall = (2 if row['workers'] == 16 else 1.5) * factor
    return {'cfr_wall_seconds': wall, 'quality_7_walks_wall_seconds': wall,
            'cfr_plus_quality_wall_seconds': wall * 2, 'root_os_peak_resident_bytes': 1000}


class ScreenChecks(unittest.TestCase):
    def test_paired_groups_and_scaling_never_adopt(self):
        with patch.object(a.old, 'observations', side_effect=measurements):
            result = a.summarize(rows())
        self.assertEqual(result['performance_screen'], 'passed')
        self.assertFalse(result['production_adoption'])
        self.assertEqual(len(result['groups']), 8)
        self.assertTrue(all(len(g['metrics']['cfr_wall_seconds']['samples']) == 3 for g in result['groups']))
        self.assertAlmostEqual(result['groups'][1]['same_arm_16worker_relative']['cfr_wall_seconds']['speedup'], 4 / 3)

    def test_missing_failed_or_reordered_solve_is_rejected(self):
        original = rows()
        failed = copy.deepcopy(original); failed[-1]['status'] = 'failed'
        reordered = copy.deepcopy(original); reordered[0], reordered[1] = reordered[1], reordered[0]
        for changed in (original[:-1], failed, reordered):
            with self.assertRaises(ValueError):
                a.summarize(changed)

    def test_warmup_excluded_but_variance_guard_enforced(self):
        def varied(row):
            result = measurements(row)
            if row['arm'] == 'candidate' and row['round'] == 3:
                result['cfr_wall_seconds'] *= 1.2
            return result
        with patch.object(a.old, 'observations', side_effect=varied):
            result = a.summarize(rows())
        self.assertEqual(result['performance_screen'], 'rejected')
        self.assertFalse(result['predeclared_guards']['all_cfr_quality_three_sample_max_over_min_at_most_1_15'])
        self.assertFalse(result['production_adoption'])

    def test_no_performance_claim_on_incomplete_proof(self):
        # An incomplete proof fails before any summary even if some timings exist.
        with patch.object(a, 'Evidence', side_effect=ValueError('missing retained evidence')):
            with patch.object(a, 'summarize') as summary:
                with self.assertRaises(ValueError):
                    a.check(Path('/not-a-proof'))
                summary.assert_not_called()


if __name__ == '__main__':
    unittest.main()
