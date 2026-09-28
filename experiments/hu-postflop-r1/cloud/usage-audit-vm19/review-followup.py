"""Independent compact arithmetic/provenance check for the separate VM08-15 proposal."""
import datetime as dt
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
ROOT = CLOUD.parents[2]


def pin(path):
    data = path.read_bytes()
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def minutes(start, end):
    elapsed = dt.datetime.fromisoformat(end.replace('Z', '+00:00')) - dt.datetime.fromisoformat(start.replace('Z', '+00:00'))
    value = D(elapsed.days * 86400 + elapsed.seconds) + D(elapsed.microseconds) / 1000000
    assert value > 0
    return int(((value + 120) / 60).to_integral_value(rounding=ROUND_CEILING))


def main():
    directory = CLOUD / 'usage-followup-20260928'
    report = json.loads((directory / 'report.json').read_bytes())
    assert report['source'] == pin(directory / 'validate.py')
    assert report['ledger_observation'] == pin(HERE / 'inputs/budget.json')
    for name, expected in report['input_pins'].items():
        assert pin(ROOT / name) == expected
    sources = json.loads((CLOUD / 'preflight-vm18/pricing-sources.json').read_bytes())
    compute, = [attempt for attempt in sources['attempts'] if attempt['source'] == 'https://cloud.google.com/products/compute/pricing/general-purpose']
    assert compute['status'] == 'acquired'
    prices = {}
    for name in ('e2-standard-2', 'e2-highcpu-32'):
        parts = {}
        for suffix in ('row', 'header', 'region'):
            path = CLOUD / 'preflight-vm18' / ('pricing-compute-' + name + '-' + suffix + '.html')
            reference, = [item for item in compute['retained'] if item['path'] == path.name]
            assert pin(path) == {key: reference[key] for key in ('bytes', 'sha256')}
            parts[suffix] = path.read_text()
        assert name in parts['row'] and 'Default' in parts['header'] and 'Iowa (us-central1)' in parts['region']
        prices[name] = D(re.findall(r'\$([0-9.]+) / 1 hour', parts['row'])[0])
    rows = []
    for row in report['rows']:
        vm = row['vm']
        whole = minutes(row['launch_attempt'], row['absence_verified'])
        assert row['whole_minutes'] == whole
        assert row['disk_minutes'] == (1440 if vm == '08' else whole)
        assert row['network_allowance_gib'] == 1 and row['network_missing_usage_unknown'] is True and row['network_formal_cap_proven'] is False
        rate = D('1.15') if vm == '12' else prices['e2-standard-2'] if vm in ('14', '15') else D('.14')
        parts = {'whole_lifetime_compute': D(whole) / 60 * rate,
                 'overlapping_highcpu32_compute': D(row['overlapping_high_minutes']) / 60 * prices['e2-highcpu-32'],
                 'whole_lifetime_ipv4': D(whole) / 60 * D('.0025'),
                 'disk': D(row['disk_minutes']) / 60 * 40 * D('.000137'),
                 'unchanged_one_gib_network_allowance': D('.30'),
                 'unchanged_original_uncertainty': D(2 if vm == '08' else 1)}
        assert parts == {key: D(value) for key, value in row['costs_usd'].items()}
        assert sum(parts.values()) == D(row['modeled_total_usd']) <= D(row['proposed_hold_usd'])
        assert sum(map(D, row['reservation_allocation_usd'].values())) == D(row['proposed_hold_usd'])
        assert D(row['held_before_usd']) - D(row['proposed_hold_usd']) == D(row['restore_usd'])
        rows.append({'vm': vm, 'restore_usd': row['restore_usd'], 'model_usd': row['modeled_total_usd']})
    restore = sum(D(row['restore_usd']) for row in rows)
    assert restore == D('2.10')
    result = {'schema': 'r1.usage-followup-independent-review/v1', 'status': 'passed_with_disclosed_evidence_limits',
              'reviewer_scope': 'Independent source review, previous full validator replay exit0, and separate arithmetic/official-price provenance checks',
              'review_source': pin(Path(__file__)), 'proposal': pin(directory / 'report.json'),
              'validator': pin(directory / 'validate.py'), 'budget_snapshot': pin(HERE / 'inputs/budget.json'),
              'rechecked_input_pins': len(report['input_pins']), 'rows': rows, 'restore_usd': str(restore),
              'full_validator_replay': {'argv': 'python -B usage-followup-20260928/validate.py --budget usage-audit-vm19/inputs/budget.json --check', 'exit_code': 0},
              'limits': ['VM09 raw named inventories establish absence, but no matching delete-operation record is claimed; README already discloses this.',
                         'VM10/11/13 retain their existing 4-vCPU price envelope; no lower actual-machine rate is introduced.',
                         'All original uncertainty and one-GiB network buffers remain. Missing Monitoring observations are unknown.',
                         'Formal billing export remains absent; cost proposal is not invoice or guaranteed upper bill.',
                         'No ledger/resource change, new API call, native command or archive access in this review.']}
    output = HERE / 'independent-followup-review.json'
    with output.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(result, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'status': result['status'], 'restore_usd': result['restore_usd'], 'output': pin(output)}))


if __name__ == '__main__':
    main()
