import copy
import hashlib
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import summarize_dense_merge as report


class DenseMergeSummaryTests(unittest.TestCase):
    def test_only_documented_clocks_are_removed(self):
        raw = dict(constructionElapsedSecs=1, freshTraining=dict(solveElapsedSecs=2, metrics=dict(sweeps=8192)),
                   deviatorTraining=dict(elapsedSecs=3, coverage=[dict(traversals=1)]),
                   evaluations=[dict(elapsedSecs=4, result=dict(value=-0.25))],
                   policySupport=[dict(rows=[dict(regrets=[-0.0, 1], strategySum=[3, 7])])])
        before = copy.deepcopy(raw)
        changed = copy.deepcopy(raw)
        changed['constructionElapsedSecs'] = 10
        changed['freshTraining']['solveElapsedSecs'] = 20
        changed['deviatorTraining']['elapsedSecs'] = 30
        changed['evaluations'][0]['elapsedSecs'] = 40
        self.assertEqual(report.normalized(raw), report.normalized(changed))
        self.assertEqual(raw, before)
        for path in ('metrics', 'support', 'evaluation', 'coverage'):
            c = copy.deepcopy(changed)
            if path == 'metrics':
                c['freshTraining']['metrics']['sweeps'] += 1
            elif path == 'support':
                c['policySupport'][0]['rows'][0]['regrets'][0] = 0.0
            elif path == 'evaluation':
                c['evaluations'][0]['result']['value'] = 0
            else:
                c['deviatorTraining']['coverage'][0]['traversals'] = 2
            self.assertNotEqual(report.serialized(report.normalized(raw)), report.serialized(report.normalized(c)))

    def cases(self):
        cases = []
        for index, name in enumerate(report.ORDER):
            kind = name.rsplit('-', 1)[0]
            repeat = int(name.rsplit('-', 1)[1])
            train = repeat * 2 + (kind == 'transactional')
            cases.append(dict(case=name, implementation=kind, trainSeconds=train,
                              constructionSeconds=1, wallSeconds=train + 2, peakWorkingSetBytes=100,
                              startedUnixSeconds=index * 100, finishedUnixSeconds=index * 100 + train + 2,
                              normalizedOutputSha256=hashlib.sha256(b'identical').hexdigest()))
        return cases

    def test_complete_medians_use_all_repeats(self):
        result = report.compare(self.cases())
        self.assertEqual(result['medians']['legacy']['trainSeconds'], 4)
        self.assertEqual(result['medians']['transactional']['trainSeconds'], 5)
        self.assertEqual(result['transactionalToLegacyRatios']['trainSeconds'], 1.25)
        self.assertEqual(result['independentLearningSeeds'], 1)
        self.assertFalse(result['completeProductionStateEqualityClaim'])

    def test_missing_reordered_mismatched_and_overlapping_cases_reject(self):
        cases = self.cases()
        bad = [cases[:-1], list(reversed(cases))]
        mismatch = copy.deepcopy(cases)
        mismatch[-1]['normalizedOutputSha256'] = 'different'
        bad.append(mismatch)
        overlap = copy.deepcopy(cases)
        overlap[0]['finishedUnixSeconds'] = overlap[1]['startedUnixSeconds'] + 1
        bad.append(overlap)
        for candidate in bad:
            with self.subTest(candidate=candidate), self.assertRaises(ValueError):
                report.compare(candidate)

    def test_literal_arguments_keep_positive_candidate_budget_and_exact_support(self):
        args = report.arguments(dict(config='config.toml', cacheDir='.cache/bench-ehs'))
        self.assertEqual(args[args.index('--fresh-sweeps') + 1], '8192')
        self.assertEqual(args[args.index('--br-traversals') + 1], '1')
        self.assertEqual([args[i+1] for i, arg in enumerate(args) if arg == '--support-node'], report.SUPPORTS)
        self.assertNotIn('--endpoint-prefix', args)
        self.assertNotIn('--checkpoint', args)

    def test_nonfinite_or_nonpositive_costs_and_nonfinite_output_reject(self):
        for value in (0, -1, True, float('nan'), float('inf')):
            with self.subTest(value=value), self.assertRaises(ValueError):
                report.positive(value)
        with self.assertRaises(ValueError):
            report.serialized(dict(value=float('nan')))

    def test_fixed_training_config_and_completed_sweeps(self):
        raw = dict(schemaVersion='solvers.multiway-checkpoint-audit/v1', sweeps=8192, solverStateVersion=4,
                   freshTraining=dict(requestedSweeps=8192, metrics=dict(sweeps=8192),
                                      solverConfig=dict(seed=0, sweep_batch=4, traverser_vector=True)))
        report.training_checks(raw)
        for field, value in [('seed', 11), ('sweep_batch', 1), ('traverser_vector', False)]:
            altered = copy.deepcopy(raw)
            altered['freshTraining']['solverConfig'][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                report.training_checks(altered)
        raw['freshTraining']['metrics']['sweeps'] = 8191
        with self.assertRaises(ValueError):
            report.training_checks(raw)

    def test_decisive_regression_and_exact_green_count(self):
        decisive = 'dense_merge_late_slot_overflow_preserves_entire_state'
        red = decisive + ' ... FAILED\ntest result: FAILED. 0 passed; 1 failed;'
        green = decisive + ' ... ok\ntest result: ok. 7 passed; 0 failed;'
        report.regression_checks(red, green, 7)
        for red_log, green_log, count in [(red, green, 6), (red, green.replace(decisive, 'other_test'), 7),
                                           (red, green.replace('7 passed', '6 passed'), 7),
                                           (red.replace(decisive, 'other_test'), green, 7)]:
            with self.subTest(count=count), self.assertRaises(ValueError):
                report.regression_checks(red_log, green_log, count)


if __name__ == '__main__':
    unittest.main()
