"""Check raw raised-opponent cost-screen outputs and retain the whole preflop census.

Source, executable, job, process ordering and measurement provenance are checked
separately by the experiment owner. This tool does not infer strategic quality
from touched columns, nonzero regrets, average mass or raw-state fingerprints.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
from pathlib import Path
import re
import struct


COUNTS = ('expectedBuckets', 'storedBuckets', 'nonzeroRegretBuckets',
          'positiveRegretBuckets', 'positiveAverageBuckets', 'averageAndNonzeroRegretBuckets')
STRATA = ('actor', 'aggressiveActions', 'activeOpponents', 'bucketActiveOpponents',
          'preflopLimpers', 'preflopFlats', 'actionCountClass')
PUBLIC = ('nodeId', 'history', 'parentHistory', 'parentActionIndex', 'actor',
          'activeOpponents', 'bucketActiveOpponents', 'aggressiveActions',
          'preflopLimpers', 'preflopFlats', 'actionLabels', 'expectedBuckets')
FOLD = 'fold/fold/fold/fold'
OPEN = FOLD + '/raise-to:3000'
THREE = OPEN + '/raise-to:10000'
FOUR = THREE + '/raise-to:21000'
FIVE = FOUR + '/raise-to:100000:all-in'
SUPPORTS = ['root', FOLD, OPEN, THREE, FOUR, FIVE]
ROOT = '0' * 32


def require(condition, message):
    if not condition:
        raise ValueError(message)


def integer(value, name, maximum=(1 << 64) - 1):
    require(type(value) is int and 0 <= value <= maximum, 'invalid integer: ' + name)
    return value


def number(value, name, *, positive=False):
    require(type(value) in (int, float) and math.isfinite(value)
            and (value > 0 if positive else value >= 0), 'invalid number: ' + name)
    return value


def finite_json(value):
    if isinstance(value, float):
        require(math.isfinite(value), 'nonfinite raw JSON')
    elif isinstance(value, dict):
        for child in value.values():
            finite_json(child)
    elif isinstance(value, list):
        for child in value:
            finite_json(child)


def fingerprint(value):
    require(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value), 'invalid BLAKE3 fingerprint')
    return value


def history(value):
    require(isinstance(value, list) and len(value) == 16, 'history must be 16 bytes')
    return bytes(integer(byte, 'history byte', 255) for byte in value).hex()


def serialized(value):
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False, allow_nan=False) + '\n').encode()


def normalized(raw, *, remove_census):
    """Preserve every field except the additive census and four clock locations."""
    result = copy.deepcopy(raw)
    if remove_census:
        result.pop('preflopSupportCensus')
    result.pop('constructionElapsedSecs')
    result['freshTraining'].pop('solveElapsedSecs')
    result['deviatorTraining'].pop('elapsedSecs')
    for item in result['evaluations']:
        item.pop('elapsedSecs')
    return result


def counts(row):
    result = {key: integer(row[key], key) for key in COUNTS}
    expected, stored, nonzero, positive, average, both = (result[key] for key in COUNTS)
    require(positive <= nonzero <= stored <= expected and average <= stored
            and max(0, nonzero + average - stored) <= both <= min(nonzero, average),
            'support count inclusion/partition')
    return result


def totals(rows):
    return {key: sum(row[key] for row in rows) for key in COUNTS}


def difference(ordinary, enumerated):
    return {key: enumerated[key] - ordinary[key] for key in COUNTS}


def census_checks(raw):
    wrapper = raw['preflopSupportCensus']
    require(set(wrapper) == {'elapsedSecs', 'result'}, 'census wrapper fields')
    number(wrapper['elapsedSecs'], 'census elapsed seconds')
    census = wrapper['result']
    require(census['schemaVersion'] == 'solvers.multiway-preflop-support-census/v1', 'census schema')
    total_nodes = integer(census['totalMaterializedNodes'], 'materialized nodes')
    total_columns = integer(census['totalMaterializedColumns'], 'materialized columns')
    require(total_nodes == raw['policyArena']['nodes'] and total_columns == raw['policyArena']['columns'],
            'census materialized arena identity')
    rows = census['nodes']
    require(isinstance(rows, list) and len(rows) == integer(census['preflopNodes'], 'preflop nodes')
            and 0 < len(rows) <= total_nodes, 'complete preflop node count')
    fingerprint(census['rawStateFingerprint'])
    known, enriched, parent_actions = {}, [], set()
    previous_id, previous_path = -1, None
    for row in rows:
        require(set(row) == set(PUBLIC) | set(COUNTS) | {'rawStateFingerprint'}, 'census node fields')
        node_id = integer(row['nodeId'], 'node id', total_nodes - 1)
        require(node_id > previous_id, 'increasing node order')
        previous_id = node_id
        key = history(row['history'])
        require(key not in known, 'duplicate public history')
        labels = row['actionLabels']
        require(isinstance(labels, list) and labels and all(isinstance(label, str) and label for label in labels)
                and len(set(labels)) == len(labels), 'legal menu shape')
        for field in ('actor', 'activeOpponents', 'bucketActiveOpponents'):
            integer(row[field], field, 5)
        for field in ('aggressiveActions', 'preflopLimpers', 'preflopFlats'):
            integer(row[field], field, 255)
        require(row['expectedBuckets'] == 169, 'full configured preflop classes')
        counts(row)
        fingerprint(row['rawStateFingerprint'])
        if not enriched:
            require(node_id == 0 and key == ROOT and row['parentHistory'] is None
                    and row['parentActionIndex'] is None, 'root identity')
            path, path_labels = [], []
        else:
            parent_key = history(row['parentHistory'])
            require(parent_key in known, 'missing or later parent')
            parent = known[parent_key]
            action = integer(row['parentActionIndex'], 'parent action', len(parent['actionLabels']) - 1)
            require((parent_key, action) not in parent_actions, 'duplicate parent/action edge')
            parent_actions.add((parent_key, action))
            path = parent['actionIndices'] + [action]
            path_labels = parent['pathLabels'] + [parent['actionLabels'][action]]
        require(previous_path is None or previous_path < tuple(path), 'public-action preorder')
        previous_path = tuple(path)
        item = dict(row, historyHex=key, actionIndices=path, pathLabels=path_labels,
                    path='/'.join(path_labels) if path_labels else 'root',
                    actionCountClass='single' if len(labels) == 1 else 'multi')
        known[key] = item
        enriched.append(item)
    aggregate = totals(rows)
    for key, value in aggregate.items():
        require(integer(census['total' + key[0].upper() + key[1:]], 'total ' + key) == value,
                'census totals must equal every node: ' + key)
    counts(aggregate)
    require(aggregate['expectedBuckets'] <= total_columns
            and aggregate['storedBuckets'] <= raw['freshTraining']['metrics']['infosets'],
            'preflop counts within all-street arena/metrics')
    return enriched, aggregate


def f32(value):
    require(type(value) in (float, int) and math.isfinite(value), 'finite f32 input')
    try:
        result = struct.unpack('<f', struct.pack('<f', value))[0]
    except (OverflowError, struct.error) as error:
        raise ValueError('f32 input overflow') from error
    require(math.isfinite(result), 'finite f32 value')
    return result


def support_checks(raw, census_rows):
    support = raw['policySupport']
    require([node['context']['requested'] for node in support] == SUPPORTS, 'six fixed support nodes')
    by_history = {row['historyHex']: row for row in census_rows}
    for node in support:
        context = node['context']
        require(context['history'] in by_history, 'support history absent from census')
        census = by_history[context['history']]
        require(context['street'] == 'preflop' and context['requested'] == census['path']
                and context['actionIndices'] == census['actionIndices']
                and context['actionLabels'] == census['pathLabels']
                and context['actor'] == census['actor']
                and context['activeOpponents'] == census['activeOpponents']
                and node['bucketActiveOpponents'] == census['bucketActiveOpponents']
                and node['actionLabels'] == census['actionLabels'], 'support public context/menu')
        require(node['expectedBuckets'] == 169 and len(node['rows']) == 169, 'all 169 support rows')
        observed = {key: 0 for key in COUNTS}
        observed['expectedBuckets'] = 169
        for bucket, row in enumerate(node['rows']):
            require(integer(row['bucket'], 'support bucket') == bucket, 'ordered support buckets')
            if row['status'] == 'missing':
                require(all(row[key] is None for key in ('regrets', 'strategySum', 'strategyMass',
                                                         'currentStrategy', 'averageStrategy')), 'missing support payload')
                continue
            width = len(node['actionLabels'])
            require(isinstance(row['regrets'], list) and isinstance(row['strategySum'], list)
                    and len(row['regrets']) == len(row['strategySum']) == width, 'support action widths')
            regrets, average = ([f32(v) for v in row[key]] for key in ('regrets', 'strategySum'))
            require(all(value >= 0 for value in average), 'nonnegative raw average mass')
            # Preserve Rust's sequential f64 accumulation; Python 3.12+ sum
            # uses a different float summation algorithm.
            mass = 0.0
            for value in average:
                mass += value
            require(type(row['strategyMass']) in (int, float) and row['strategyMass'] == mass,
                    'exact raw f32 average mass')
            nonzero, positive, has_average = any(v != 0 for v in regrets), any(v > 0 for v in regrets), mass > 0
            status = ('stored-positive-regrets' if positive else
                      'stored-nonpositive-regrets' if nonzero else 'stored-zero-regrets')
            require(row['status'] == status, 'raw regret status')
            require((row['averageStrategy'] is not None) == has_average, 'average fallback must remain null')
            for probabilities in [row['currentStrategy']] + ([row['averageStrategy']] if has_average else []):
                require(isinstance(probabilities, list) and len(probabilities) == width, 'support probability widths')
                values = [f32(value) for value in probabilities]
                require(all(value >= 0 for value in values) and abs(sum(values) - 1.0) <= 1e-6,
                        'normalized support probabilities')
            for key, present in zip(COUNTS[1:], (True, nonzero, positive, has_average, nonzero and has_average)):
                observed[key] += int(present)
        for key, value in observed.items():
            support_key = 'averageBuckets' if key == 'positiveAverageBuckets' else key
            require(integer(node[support_key], support_key) == value and census[key] == value,
                    'raw support/census count: ' + key)


def raw_checks(raw, *, candidate):
    finite_json(raw)
    require(raw['schemaVersion'] == 'solvers.multiway-checkpoint-audit/v1' and raw['solverStateVersion'] == 4,
            'audit/state schema')
    require(raw['sweeps'] == 8192 and raw['freshTraining']['requestedSweeps'] == 8192, 'fixed fresh sweep budget')
    fresh = raw['freshTraining']
    config, metrics = fresh['solverConfig'], fresh['metrics']
    require(config['seed'] == 0 and config['sweep_batch'] == 4 and config['traverser_vector'] is True
            and config['exploration_epsilon'] == 0 and config['prune'] is False
            and config['max_memory_bytes'] == 8 * 1024**3 and config['discount_until'] == 0,
            'fixed seed/batch/vector/memory and no exploration/pruning/discount')
    require(metrics['sweeps'] == 8192 and metrics['traversals'] == 8192 * 6, 'complete six-seat sweeps')
    for field in ('sweeps', 'traversals', 'infosets', 'memory_bytes', 'total_deal_attempts', 'hand_updates'):
        integer(metrics[field], field)
    require(metrics['total_deal_attempts'] >= metrics['traversals']
            and metrics['mean_deal_attempts'] == metrics['total_deal_attempts'] / metrics['traversals'], 'deal counters')
    require(metrics['memory_bytes'] == raw['policyArena']['bytes'] and raw['policyArena']['pages_committed'] is True,
            'fixed committed policy arena')
    require(len(metrics['average_positive_regret']) == 6, 'six regret metrics')
    for value in metrics['average_positive_regret']:
        number(value, 'nonnegative regret diagnostic')
    require(raw['evaluationSamplesPerSeed'] == 128 and raw['evaluationSeeds'] == [101, 202]
            and [evaluation['seed'] for evaluation in raw['evaluations']] == [101, 202]
            and all(evaluation['result']['samples'] == 128 for evaluation in raw['evaluations']), 'fixed ordinary evaluation schedule')
    require(raw['deviatorTraining']['traversalsPerSeat'] == 1
            and len(raw['deviatorTraining']['coverage']) == 6, 'six incidental candidate traversals')
    for row in raw['deviatorTraining']['coverage']:
        for key in ('traversals', 'visited_infosets', 'retained_infosets', 'total_visits', 'retained_visits'):
            integer(row[key], 'deviator ' + key)
        require(row['traversals'] == 1 and row['retained_infosets'] <= row['visited_infosets'] <= row['total_visits']
                and row['retained_visits'] <= row['total_visits'], 'deviator coverage containment')
    require(len(raw['nodes']) == 1 and raw['nodes'][0]['requested'] == 'root'
            and raw['nodes'][0]['frequency'] is None, 'no additional node-frequency sampling')
    for value in [raw['constructionElapsedSecs'], fresh['solveElapsedSecs'], raw['deviatorTraining']['elapsedSecs']
                  ] + [item['elapsedSecs'] for item in raw['evaluations']]:
        number(value, 'elapsed seconds', positive=True)
    rows, aggregate = census_checks(raw)
    support_checks(raw, rows)
    research = fresh.get('regretSamplingResearch')
    if candidate:
        require(isinstance(research, dict) and set(research) == {
            'variant', 'completedSweeps', 'publicPreflopNodes', 'eligiblePublicNodes',
            'eligibilityBytes', 'maxExpansionsPerPath'}, 'candidate research metadata')
        require(research['variant'] == 'enumerate-first-raised-preflop'
                and research['completedSweeps'] == 8192 and research['maxExpansionsPerPath'] == 1
                and research['publicPreflopNodes'] == len(rows)
                and research['eligiblePublicNodes'] == sum(row['aggressiveActions'] >= 1 and len(row['actionLabels']) > 1 for row in rows)
                and research['eligibilityBytes'] == raw['policyArena']['nodes'], 'candidate enumeration scope')
    else:
        require('regretSamplingResearch' not in fresh, 'ordinary mode must not enumerate')
    return rows, aggregate


def compare(ordinary, enumerated, legacy):
    left_rows, left_totals = raw_checks(ordinary, candidate=False)
    right_rows, right_totals = raw_checks(enumerated, candidate=True)
    finite_json(legacy)
    require('preflopSupportCensus' not in legacy, 'legacy must predate additive census')
    old_normal = serialized(normalized(legacy, remove_census=False))
    require(serialized(normalized(ordinary, remove_census=True)) == old_normal,
            'ordinary differs from retained legacy beyond four clocks and additive census')
    require(set(ordinary) == set(enumerated), 'same requested output sections')
    for key in ('schemaVersion', 'solverStateVersion', 'config', 'sweeps', 'configurationFingerprint',
                'abstractionFingerprint', 'policyArena', 'evaluationSamplesPerSeed', 'evaluationSeeds'):
        require(ordinary[key] == enumerated[key], 'common run identity: ' + key)
    for key in ('requestedSweeps', 'effectiveConfigBlake3', 'solverConfig'):
        require(ordinary['freshTraining'][key] == enumerated['freshTraining'][key], 'common training identity: ' + key)
    require(len(left_rows) == len(right_rows), 'same complete public census')
    groups, nodes = {}, []
    for left, right in zip(left_rows, right_rows):
        require(all(left[key] == right[key] for key in PUBLIC), 'same public metadata/node order')
        left_counts, right_counts = counts(left), counts(right)
        if left_counts != right_counts:
            require(left['rawStateFingerprint'] != right['rawStateFingerprint'], 'changed support requires changed node fingerprint')
        key = tuple(left[field] for field in STRATA)
        group = groups.setdefault(key, {'ordinary': [], 'enumerated': []})
        group['ordinary'].append(left_counts)
        group['enumerated'].append(right_counts)
        nodes.append(dict(public={key: left[key] for key in PUBLIC}, historyHex=left['historyHex'],
                          path=left['path'], actionIndices=left['actionIndices'], pathLabels=left['pathLabels'],
                          ordinary=left_counts, enumerated=right_counts, delta=difference(left_counts, right_counts),
                          ordinaryRawStateFingerprint=left['rawStateFingerprint'],
                          enumeratedRawStateFingerprint=right['rawStateFingerprint']))
    left_hash = ordinary['preflopSupportCensus']['result']['rawStateFingerprint']
    right_hash = enumerated['preflopSupportCensus']['result']['rawStateFingerprint']
    require((left_hash == right_hash) == all(left['rawStateFingerprint'] == right['rawStateFingerprint']
            for left, right in zip(left_rows, right_rows)), 'full/node fingerprint consistency')
    strata = []
    for key, group in sorted(groups.items()):
        left, right = totals(group['ordinary']), totals(group['enumerated'])
        strata.append(dict(stratum=dict(zip(STRATA, key)), nodeCount=len(group['ordinary']),
                           ordinary=left, enumerated=right, delta=difference(left, right)))
    timing = {name: {'solveElapsedSecs': raw['freshTraining']['solveElapsedSecs'],
                     'censusElapsedSecs': raw['preflopSupportCensus']['elapsedSecs']}
              for name, raw in [('ordinary', ordinary), ('enumerated', enumerated)]}
    ratios = {field: timing['enumerated'][field] / timing['ordinary'][field]
              if timing['ordinary'][field] else None for field in timing['ordinary']}
    return dict(schemaVersion='solvers.raised-opponent-raw-summary/v1',
                ordinaryMatchesLegacyExceptFourClocksAndAdditiveCensus=True,
                normalizedLegacyOutputSha256=hashlib.sha256(old_normal).hexdigest(),
                publicNodeCount=len(nodes), strataCount=len(strata),
                totals=dict(ordinary=left_totals, enumerated=right_totals, delta=difference(left_totals, right_totals)),
                ordinaryRawStateFingerprint=left_hash, enumeratedRawStateFingerprint=right_hash,
                timings=timing, enumeratedToOrdinaryTimeRatios=ratios,
                strata=strata, nodes=nodes, independentTrainingSeeds=1,
                qualityImprovementClaim=False, promotionAllowed=False, broaderGoalComplete=False,
                interpretation='All materialized preflop nodes are retained, including untouched and zero-numeric-support buckets. '
                'Support counts and fingerprints describe storage and raw numeric state, not visits, ESS or strategy quality. '
                'Each arm uses one fixed training seed and equal sweeps; elapsed ratios are descriptive, not timing confidence intervals. '
                'Raw file/schema/count checks are performed here; source, binary, config bytes, jobs, process order and external measurement '
                'provenance are validated separately. BLAKE3 strings are format-checked and cross-checked for consistency; this compact '
                'output cannot independently reconstruct every raw column or cryptographically verify every history. '
                'Six full support exports are checked against census counts and parent-reconstructed paths.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('ordinary', 'enumerated', 'legacy', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    paths = {name: getattr(args, name) for name in ('ordinary', 'enumerated', 'legacy')}
    blobs = {name: path.read_bytes() for name, path in paths.items()}
    result = compare(*(json.loads(blobs[name].decode('utf-8-sig')) for name in paths))
    result['inputs'] = {name: dict(path=str(paths[name].resolve()), sha256=hashlib.sha256(data).hexdigest())
                        for name, data in blobs.items()}
    args.out.write_bytes(serialized(result))


if __name__ == '__main__':
    main()
