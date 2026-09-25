import copy
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import summarize_whole_preflop_deviation as report


def estimate(mean, stderr=0.25):
    return dict(mean=mean, stderr=stderr,
                ci95=[mean - 1.96 * stderr, mean + 1.96 * stderr])


def visits(**values):
    return {street: values.get(street, 0) for street in ('preflop', 'flop', 'turn', 'river')}


def coverage(candidate):
    if candidate:
        parts = dict(decision_visits=visits(preflop=40, flop=8),
                     stored_strategy_visits=visits(preflop=30, flop=5),
                     uniform_fallback_visits=visits(preflop=10, flop=3),
                     average_strategy_visits=visits(preflop=20, flop=3),
                     current_strategy_visits=visits(),
                     regret_fallback_visits=visits(preflop=10, flop=2))
    else:
        parts = dict(decision_visits=visits(preflop=40, flop=8),
                     trained_action_visits=visits(preflop=24),
                     baseline_fallback_visits=visits(preflop=16, flop=8))
    parts = {name: {street: count * 1024 for street, count in value.items()}
             for name, value in parts.items()}
    return {**{name: sum(value.values()) for name, value in parts.items()},
            **{name + '_by_street': value for name, value in parts.items()}}


def fixture():
    """Only 12 estimate rows; budgets are metadata, not simulated worlds."""
    held_out = []
    for seed_index, seed in enumerate((2701, 2702)):
        baseline = [estimate(seat - 2.5) for seat in range(6)]
        gains = [estimate((seat - 3) * 0.5 - seed_index * 0.125) for seat in range(6)]
        held_out.append(dict(
            seed=seed, samples=32768, elapsedSecs=0.5,
            totalDealAttempts=33000,
            baseline=baseline,
            deviating=[estimate(base['mean'] + gain['mean'])
                       for base, gain in zip(baseline, gains)],
            gains=gains,
            coverage=[coverage(False) for _ in range(6)],
            candidatePolicyCoverage=[coverage(True) for _ in range(6)]))
    return dict(
        schemaVersion='solvers.multiway-preflop-deviation/v1',
        scope='all-preflop-decisions-with-frozen-postflop',
        config=dict(fitTraversalsPerSeat=8192, fitSeed=2601,
                    heldOutSamples=32768, heldOutSeeds=[2701, 2702]),
        variant=dict(purify_threshold=0.0, use_current_strategy=False),
        minFitVisits=8, fitElapsedSecs=1.0, fitPolicyFingerprint='a' * 64,
        maxBufferedSamples=4096,
        fitCoverage=[dict(traversals=8192, visited_infosets=3, retained_infosets=2,
                          total_visits=8196, retained_visits=8192) for _ in range(6)],
        heldOut=held_out)


def old_fixture():
    """Exercise exact normalization boundaries, not the old raw validator."""
    return dict(constructionElapsedSecs=1.0,
                freshTraining=dict(solveElapsedSecs=2.0, metrics=dict(sweeps=8192)),
                deviatorTraining=dict(elapsedSecs=0.2, traversalsPerSeat=1),
                evaluations=[dict(elapsedSecs=0.3, result=dict(mean=-0.5, elapsedSecs=42.0))],
                preflopSupportCensus=dict(elapsedSecs=0.4, result=dict(rawStateFingerprint='b' * 64)),
                policySupport=[dict(regrets=[-0.0, 1.0], strategySum=[1.0, 0.0])],
                interpretation=report.LEGACY_INTERPRETATION)


class WholePreflopDeviationTests(unittest.TestCase):
    def test_retains_every_seat_and_seed_with_negative_gains(self):
        value = fixture()
        before = copy.deepcopy(value)
        rows = report.diagnostic(value)
        self.assertEqual([(row['seed'], row['seat']) for row in rows],
                         [(seed, seat) for seed in (2701, 2702) for seat in range(6)])
        self.assertEqual([row['position'] for row in rows[:6]],
                         ['BTN', 'SB', 'BB', 'UTG', 'HJ', 'CO'])
        self.assertEqual(rows[0]['gain'], estimate(-1.5))
        self.assertEqual(rows[6]['gain'], estimate(-1.625))
        self.assertEqual(rows[0]['preflopTrainedVisitFraction'], 24 / 40)
        self.assertEqual(value, before)
        rows[0]['gain']['mean'] = 999
        self.assertEqual(value, before, 'summary estimates must not alias input rows')

    def test_zero_decision_coverage_is_unmeasured_not_full_coverage(self):
        value = fixture()
        row = value['heldOut'][0]['coverage'][0]
        for key in list(row):
            row[key] = visits() if key.endswith('_by_street') else 0
        rows = report.diagnostic(value)
        self.assertIsNone(rows[0]['preflopTrainedVisitFraction'])
        self.assertEqual(len(rows), 12)

    def test_rejects_postflop_training_even_when_all_partitions_balance(self):
        value = fixture()
        row = value['heldOut'][0]['coverage'][0]
        row['trained_action_visits_by_street']['preflop'] -= 1
        row['trained_action_visits_by_street']['flop'] += 1
        row['baseline_fallback_visits_by_street']['preflop'] += 1
        row['baseline_fallback_visits_by_street']['flop'] -= 1
        with self.assertRaisesRegex(ValueError, 'postflop continuation changed'):
            report.diagnostic(value)

    def test_rejects_coverage_partition_and_source_corruption(self):
        for kind in ('deviator', 'stored', 'street', 'current'):
            with self.subTest(kind=kind):
                value = fixture()
                held = value['heldOut'][0]
                if kind == 'deviator':
                    row = held['coverage'][0]
                    row['trained_action_visits'] += 1
                    row['trained_action_visits_by_street']['preflop'] += 1
                else:
                    row = held['candidatePolicyCoverage'][0]
                    if kind == 'stored':
                        row['average_strategy_visits'] += 1
                        row['average_strategy_visits_by_street']['preflop'] += 1
                    elif kind == 'street':
                        del row['decision_visits_by_street']['river']
                    else:
                        row['current_strategy_visits'] += 1
                        row['current_strategy_visits_by_street']['preflop'] += 1
                        row['average_strategy_visits'] -= 1
                        row['average_strategy_visits_by_street']['preflop'] -= 1
                with self.assertRaises(ValueError):
                    report.diagnostic(value)

    def test_fit_cutoff_rejects_insufficient_retained_or_excess_unretained_visits(self):
        for total, retained in ((19, 15), (28, 20)):
            value = fixture()
            value['fitCoverage'][0].update(total_visits=total, retained_visits=retained)
            with self.subTest(total=total, retained=retained), self.assertRaises(ValueError):
                report.diagnostic(value)

    def test_fit_cannot_attribute_visits_to_no_retained_keys(self):
        for visited, total, retained_visits in ((2, 3, 1), (0, 1, 1)):
            value = fixture()
            value['fitCoverage'][0].update(visited_infosets=visited, retained_infosets=0,
                                           total_visits=total, retained_visits=retained_visits)
            with self.subTest(visited=visited), self.assertRaises(ValueError):
                report.diagnostic(value)

    def test_rejects_missing_seats_or_seeds_instead_of_selecting_a_subset(self):
        for field in ('fitCoverage', 'baseline', 'deviating', 'gains', 'coverage',
                      'candidatePolicyCoverage', 'heldOut'):
            value = fixture()
            rows = value[field] if field in ('fitCoverage', 'heldOut') else value['heldOut'][0][field]
            rows.pop()
            with self.subTest(field=field), self.assertRaises(ValueError):
                report.diagnostic(value)

    def test_rejects_budget_seed_order_cutoff_and_variant_changes(self):
        mutations = [
            lambda v: v['config'].update(fitTraversalsPerSeat=8191),
            lambda v: v['config'].update(fitSeed=2701),
            lambda v: v['fitCoverage'][0].update(traversals=8191),
            lambda v: v['heldOut'][0].update(samples=32767),
            lambda v: v['heldOut'][1].update(seed=2701),
            lambda v: v['heldOut'].reverse(),
            lambda v: v.update(minFitVisits=7),
            lambda v: v.update(maxBufferedSamples=4097),
            lambda v: v['variant'].update(use_current_strategy=True),
        ]
        for index, mutate in enumerate(mutations):
            value = fixture()
            mutate(value)
            with self.subTest(index=index), self.assertRaises(ValueError):
                report.diagnostic(value)

    def test_rejects_clipped_loss_inconsistent_intervals_and_nonfinite_values(self):
        mutations = [
            lambda h: h['gains'].__setitem__(0, estimate(0.0)),
            lambda h: h['gains'][0].update(ci95=[0.0, 0.0]),
            lambda h: h['gains'][0].update(stderr=-0.1),
            lambda h: h['gains'][0].update(mean=float('nan')),
            lambda h: h.update(totalDealAttempts=32767),
            lambda h: h['coverage'][0].update(decision_visits=True),
        ]
        for index, mutate in enumerate(mutations):
            value = fixture()
            mutate(value['heldOut'][0])
            with self.subTest(index=index), self.assertRaises(ValueError):
                report.diagnostic(value)

    def test_old_normalization_removes_only_named_clocks_and_additive_diagnostic(self):
        old = old_fixture()
        before = copy.deepcopy(old)
        new = copy.deepcopy(old)
        new['constructionElapsedSecs'] = 9.0
        new['freshTraining']['solveElapsedSecs'] = 8.0
        new['deviatorTraining']['elapsedSecs'] = 7.0
        new['evaluations'][0]['elapsedSecs'] = 6.0
        new['preflopSupportCensus']['elapsedSecs'] = 5.0
        new['preflopDeviation'] = fixture()
        new['interpretation'] = report.PREFLOP_INTERPRETATION
        self.assertEqual(report.normalized_old(new), report.normalized_old(old))
        self.assertEqual(old, before)
        for change in ('policy', 'signed-zero', 'nested-clock', 'census-state', 'counter'):
            changed = copy.deepcopy(new)
            if change == 'policy':
                changed['policySupport'][0]['strategySum'][0] = 2.0
            elif change == 'signed-zero':
                changed['policySupport'][0]['regrets'][0] = 0.0
            elif change == 'nested-clock':
                changed['evaluations'][0]['result']['elapsedSecs'] += 1.0
            elif change == 'census-state':
                changed['preflopSupportCensus']['result']['rawStateFingerprint'] = 'c' * 64
            else:
                changed['freshTraining']['metrics']['sweeps'] += 1
            with self.subTest(change=change):
                self.assertNotEqual(report.normalized_old(changed), report.normalized_old(old))

    def test_arm_comparison_preserves_loss_and_rejects_changed_raw_policy(self):
        old = old_fixture()
        new = copy.deepcopy(old)
        new['preflopDeviation'] = fixture()
        new['interpretation'] = report.PREFLOP_INTERPRETATION
        # The large old-schema validator is tested separately. Mock only that
        # boundary, keeping this helper's normalization and diagnostic checks.
        with patch.object(report.previous, 'raw_checks') as old_check:
            result = report.compare_arm(new, old, candidate=True)
            self.assertTrue(result['oldOutputExactlyReplayed'])
            self.assertEqual(len(result['rows']), 12)
            self.assertEqual(result['rows'][0]['gain']['mean'], -1.5)
            self.assertEqual(old_check.call_count, 2)
            for call in old_check.call_args_list:
                self.assertEqual(call.kwargs, {'candidate': True})
            new['policySupport'][0]['regrets'][1] = 2.0
            with self.assertRaisesRegex(ValueError, 'old diagnostics changed'):
                report.compare_arm(new, old, candidate=True)

    def test_normalization_only_allows_the_exact_predeclared_interpretation_change(self):
        old = old_fixture()
        new = copy.deepcopy(old)
        new['preflopDeviation'] = fixture()
        new['interpretation'] = report.PREFLOP_INTERPRETATION
        self.assertEqual(report.normalized_old(new), report.normalized_old(old))
        for source, text in [
            (new, report.LEGACY_INTERPRETATION),
            (old, report.PREFLOP_INTERPRETATION),
            (new, report.PREFLOP_INTERPRETATION + ' Undeclared extra claim.'),
            (old, report.LEGACY_INTERPRETATION.replace('not a full best response', 'a full best response')),
        ]:
            changed = copy.deepcopy(source)
            changed['interpretation'] = text
            with self.subTest(diagnostic='preflopDeviation' in changed), self.assertRaisesRegex(ValueError, 'interpretation'):
                report.normalized_old(changed)


if __name__ == '__main__':
    unittest.main()
