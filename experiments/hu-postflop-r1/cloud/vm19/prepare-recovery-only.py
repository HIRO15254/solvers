"""Record one recovery-only window inside the existing reservation; no cloud API."""
import datetime as dt
from decimal import Decimal as D
from fractions import Fraction as F
import hashlib
import json
import math
import os
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent


def need(v, m):
    if not v:
        raise ValueError(m)


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def utc(s):
    return dt.datetime.fromisoformat(s.replace('Z', '+00:00')).astimezone(dt.timezone.utc)


def new(path, raw):
    with path.open('xb') as f:
        f.write(raw)
        f.flush()
        os.fsync(f.fileno())


def main():
    before = (CLOUD / 'budget.json').read_bytes()
    need(pin(before)['sha256'] == '6e29506c473dd8438f5aa5d7748a280c6f06a4bc689d5c5572768431c26b5e7d', 'Ledger changed')
    raw = (HERE / 'resume-state01.stdout.log').read_bytes()
    readback = json.loads((HERE / 'resume-state01.result.json').read_bytes())
    need(readback['exit_code'] == 0 and readback['stdout'] == pin(raw), 'Readback not verified')
    state = json.loads(raw)
    need(state['id'] == '6599180552829758403' and state['status'] == 'TERMINATED'
         and state['machineType'].endswith('/e2-standard-2'), 'Stopped same2CPU VM required')
    launch = json.loads((CLOUD / 'launch-r1-20260927-19.json').read_bytes())
    need(state['scheduling']['terminationTime'] == launch['termination_time'], 'Original deadline changed')
    now = dt.datetime.now(dt.timezone.utc)
    need(0 <= (now - utc(readback['ended_utc'])).total_seconds() < 900, 'Stale readback')
    need(utc(launch['termination_time']) < now, 'Original experiment has not expired')
    disk = json.loads((HERE / 'disk-state01.stdout.log').read_bytes())
    cleanup = utc(disk['creationTimestamp']) + dt.timedelta(hours=24)
    stop = (now + dt.timedelta(seconds=600)).replace(microsecond=0)
    need(stop + dt.timedelta(minutes=5) < cleanup, 'Original disk cleanup window insufficient')
    prior = math.ceil(((utc(state['lastStopTimestamp']) - utc(launch['attempted_at'])).total_seconds() + 120) / 60)
    need(prior == 43, 'Reviewed prior lifetime changed')
    # Charge every prior minute at the highest authorized E2 regular ceiling,
    # without assuming small-machine usage throughout the unobserved interval.
    components = {'prior_any_authorized_e2': F(prior, 60) * F('.80'),
                  'recovery_12min_small_ceiling': F(12, 60) * F('.14'),
                  'ipv4_55min': F(prior + 12, 60) * F('.0025'),
                  'disk_20gib_24h': 20 * 24 * F('.000137'),
                  'egress_512mib': F('.5') * F('.30'), 'original_uncertainty': F(1)}
    total = sum(components.values())
    need(total == F('1.819385') < F('1.85'), 'Recovery exceeds existing reservation')
    ledger = json.loads(before)
    held = sum(D(str(r['billed_usd'] if r['reservation_released'] else r['reserved_usd'])) for r in ledger['reservations'])
    need(ledger['authorized_limit'] == 40 and held == D('39.95'), 'Cumulative budget differs')
    row, = [r for r in ledger['reservations'] if r['id'] == 'r1-20260927-19']
    need(row['reserved_usd'] == 1.85 and not row['reservation_released'] and row['billed_usd'] is None,
         'Reservation changed')
    record = {'schema': 'r1.vm19.recovery-only-amendment/v1', 'prepared_at_utc': now.isoformat(),
              'reason': 'Original experiment window expired during interruption; preserve existing evidence only',
              'instance_id': state['id'], 'machine_type': 'e2-standard-2', 'maximum_recovery_starts': 1,
              'recovery_stop_utc': stop.isoformat(), 'maximum_recovery_runtime_seconds': 600,
              'original_experiment_stop_utc': launch['termination_time'], 'original_experiment_expired': True,
              'build_or_solve_permitted': False, 'resize_permitted': False,
              'disk_cleanup_deadline_utc': cleanup.isoformat(), 'reserved_usd_unchanged': 1.85,
              'held_total_usd_unchanged': 39.95, 'actual_billed_usd': None,
              'cost_components_exact_usd': {k: str(v) for k,v in components.items()},
              'cost_total_exact_usd': str(total), 'cost_total_usd': 1.819385,
              'current_state': pin(raw), 'original_launch': pin((CLOUD / 'launch-r1-20260927-19.json').read_bytes()),
              'recovery_bootstrap': pin((HERE / 'recovery-only-bootstrap.sh').read_bytes()),
              'source': pin(Path(__file__).read_bytes()), 'budget_before': pin(before), 'cloud_started': False}
    row.setdefault('events', []).append(record)
    after = (json.dumps(ledger, indent=2, ensure_ascii=False) + '\n').encode()
    new(HERE / 'budget-before-recovery-only.json', before)
    new(HERE / 'recovery-only-amendment.json', (json.dumps({**record, 'budget_after': pin(after)}, indent=2)+'\n').encode())
    temporary = CLOUD / 'budget-vm19-recovery.tmp'
    new(temporary, after)
    os.replace(temporary, CLOUD / 'budget.json')
    print(json.dumps({'recovery_stop_utc': stop.isoformat(), 'cost_model_usd': 1.819385,
                      'reserved_usd_unchanged': 1.85, 'budget_after': pin(after)}))


if __name__ == '__main__':
    main()
