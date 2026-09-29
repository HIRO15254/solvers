"""Apply the reviewed VM02/05 disk-only proposal once; no cloud operations."""
import argparse
import datetime as dt
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import importlib.util
import json
import os
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
BEFORE_SHA = '5f57f5a932540fc8d7c88b25787d1c2e98903c729bd1eda184857415f905a825'


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def encode(value):
    return (json.dumps(value, indent=2, ensure_ascii=False) + '\n').encode()


def fresh(path, raw):
    with path.open('xb') as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def held(ledger):
    return sum(D(str(r['billed_usd'] if r['reservation_released'] else r['reserved_usd']))
               for r in ledger['reservations'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--proposal-sha256', required=True)
    args = parser.parse_args()
    before = (CLOUD / 'budget.json').read_bytes()
    raw_report = (HERE / 'proposal01.json').read_bytes()
    assert pin(before)['sha256'] == BEFORE_SHA
    assert pin(raw_report)['sha256'] == args.proposal_sha256
    report = json.loads(raw_report)
    assert pin((HERE / 'recalculate.py').read_bytes()) == report['source']
    spec = importlib.util.spec_from_file_location('early_disk_recalculation', HERE / 'recalculate.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert module.report() == report
    ledger = json.loads(before)
    assert ledger['authorized_limit'] == 40 and held(ledger) == D('39.95')
    timestamp = dt.datetime.now(dt.timezone.utc).isoformat()
    changes = []
    assert [r['vm'] for r in report['rows']] == ['02', '05']
    for evidence, minutes, new_hold in zip(report['rows'], [44, 61], [D('4.10'), D('4.20')]):
        old = evidence['old_costs_usd_unchanged_except_disk']
        assert D(old['original_other_reserve']) == D('3.2')
        assert D(old['egress_2gib_at_0_30']) == D('.6')
        assert evidence['disk_minutes_after_upper_rounding'] == minutes
        total = D(old['whole_lifetime_plus_full_allowances']) - D('.3288') + D(minutes)/60*100*D('.000137')
        assert abs(total - D(evidence['new_model_usd'])) < D('1e-24')
        assert (total / D('.05')).to_integral_value(rounding=ROUND_CEILING)*D('.05') == new_hold
        row, = [r for r in ledger['reservations'] if r['id'] == evidence['reservation_id']]
        assert row['reserved_usd'] == 4.5 and row['billed_usd'] is None and not row['reservation_released']
        event = {'event': 'usage_based_disk_reservation_reconciliation', 'at': timestamp,
                 'id': row['id'], 'old_reserved_usd': 4.5, 'new_reserved_usd': float(new_hold),
                 'restored_to_available_budget_usd': float(D('4.5') - new_hold),
                 'evidence': 'experiments/hu-postflop-r1/cloud/early-delete-followup-20260928/proposal01.json',
                 'evidence_sha256': args.proposal_sha256, 'billed_usd': None, 'final_invoice': False,
                 'note': 'Disk-only model replacement using preemption DONE with original DELETE/autoDelete policy and present absence; independent disk billing-end timestamp unavailable. Original nondisk costs and uncertainty retained.'}
        row['reserved_usd'] = float(new_hold)
        row.setdefault('events', []).append(event)
        changes.append(event)
    assert held(ledger) == D('39.25')
    receipt = {'schema': 'r1.early-disk-return-applied/v1', 'at_utc': timestamp,
               'authorized_limit_usd': 40, 'held_before_usd': 39.95, 'held_after_usd': 39.25,
               'restored_usd': .70, 'available_after_usd': .75, 'changes': changes,
               'budget_before': pin(before), 'proposal': pin(raw_report),
               'application_source': pin(Path(__file__).read_bytes()), 'billed_usd': None,
               'final_invoice': False, 'cloud_mutations': False, 'local_native_or_archive_work': False}
    ledger.setdefault('usage_reconciliations', []).append(receipt)
    after = encode(ledger)
    receipt = {**receipt, 'budget_after': pin(after)}
    fresh(HERE / 'budget-before.json', before)
    fresh(HERE / 'applied.json', encode(receipt))
    temporary = CLOUD / 'budget-early-disk-return.tmp'
    fresh(temporary, after)
    assert (CLOUD / 'budget.json').read_bytes() == before
    os.replace(temporary, CLOUD / 'budget.json')
    assert (CLOUD / 'budget.json').read_bytes() == after
    print(json.dumps({k: receipt[k] for k in ('restored_usd', 'held_after_usd', 'available_after_usd', 'budget_after')}))


if __name__ == '__main__':
    main()
