"""Bind compact CPU-profile evidence to VM20 before invoking its unchanged-policy summary.

Local compact JSON only; no API, native executable or archive access. Requires
the later state32-01 SDK capture and completed trusted-reader report/receipt.
"""
import argparse
import datetime as dt
import importlib.util
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
NAME = 'solvers-r1-20260928-20'
INSTANCE_ID = '2562385330130146252'
PROJECT = 'solvers-abstraction-20260723'
ZONE = 'us-central1-b'
CREATION = {'bytes': 30180, 'sha256': 'deb00a6adec077727d194d9fc9f0d9d7edb49715a6d923128f1d698499a41552'}
SUMMARY = {'bytes': 19334, 'sha256': '2f0ba9a074cedbbedd3530996443bcf24021e4eb522b38418eaa579cbf2d26b4'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ('report', 'receipt', 'json', 'markdown', 'guard-json'):
        parser.add_argument('--' + flag, type=Path, required=True)
    parser.add_argument('--top', type=int, default=10)
    args = parser.parse_args()
    source = HERE / 'summarize-profile.py'
    spec = importlib.util.spec_from_file_location('vm20_compact_summary', source)
    summary = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(summary)
    summary.need(summary.pin(summary.small_file(source)) == SUMMARY, 'Frozen VM20 summarizer changed')
    summary.need(not args.guard_json.exists() and len({p.resolve() for p in (args.json, args.markdown, args.guard_json)}) == 3,
                 'Fresh distinct summary/guard paths required')
    pins = {}
    def read(path):
        raw = summary.small_file(path)
        pins[str(path.resolve())] = summary.pin(raw)
        return summary.loads(raw)
    created_path = HERE.parent / 'create-result-r1-20260928-20.json'
    created_list = read(created_path)
    summary.need(pins[str(created_path.resolve())] == CREATION and len(created_list) == 1, 'Frozen VM20 creation changed')
    created = created_list[0]
    summary.need(created['id'] == INSTANCE_ID and created['name'] == NAME, 'Creation belongs to another VM')
    original_stop = created['scheduling']['terminationTime']
    create_receipt = read(HERE / 'create.receipt.json')
    summary.need(create_receipt['exit_code'] == 0 and create_receipt['reservation_id'] == 'r1-20260928-20'
                 and create_receipt['termination_time'] == original_stop, 'Create receipt failed/different')
    state_receipt = read(HERE / 'state32-01.result.json')
    argv = state_receipt['argv']
    summary.need(state_receipt['exit_code'] == 0 and argv[1:5] == ['compute', 'instances', 'describe', NAME],
                 '32-CPU state query differs')
    for key, value in (('project', PROJECT), ('zone', ZONE), ('format', 'json')):
        summary.need([arg for arg in argv if arg.startswith('--' + key + '=')] == ['--' + key + '=' + value]
                     and '--' + key not in argv, '32-CPU query scope differs')
    state_path = HERE / 'state32-01.stdout.log'
    state = read(state_path)
    summary.need(state_receipt['stdout'] == pins[str(state_path.resolve())], '32-CPU stdout pin differs')
    stderr_path = HERE / 'state32-01.stderr.log'
    stderr = summary.small_file(stderr_path)
    pins[str(stderr_path.resolve())] = summary.pin(stderr)
    summary.need(state_receipt['stderr'] == summary.pin(stderr), '32-CPU stderr pin differs')
    base = f'https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}'
    summary.need(state['id'] == INSTANCE_ID and state['name'] == NAME and state['status'] == 'RUNNING'
                 and state['selfLink'] == base + '/instances/' + NAME
                 and state['machineType'] == base + '/machineTypes/e2-highcpu-32', '32-CPU observed identity differs')
    summary.need(state['scheduling']['terminationTime'] == original_stop and state['scheduling']['provisioningModel'] == 'SPOT'
                 and state['scheduling']['instanceTerminationAction'] == 'STOP'
                 and state['scheduling']['automaticRestart'] is False, '32-CPU scheduling differs')
    report = read(args.report)
    receipt = read(args.receipt)
    for name, cpus in (('build_host', 2), ('measurement_host', 32)):
        summary.need(report[name]['instance_id'] == INSTANCE_ID and report[name]['logical_cpus'] == cpus,
                     'Trusted report host differs from VM20')
    summary.need(report['build_host']['boot_id'] != report['measurement_host']['boot_id'], 'Build/measurement boot boundary missing')
    stamp = lambda value: dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    times = [stamp(created['creationTimestamp']), stamp(state_receipt['started_utc']),
             stamp(state_receipt['ended_utc']), stamp(receipt['started_at']), stamp(receipt['ended_at'])]
    summary.need(all(t.utcoffset() is not None for t in times) and times == sorted(times), 'Cloud state/reader timing order differs')
    # All original reader, hash, completeness, loss/throttle, and leaf-total
    # checks execute in the fixed summarizer after the cloud identity guard.
    sys.argv = [str(source), '--report', str(args.report), '--receipt', str(args.receipt), '--json', str(args.json),
                '--markdown', str(args.markdown), '--top', str(args.top)]
    summary.main()
    result = {'schema': 'r1.vm20-profile-summary-guard/v1', 'status': 'intended_instance_and_trusted_summary_verified',
              'instance_id': INSTANCE_ID, 'instance_name': NAME, 'project': PROJECT, 'zone': ZONE,
              'original_stop_utc': original_stop, 'inputs': pins, 'summarizer': SUMMARY,
              'guard_source': summary.pin(summary.small_file(__file__)),
              'summary_json': summary.pin(summary.small_file(args.json)),
              'summary_markdown': summary.pin(summary.small_file(args.markdown)),
              'api_or_native_or_archive_access': False}
    with args.guard_json.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(result, stream, indent=2)
        stream.write('\n')


if __name__ == '__main__':
    main()
