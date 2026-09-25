import copy
import json
from pathlib import Path
import struct
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import summarize_raised_opponent as report


def fixture():
    # Include two nonsupport nodes with the same stratum. The six reported
    # support nodes are only a subset of this complete synthetic public tree.
    paths = [tuple([0] * n) for n in range(5)]
    paths += [(0, 0, 0, 0, 1), (0, 0, 0, 0, 1, 2),
              (0, 0, 0, 0, 1, 2, 2), (0, 0, 0, 0, 1, 2, 2, 2),
              (0, 1), (1,), (1, 0)]
    menus = [
        ['fold', 'raise-to:2000'], ['fold', 'raise-to:2000'], ['fold', 'raise-to:2000'],
        ['fold', 'raise-to:2000'], ['fold', 'raise-to:3000', 'raise-to:100000:all-in'],
        ['fold', 'call:2000', 'raise-to:10000', 'raise-to:100000:all-in'],
        ['fold', 'call:7000', 'raise-to:21000', 'raise-to:100000:all-in'],
        ['fold', 'call:11000', 'raise-to:100000:all-in'], ['fold', 'call:79000:all-in'],
        ['fold', 'call:1000'], ['fold', 'call:1000'], ['fold', 'call:1000'],
    ]
    actors = [3, 4, 5, 0, 1, 2, 1, 2, 1, 5, 4, 5]
    opponents = [5, 4, 3, 2, 1, 1, 1, 1, 1, 4, 5, 4]
    aggressive = [0, 0, 0, 0, 0, 1, 2, 3, 4, 1, 1, 1]
    rows, by_path, path_labels = [], {}, {}
    for index, path in enumerate(paths):
        key = list(index.to_bytes(16, 'little'))
        parent = by_path[path[:-1]] if path else None
        labels = path_labels[path[:-1]] + [parent['actionLabels'][path[-1]]] if path else []
        row = dict(nodeId=index, history=key, parentHistory=parent['history'] if parent else None,
                   parentActionIndex=path[-1] if path else None, actor=actors[index],
                   activeOpponents=opponents[index], bucketActiveOpponents=opponents[index],
                   aggressiveActions=aggressive[index], preflopLimpers=0, preflopFlats=0,
                   actionLabels=menus[index], rawStateFingerprint=f'{index:064x}',
                   **{name: (169 if name == 'expectedBuckets' else 0) for name in report.COUNTS})
        rows.append(row)
        by_path[path], path_labels[path] = row, labels
    census = dict(schemaVersion='solvers.multiway-preflop-support-census/v1',
                  totalMaterializedNodes=24, totalMaterializedColumns=24 * 169,
                  preflopNodes=len(rows), rawStateFingerprint='f' * 64, nodes=rows)
    for key in report.COUNTS:
        census['total' + key[0].upper() + key[1:]] = sum(row[key] for row in rows)
    support = []
    for index in (0, 4, 5, 6, 7, 8):
        node, path = rows[index], paths[index]
        context = dict(requested='/'.join(path_labels[path]) if path else 'root',
                       history=bytes(node['history']).hex(), actionIndices=list(path),
                       actionLabels=path_labels[path], actor=node['actor'], street='preflop',
                       activeOpponents=node['activeOpponents'])
        item = dict(context=context, actionLabels=node['actionLabels'], bucketActiveOpponents=node['bucketActiveOpponents'],
                    rows=[dict(bucket=bucket, status='missing', regrets=None, strategySum=None, strategyMass=None,
                               currentStrategy=None, averageStrategy=None) for bucket in range(169)])
        for key in report.COUNTS:
            item['averageBuckets' if key == 'positiveAverageBuckets' else key] = node[key]
        support.append(item)
    ordinary = dict(schemaVersion='solvers.multiway-checkpoint-audit/v1', solverStateVersion=4,
                    config='frozen.toml', sweeps=8192, configurationFingerprint='a' * 64,
                    abstractionFingerprint='b' * 64,
                    policyArena=dict(nodes=24, columns=24 * 169, slots=24 * 169 * 2, bytes=1024,
                                     pages_committed=True), constructionElapsedSecs=1.0,
                    evaluationSamplesPerSeed=128, evaluationSeeds=[101, 202],
                    deviatorTraining=dict(seed=42, traversalsPerSeat=1, elapsedSecs=0.1,
                        coverage=[dict(traversals=1, visited_infosets=1, retained_infosets=0,
                                       total_visits=1, retained_visits=0) for _ in range(6)]),
                    evaluations=[dict(seed=seed, elapsedSecs=0.2, result=dict(samples=128, opaqueValue=-1.25))
                                 for seed in [101, 202]],
                    nodes=[dict(requested='root', frequency=None, hands=[])],
                    freshTraining=dict(requestedSweeps=8192, solveElapsedSecs=10.0,
                        effectiveConfigBlake3='c' * 64,
                        solverConfig=dict(seed=0, sweep_batch=4, traverser_vector=True,
                                          exploration_epsilon=0.0, prune=False, max_memory_bytes=8 * 1024**3,
                                          discount_until=0),
                        metrics=dict(sweeps=8192, traversals=49152, infosets=0, memory_bytes=1024,
                                     total_deal_attempts=49152, mean_deal_attempts=1.0, hand_updates=100,
                                     average_positive_regret=[0.0] * 6)),
                    preflopSupportCensus=dict(elapsedSecs=0.5, result=census), policySupport=support,
                    interpretation='fixed fixture')
    enumerated = copy.deepcopy(ordinary)
    enumerated['freshTraining']['solveElapsedSecs'] = 15.0
    enumerated['preflopSupportCensus']['elapsedSecs'] = 0.6
    enumerated['freshTraining']['regretSamplingResearch'] = dict(
        variant='enumerate-first-raised-preflop', completedSweeps=8192, publicPreflopNodes=len(rows),
        eligiblePublicNodes=sum(row['aggressiveActions'] >= 1 and len(row['actionLabels']) > 1 for row in rows),
        eligibilityBytes=24, maxExpansionsPerPath=1)
    legacy = copy.deepcopy(ordinary)
    del legacy['preflopSupportCensus']
    legacy['constructionElapsedSecs'] = 2.0
    legacy['freshTraining']['solveElapsedSecs'] = 20.0
    legacy['deviatorTraining']['elapsedSecs'] = 0.3
    for evaluation in legacy['evaluations']:
        evaluation['elapsedSecs'] = 0.4
    return ordinary, enumerated, legacy


def add_numeric_support(raw, node_index=9, *, average=False):
    census = raw['preflopSupportCensus']['result']
    row = census['nodes'][node_index]
    changed = ['storedBuckets', 'nonzeroRegretBuckets', 'positiveRegretBuckets']
    if average:
        changed += ['positiveAverageBuckets', 'averageAndNonzeroRegretBuckets']
    for key in changed:
        row[key] += 1
        census['total' + key[0].upper() + key[1:]] += 1
    row['rawStateFingerprint'] = 'd' * 64
    census['rawStateFingerprint'] = 'e' * 64
    raw['freshTraining']['metrics']['infosets'] += 1
    for support in raw['policySupport']:
        if support['context']['history'] != bytes(row['history']).hex():
            continue
        for key in changed:
            support['averageBuckets' if key == 'positiveAverageBuckets' else key] += 1
        width = len(row['actionLabels'])
        avg = [0.1 if average else 0.0] + [0.0] * (width - 1)
        mass = struct.unpack('<f', struct.pack('<f', avg[0]))[0]
        support['rows'][0] = dict(bucket=0, status='stored-positive-regrets',
                                 regrets=[1.0] + [0.0] * (width - 1), strategySum=avg,
                                 strategyMass=mass, currentStrategy=[1.0] + [0.0] * (width - 1),
                                 averageStrategy=([1.0] + [0.0] * (width - 1)) if average else None)


class RaisedOpponentSummaryTests(unittest.TestCase):
    def test_complete_nodes_strata_and_descriptive_costs(self):
        raw = fixture()
        before = copy.deepcopy(raw)
        result = report.compare(*raw)
        self.assertEqual(raw, before)
        self.assertEqual(result['publicNodeCount'], 12)
        self.assertEqual(sum(row['nodeCount'] for row in result['strata']), 12)
        group = next(row for row in result['strata'] if row['nodeCount'] == 2)
        self.assertEqual(group['ordinary']['expectedBuckets'], 338)
        self.assertEqual(result['enumeratedToOrdinaryTimeRatios'], dict(solveElapsedSecs=1.5, censusElapsedSecs=1.2))
        self.assertTrue(result['ordinaryMatchesLegacyExceptFourClocksAndAdditiveCensus'])
        self.assertFalse(result['qualityImprovementClaim'])
        self.assertFalse(result['promotionAllowed'])

    def test_numeric_change_at_node_outside_six_exports_is_retained(self):
        ordinary, candidate, legacy = fixture()
        add_numeric_support(candidate)
        result = report.compare(ordinary, candidate, legacy)
        self.assertEqual(result['totals']['delta']['nonzeroRegretBuckets'], 1)
        self.assertEqual(result['nodes'][9]['delta']['nonzeroRegretBuckets'], 1)
        group = next(row for row in result['strata'] if row['nodeCount'] == 2)
        self.assertEqual(group['delta']['nonzeroRegretBuckets'], 1)
        self.assertEqual(result['nodes'][9]['path'], 'fold/raise-to:2000')

    def test_missing_or_duplicate_nodes_reject(self):
        for kind in ('missing', 'duplicate', 'order'):
            raw = fixture()
            rows = raw[1]['preflopSupportCensus']['result']['nodes']
            if kind == 'missing':
                rows.pop()
            elif kind == 'duplicate':
                rows[-1]['history'] = rows[-2]['history']
            else:
                rows[-1], rows[-2] = rows[-2], rows[-1]
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                report.compare(*raw)

    def test_invalid_counts_totals_and_intersection_reject(self):
        for kind in ('total', 'boolean', 'negative', 'above_expected', 'impossible_intersection'):
            raw = fixture()
            census = raw[1]['preflopSupportCensus']['result']
            row = census['nodes'][9]
            if kind == 'total':
                census['totalStoredBuckets'] = 1
            elif kind == 'boolean':
                row['storedBuckets'] = True
            elif kind == 'negative':
                row['storedBuckets'] = -1
            elif kind == 'above_expected':
                row['storedBuckets'] = 170
            else:
                row.update(storedBuckets=1, nonzeroRegretBuckets=1, positiveAverageBuckets=1)
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                report.compare(*raw)

    def test_parent_path_and_public_metadata_mismatch_reject(self):
        for kind in ('parent', 'edge', 'actor', 'labels'):
            raw = fixture()
            row = raw[1]['preflopSupportCensus']['result']['nodes'][9]
            if kind == 'parent':
                row['parentHistory'] = [255] * 16
            elif kind == 'edge':
                row['parentActionIndex'] = 99
            elif kind == 'actor':
                row['actor'] = 0
            else:
                row['actionLabels'][0] = 'check'
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                report.compare(*raw)

    def test_candidate_variant_budget_and_scope_reject(self):
        for field, value in [('variant', 'uniform-one'), ('completedSweeps', 4096),
                             ('maxExpansionsPerPath', 2), ('publicPreflopNodes', 1),
                             ('eligiblePublicNodes', 99), ('eligibilityBytes', 1)]:
            raw = fixture()
            raw[1]['freshTraining']['regretSamplingResearch'][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                report.compare(*raw)

    def test_legacy_comparison_removes_only_four_clock_locations_and_census(self):
        for kind in ('metric', 'extra_clock', 'evaluation', 'signed_zero', 'config'):
            raw = fixture()
            if kind == 'metric':
                raw[0]['freshTraining']['metrics']['hand_updates'] += 1
            elif kind == 'extra_clock':
                raw[0]['unknownElapsedSecs'] = 1.0
            elif kind == 'evaluation':
                raw[0]['evaluations'][0]['result']['opaqueValue'] += 1.0
            elif kind == 'signed_zero':
                raw[0]['freshTraining']['metrics']['average_positive_regret'][0] = -0.0
            else:
                raw[0]['config'] = 'another.toml'
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                report.compare(*raw)

    def test_six_support_raw_f32_mass_and_census_counts(self):
        raw = fixture()
        add_numeric_support(raw[1], 0, average=True)
        result = report.compare(*raw)
        self.assertEqual(result['totals']['delta']['positiveAverageBuckets'], 1)
        raw[1]['policySupport'][0]['rows'][0]['strategyMass'] = 0.1
        with self.assertRaisesRegex(ValueError, 'exact raw f32 average mass'):
            report.compare(*raw)

    def test_support_metadata_missing_rows_and_counts_reject(self):
        for kind in ('missing_row', 'count', 'path_labels', 'status', 'zero_average_fallback'):
            raw = fixture()
            support = raw[1]['policySupport'][0]
            if kind == 'missing_row':
                support['rows'].pop()
            elif kind == 'count':
                support['storedBuckets'] = 1
            elif kind == 'path_labels':
                support['context']['actionLabels'] = support['actionLabels']
            elif kind == 'status':
                support['rows'][0]['status'] = 'stored-zero-regrets'
            else:
                support['rows'][0]['averageStrategy'] = [0.5, 0.5]
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                report.compare(*raw)

    def test_nonfinite_cost_or_raw_value_reject_and_zero_census_ratio_is_null(self):
        for bad in (float('nan'), float('inf'), -1.0, True):
            raw = fixture()
            raw[1]['preflopSupportCensus']['elapsedSecs'] = bad
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                report.compare(*raw)
        raw = fixture()
        raw[1]['evaluations'][0]['result']['opaqueValue'] = float('nan')
        with self.assertRaises(ValueError):
            report.compare(*raw)
        raw = fixture()
        raw[0]['preflopSupportCensus']['elapsedSecs'] = 0.0
        self.assertIsNone(report.compare(*raw)['enumeratedToOrdinaryTimeRatios']['censusElapsedSecs'])

    def test_inconsistent_fingerprints_reject(self):
        raw = fixture()
        add_numeric_support(raw[1])
        raw[1]['preflopSupportCensus']['result']['rawStateFingerprint'] = 'f' * 64
        with self.assertRaises(ValueError):
            report.compare(*raw)
        raw[1]['preflopSupportCensus']['result']['rawStateFingerprint'] = 'invalid'
        with self.assertRaises(ValueError):
            report.compare(*raw)

    def test_cli_output_bytes_reproduce(self):
        # Exercise CLI parsing, input hashes and serialized output while the
        # file boundary is in memory; this is independent of host temp ACLs.
        folder = Path('runs/raised-opponent-cli-fixture')
        files, written, argv = {}, [], ['summary']
        for name, value in zip(('ordinary', 'enumerated', 'legacy'), fixture()):
            path = folder / (name + '.json')
            files[str(path)] = report.serialized(value)
            argv += ['--' + name, str(path)]
        output = folder / 'summary.json'
        argv += ['--out', str(output)]
        with patch.object(sys, 'argv', argv), \
                patch.object(Path, 'read_bytes', autospec=True, side_effect=lambda path: files[str(path)]), \
                patch.object(Path, 'write_bytes', autospec=True,
                             side_effect=lambda path, data: written.append((path, data))):
            report.main()
            report.main()
        self.assertEqual(written[0], written[1])
        self.assertEqual(written[0][0], output)
        parsed = json.loads(written[0][1])
        self.assertEqual(set(parsed['inputs']), {'ordinary', 'enumerated', 'legacy'})
        self.assertEqual(len(parsed['nodes']), 12)


if __name__ == '__main__':
    unittest.main()
