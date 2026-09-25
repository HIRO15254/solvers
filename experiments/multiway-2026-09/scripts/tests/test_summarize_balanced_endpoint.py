"""Self-contained corruption regressions; no solver runs or retained run required."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import summarize_balanced_endpoint as report
from summarize_opponent_exploration import f32, normalized_f32


def estimate(mean, stderr=0.0):
    return dict(mean=mean, stderr=stderr)


def sampling(n, positive, seed, replays):
    def street(visited):
        return dict(positive_weight_decision_visits=positive if visited else 0,
                    positive_weight_trajectory_visits=positive if visited else 0,
                    trajectory_probability=estimate(float(visited)),
                    decisions_per_prefix_trajectory=estimate(float(visited)),
                    decision_weight_effective_sample_size=float(positive) if visited else 0.0,
                    **{key + "_fraction": estimate(float(key == "average")) if visited else None
                       for key in ("average", "current", "regret_fallback", "uniform_fallback")})
    return dict(seed=seed, samples=n, total_deal_attempts=n, terminal_replays=replays,
                positive_weight_samples=positive,
                relative_weight_mean=estimate(positive / n), effective_sample_size=float(positive),
                max_normalized_weight=1 / positive,
                prefix_current_fraction=estimate(0.0), prefix_regret_fallback_fraction=estimate(0.0),
                prefix_uniform_fallback_fraction=estimate(0.0), baseline_seats=[estimate(0.0)] * 6,
                coverage_by_street=[street(s == 3) for s in range(4)],
                coverage_by_seat=[[street(seat == 1 and s == 3) for s in range(4)] for seat in range(6)])


def fixture():
    case = dict(name="hu3-river", endpoint="call:2000/check", history="01" * 16,
                actionIndices=[0, 0], actor=1, activeOpponents=1,
                actionLabels=["check", "bet-to:10000", "bet-to:90000:all-in"],
                preflopActionCount=1, preflopHistory="02" * 16, potBb=20)
    schedule = dict(fitSamples=65536, fitSeed=602, heldOutSamples=131072,
                    heldOutSeeds=[702, 703], rootSamples=262144, rootSeeds=[801, 802], timeoutSeconds=900)
    support_rows, rows = [], []
    for bucket in range(32):
        sums = [f32(0.1), f32(0.2), f32(0.3)]
        mass = 0.0
        for value in sums:
            mass += value
        support_rows.append(dict(bucket=bucket, status="stored-zero-regrets", regrets=[0.0] * 3,
                                 strategySum=sums, strategyMass=mass,
                                 currentStrategy=[f32(1 / 3)] * 3, averageStrategy=normalized_f32(sums)))
        count = 128 if bucket < 2 else 1 if bucket == 2 else 0
        rows.append(dict(key=dict(history=[1] * 16, player=1, street=3, active_opponents=1,
                                  bucket_path=[4294967295] * 3 + [bucket]), baseline_source="average",
                         positive_weight_samples=count, relative_weight_sum=float(count),
                         effective_sample_size=float(count), max_normalized_weight=1 / count if count else 0.0,
                         action_gains=[estimate(v) for v in ([2.0, 1.0, -1.0] if bucket == 0 else [-1.0, -2.0, -3.0])]
                         if count >= 2 else [None] * 3, selected_action=0 if bucket == 0 else None))
    support = dict(context=report.public_context(case), expectedBuckets=32, bucketActiveOpponents=1,
                   actionLabels=case["actionLabels"], storedBuckets=32, nonzeroRegretBuckets=0,
                   positiveRegretBuckets=0, averageBuckets=32, averageAndNonzeroRegretBuckets=0,
                   rows=support_rows)
    result = dict(config=dict(fit_samples=65536, fit_seed=602, held_out_samples=131072,
                              held_out_seeds=[702, 703], min_fit_ess=64.0),
                  variant=dict(purify_threshold=0.0, use_current_strategy=False),
                  history=[1] * 16, action_indices=case["actionIndices"], actor=1, street="river",
                  active_opponents=1, bucket_active_opponents=1, expected_buckets=32,
                  action_labels=case["actionLabels"], fit_elapsed_secs=1.0,
                  fit=dict(sampling=sampling(65536, 257, 602, 1028), retained_buckets=1, rows=rows),
                  held_out=[dict(elapsed_secs=1.0, sampling=sampling(131072, 64, seed, 88),
                                 gain=estimate(1.0, 0.2), retained_key_weight_fraction=estimate(24 / 64, 0.01))
                            for seed in (702, 703)])
    return result, schedule, case, support


class BalancedEndpointValidationTests(unittest.TestCase):
    def setUp(self):
        self.result, self.schedule, self.case, self.support = fixture()

    def check(self):
        return report.endpoint_checks(self.result, self.schedule, self.case, self.support, 64)

    def test_valid_table_preserves_signed_gain_and_distinct_drop_reasons(self):
        self.result["held_out"][0]["gain"]["mean"] = -1.5
        self.check()
        selected = report.fit_selection(self.result, 64)
        self.assertEqual(selected["retainedBuckets"], [0])
        self.assertEqual(selected["noPositiveFitGainBuckets"], [1])
        self.assertEqual(selected["insufficientEssBuckets"], list(range(2, 32)))
        self.assertEqual(selected["singletonBuckets"], [2])
        self.assertEqual(selected["fitWeightFractions"], dict(retained=128 / 257,
                         insufficientEss=1 / 257, noPositiveFitGain=128 / 257))

    def test_rejects_missing_bucket(self):
        self.result["fit"]["rows"].pop()
        with self.assertRaisesRegex(ValueError, "complete fitted rows"):
            self.check()

    def test_rejects_other_players_information_key(self):
        self.result["fit"]["rows"][0]["key"]["player"] = 2
        with self.assertRaisesRegex(ValueError, "own-information"):
            self.check()

    def test_rejects_average_source_for_zero_average_mass(self):
        row = self.support["rows"][0]
        row.update(strategySum=[0.0] * 3, strategyMass=0.0, averageStrategy=None)
        self.support["averageBuckets"] = 31
        with self.assertRaisesRegex(ValueError, "baseline source/raw"):
            self.check()

    def test_rejects_action_not_selected_by_fit(self):
        self.result["fit"]["rows"][0]["selected_action"] = 1
        with self.assertRaisesRegex(ValueError, "fit-only gate"):
            self.check()

    def test_rejects_singleton_standard_error_estimate(self):
        self.result["fit"]["rows"][2]["action_gains"][0] = estimate(1.0)
        with self.assertRaisesRegex(ValueError, "per-key positive worlds"):
            self.check()

    def test_rejects_wrong_all_key_weight_denominator(self):
        self.result["fit"]["sampling"]["relative_weight_mean"]["mean"] *= 2
        with self.assertRaisesRegex(ValueError, "fit weight partition"):
            self.check()

    def test_rejects_gain_without_candidate_replay_even_with_fit_support(self):
        held = self.result["held_out"][0]
        held["sampling"]["terminal_replays"] = held["sampling"]["positive_weight_samples"]
        held["retained_key_weight_fraction"] = estimate(0.0)
        with self.assertRaisesRegex(ValueError, "unsupported held-out"):
            self.check()

    def test_rejects_relabeling_a_held_out_seed(self):
        self.result["held_out"][0]["sampling"]["seed"] = 602
        with self.assertRaisesRegex(ValueError, "sampling schedule"):
            self.check()

    def test_raw_mass_uses_f32_values_not_json_decimal_approximations(self):
        self.support["rows"][0]["strategyMass"] = sum([0.1, 0.2, 0.3])
        with self.assertRaisesRegex(ValueError, "raw strategy mass"):
            self.check()

    def test_rejects_proposal_relative_weight_as_root_reach(self):
        entries = []
        for seed in self.schedule["rootSeeds"]:
            sampled = sampling(262144, 64, seed, 64)
            prefix = {**sampled, "history": [1] * 16, "action_indices": self.case["actionIndices"],
                      "seats": sampled["baseline_seats"], "reach_probability": sampled["relative_weight_mean"]}
            entries.append(dict(seed=seed, prefixes=[report.public_context(self.case)],
                                result=dict(seed=seed, samples=262144, total_deal_attempts=262144,
                                            prefixes=[prefix])))
        data = dict(conditionalEvaluations=entries)
        with self.assertRaisesRegex(ValueError, "absolute root reach labeling"):
            report.root_checks(data, self.schedule, self.case)
        for entry in entries:
            del entry["result"]["prefixes"][0]["relative_weight_mean"]
        self.assertEqual(len(report.root_checks(data, self.schedule, self.case)), 2)


if __name__ == "__main__":
    unittest.main()
