"""Validate the predeclared whole-preflop pilot without selecting favorable seats."""
import argparse
import copy
import hashlib
import json
import math
from pathlib import Path

import summarize_raised_opponent as previous

CONFIG = dict(fitTraversalsPerSeat=8192, fitSeed=2601,
              heldOutSamples=32768, heldOutSeeds=[2701, 2702])
STREETS = ('preflop', 'flop', 'turn', 'river')
POSITIONS = ('BTN', 'SB', 'BB', 'UTG', 'HJ', 'CO')
LEGACY_INTERPRETATION = "Held-out gains cover two fixed candidate deviations per seat; the trained candidate is used where it retained an action and otherwise falls back to the main regret-greedy candidate. Deviator coverage in this document is training coverage, not held-out replay coverage. Node conditional action rates are self-normalized reach-weighted ratio estimates, are not claimed finite-sample unbiased, and represent a composite policy whenever their fallback reach-weight fraction is positive. These results are diagnostics, not a full best response, exploitability, or Nash certificate. Evaluation seeds are reported separately and are not selected or pooled."
PREFLOP_INTERPRETATION = "The ordinary evaluations array covers two fixed candidate deviations per seat with regret-greedy fallback, and deviatorTraining reports fitting coverage. The separate preflopDeviation object fits all own preflop decisions with frozen postflop continuation and baseline fallback; it reports signed paired gains and held-out replay coverage for every seat and seed. Its pointwise intervals are not simultaneous guarantees. Node conditional action rates are self-normalized reach-weighted ratio estimates and can describe a composite fallback policy. These diagnostics are not a full best response, exploitability, or Nash certificate. Evaluation seeds are reported separately and are not selected or pooled."
require = previous.require


def estimate(row):
    require(set(row) == {'mean', 'stderr', 'ci95'}, 'estimate fields')
    require(type(row['mean']) in (int, float) and math.isfinite(row['mean']), 'signed finite mean')
    previous.number(row['stderr'], 'standard error')
    require(len(row['ci95']) == 2, 'interval length')
    for actual, sign in zip(row['ci95'], (-1, 1)):
        require(type(actual) in (int, float) and math.isfinite(actual), 'finite interval')
        expected = row['mean'] + sign * 1.96 * row['stderr']
        require(math.isclose(actual, expected, rel_tol=1e-12, abs_tol=1e-12), 'pointwise paired interval')


def coverage(row, *, candidate):
    fields = ('decision_visits', 'stored_strategy_visits', 'uniform_fallback_visits',
              'average_strategy_visits', 'current_strategy_visits', 'regret_fallback_visits') if candidate else (
                  'decision_visits', 'trained_action_visits', 'baseline_fallback_visits')
    require(set(row) == set(fields) | {field + '_by_street' for field in fields}, 'coverage fields')
    for field in fields:
        previous.integer(row[field], field)
        per_street = row[field + '_by_street']
        require(set(per_street) == set(STREETS), 'all coverage streets')
        require(sum(previous.integer(per_street[street], field) for street in STREETS) == row[field], 'street sum')
    for street in (None,) + STREETS:
        get = lambda field: row[field] if street is None else row[field + '_by_street'][street]
        if candidate:
            require(get('decision_visits') == get('stored_strategy_visits') + get('uniform_fallback_visits'), 'baseline source partition')
            require(get('stored_strategy_visits') == sum(get(field) for field in (
                'average_strategy_visits', 'current_strategy_visits', 'regret_fallback_visits')), 'stored source partition')
            require(get('current_strategy_visits') == 0, 'pilot uses average baseline')
        else:
            require(get('decision_visits') == get('trained_action_visits') + get('baseline_fallback_visits'), 'deviator source partition')
            if street in STREETS[1:]:
                require(get('trained_action_visits') == 0, 'postflop continuation changed')


def diagnostic(value):
    previous.finite_json(value)
    require(set(value) == {'schemaVersion', 'scope', 'config', 'variant', 'minFitVisits',
            'fitElapsedSecs', 'fitCoverage', 'fitPolicyFingerprint', 'maxBufferedSamples', 'heldOut'}, 'diagnostic fields')
    require(value['schemaVersion'] == 'solvers.multiway-preflop-deviation/v1'
            and value['scope'] == 'all-preflop-decisions-with-frozen-postflop', 'diagnostic scope')
    require(value['config'] == CONFIG, 'predeclared fit/held-out budgets')
    require(value['variant'] == {'purify_threshold': 0.0, 'use_current_strategy': False}, 'plain average variant')
    require(value['minFitVisits'] == 8 and value['maxBufferedSamples'] == 4096, 'fit cutoff and bounded replay')
    previous.number(value['fitElapsedSecs'], 'fit elapsed', positive=True)
    previous.fingerprint(value['fitPolicyFingerprint'])
    require(len(value['fitCoverage']) == 6, 'all fit seats')
    for row in value['fitCoverage']:
        require(set(row) == {'traversals', 'visited_infosets', 'retained_infosets', 'total_visits', 'retained_visits'}, 'fit fields')
        for key, count in row.items():
            previous.integer(count, key)
        require(row['traversals'] == CONFIG['fitTraversalsPerSeat'], 'full per-seat fit budget')
        require(row['retained_infosets'] <= row['visited_infosets'] <= row['total_visits'], 'fit key inclusion')
        require(8 * row['retained_infosets'] <= row['retained_visits'] <= row['total_visits'], 'retained visits')
        require(row['retained_infosets'] != 0 or row['retained_visits'] == 0, 'no retained visits without keys')
        unretained = row['visited_infosets'] - row['retained_infosets']
        require(unretained <= row['total_visits'] - row['retained_visits'] <= 7 * unretained, 'unretained visit cutoff')
    require([held['seed'] for held in value['heldOut']] == CONFIG['heldOutSeeds'], 'all held-out seeds in order')
    rows = []
    for held in value['heldOut']:
        require(set(held) == {'seed', 'samples', 'elapsedSecs', 'totalDealAttempts', 'baseline',
                'deviating', 'gains', 'coverage', 'candidatePolicyCoverage'}, 'held-out fields')
        require(held['samples'] == CONFIG['heldOutSamples'], 'complete held-out budget')
        require(previous.integer(held['totalDealAttempts'], 'deal attempts') >= held['samples'], 'physical deal attempts')
        previous.number(held['elapsedSecs'], 'held-out elapsed', positive=True)
        for field in ('baseline', 'deviating', 'gains', 'coverage', 'candidatePolicyCoverage'):
            require(len(held[field]) == 6, 'all held-out seats: ' + field)
        for seat, position in enumerate(POSITIONS):
            baseline, deviating, gain = (held[field][seat] for field in ('baseline', 'deviating', 'gains'))
            for result in (baseline, deviating, gain):
                estimate(result)
            require(math.isclose(gain['mean'], deviating['mean'] - baseline['mean'], rel_tol=1e-10, abs_tol=1e-10), 'paired mean identity')
            cov = held['coverage'][seat]
            coverage(cov, candidate=False)
            coverage(held['candidatePolicyCoverage'][seat], candidate=True)
            decisions = cov['decision_visits_by_street']['preflop']
            trained = cov['trained_action_visits_by_street']['preflop']
            rows.append(dict(seed=held['seed'], seat=seat, position=position,
                             baseline=copy.deepcopy(baseline), deviating=copy.deepcopy(deviating), gain=copy.deepcopy(gain),
                             preflopDecisionVisits=decisions, preflopTrainedActionVisits=trained,
                             preflopTrainedVisitFraction=trained / decisions if decisions else None))
    return rows


def normalized_old(raw):
    result = previous.normalized(raw, remove_census=False)
    if 'preflopDeviation' in result:
        require(result['interpretation'] == PREFLOP_INTERPRETATION, 'explicit diagnostic interpretation')
        result['interpretation'] = LEGACY_INTERPRETATION
        result.pop('preflopDeviation')
    else:
        require(result['interpretation'] == LEGACY_INTERPRETATION, 'retained legacy interpretation')
    result['preflopSupportCensus'].pop('elapsedSecs')
    return previous.serialized(result)


def compare_arm(raw, old, *, candidate):
    previous.raw_checks(raw, candidate=candidate)
    previous.raw_checks(old, candidate=candidate)
    require('preflopDeviation' not in old, 'reference predates whole-preflop diagnostic')
    require(normalized_old(raw) == normalized_old(old), 'trained state or old diagnostics changed')
    rows = diagnostic(raw['preflopDeviation'])
    return dict(oldOutputExactlyReplayed=True,
                normalizedOldOutputSha256=hashlib.sha256(normalized_old(old)).hexdigest(),
                constructionElapsedSecs=raw['constructionElapsedSecs'],
                solveElapsedSecs=raw['freshTraining']['solveElapsedSecs'],
                diagnostic=copy.deepcopy(raw['preflopDeviation']), rows=rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for field in ('ordinary', 'enumerated', 'old-ordinary', 'old-enumerated', 'out'):
        parser.add_argument('--' + field, type=Path, required=True)
    args = parser.parse_args()
    read = lambda path: json.loads(path.read_text(encoding='utf-8-sig'))
    result = dict(schemaVersion='solvers.whole-preflop-deviation-pilot-summary/v1',
                  trainingSeeds=[0], sweeps=8192, configuration=CONFIG,
                  arms={name: compare_arm(read(getattr(args, name)), read(getattr(args, 'old_' + name)),
                                         candidate=name == 'enumerated') for name in ('ordinary', 'enumerated')},
                  interpretation='Signed fixed-candidate root gains, frozen postflop. Pointwise intervals; no full BR, simultaneous guarantee or production promotion. One training seed and equal sweeps, not equal compute; rare branches need separate endpoint evaluation.')
    args.out.write_bytes(previous.serialized(result))
    print(json.dumps(dict(status='passed', arms=list(result['arms']), rowsPerArm=12)))


if __name__ == '__main__':
    main()
