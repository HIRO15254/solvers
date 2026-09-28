"""Explicit five-cent upper rounding; preserve and replay the initial VM20 audit."""
import argparse
from decimal import Decimal as D, ROUND_CEILING
import json
from pathlib import Path

import analyze as initial

HERE = Path(__file__).resolve().parent


def encode(report):
    return (json.dumps(report, indent=2) + '\n').encode()


def build_report():
    report = initial.build_report()
    initial.require(encode(report) == (HERE / 'report.json').read_bytes(),
                    'Original tenth-dollar proposal changed')
    original_cost = dict(report['cost'])
    total = D(original_cost['model_usd'])
    hold = (total / D('.05')).to_integral_value(rounding=ROUND_CEILING) * D('.05')
    restore = D(original_cost['previous_hold_usd']) - hold
    initial.require(restore > 0 and hold >= total, 'No conservative unused reservation')
    report['source'] = initial.pin(Path(__file__))
    report['original_proposal'] = {'source': initial.pin(HERE / 'analyze.py'),
                                   'report': initial.pin(HERE / 'report.json'),
                                   'cost': original_cost}
    report['cost'] = {**original_cost, 'proposed_hold_usd': str(hold),
                      'proposed_restore_usd': str(restore),
                      'rounding_margin_usd': str(hold - total),
                      'reservation_rounding_step_usd': '.05',
                      'reservation_rounding_direction': 'ceiling',
                      'reservation_rounding_basis':
                          'Explicit finer bookkeeping granularity requested and agreed by root after acquisition; '
                          'not an established global policy and not a reduction in modeled expense or buffers.'}
    report['budget_snapshot']['proposed_new_available_usd'] = str(
        D(report['budget_snapshot']['available_usd']) + restore)
    report['rounding_review'] = {
        'original_tenth_dollar_proposal_preserved': True,
        'model_and_components_unchanged': True,
        'original_uncertainty_usd': '1', 'original_network_floor_gib': '.5',
        'extra_seconds_per_lifecycle_window_unchanged': 120,
        'finding': 'VM19 explicitly used a tenth-dollar ceiling; 1.264 to 1.30 is also compatible '
                   'with a five-cent ceiling but does not establish that policy. Earlier audits '
                   'documented optional finer rounding. No fixed universal rounding minimum was found.'}
    return report


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--report', type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    report = build_report()
    data = encode(report)
    if args.check:
        initial.require((HERE / 'report02.json').read_bytes() == data, 'Final proposal differs')
    elif args.report:
        with args.report.open('xb') as stream:
            stream.write(data)
    print(json.dumps({'status': report['status'], 'cost': report['cost'],
                      'proposed_new_available_usd': report['budget_snapshot']['proposed_new_available_usd']}))
