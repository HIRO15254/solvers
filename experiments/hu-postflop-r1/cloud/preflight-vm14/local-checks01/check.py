"""Small local source/docs/ledger checks; no native artifact traversal or cloud I/O."""
import ast
import copy
from datetime import datetime, timezone
from decimal import Decimal
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
CLOUD = ROOT / 'experiments/hu-postflop-r1/cloud'
D = Decimal


def pin_bytes(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def small(path):
    if path.stat().st_size > 2 * 1024**2:
        raise ValueError('Out of small-file scope: ' + str(path))
    return path.read_bytes()


def load(path):
    return json.loads(small(path))


def main():
    started = datetime.now(timezone.utc).isoformat()
    status = subprocess.run(['git', 'status', '--porcelain=v1', '-z', '--untracked-files=all'],
                            cwd=ROOT, check=True, capture_output=True).stdout
    (HERE / 'git-status.bin').write_bytes(status)
    pieces = iter(status.decode('utf-8').split('\0'))
    changed = []
    for item in pieces:
        if not item:
            continue
        changed.append(item[3:])
        if 'R' in item[:2] or 'C' in item[:2]:
            next(pieces)
    python = sorted(p for p in changed if p.endswith('.py') and (ROOT / p).is_file())
    markdown = sorted(p for p in changed if p.endswith('.md') and (ROOT / p).is_file())
    shell = sorted(p for p in changed if p.endswith('.sh') and (ROOT / p).is_file())
    docs_path = ROOT / 'tools/check_docs.py'
    spec = importlib.util.spec_from_file_location('local_doc_checker', docs_path)
    docs = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(docs)
    full_docs = docs.documents(ROOT)
    audit = CLOUD / 'usage-cost-bound-20260927'
    inputs = {ROOT / p for p in [*python, *markdown, *shell]}
    inputs.update(full_docs)
    inputs.update([docs_path, CLOUD / 'budget.json', CLOUD / 'preflight-vm14/reservation.json',
                   audit / 'applied.json', audit / 'inputs.json', audit / 'report.json', audit / 'calculate.py',
                   audit / 'budget-before.json', audit / 'monitoring-usage.json'])
    applied = load(audit / 'applied.json')
    inputs.update(CLOUD / e['path'] for e in applied['evidence'])
    source_before = {str(p.relative_to(ROOT)): pin_bytes(small(p)) for p in sorted(inputs)}
    results, errors = [], []

    def check(name, function):
        timer = time.perf_counter()
        try:
            detail = function()
            result = {'name': name, 'status': 'passed', 'detail': detail}
        except Exception as error:
            errors.append({'name': name, 'error': repr(error)})
            result = {'name': name, 'status': 'failed', 'error': repr(error)}
        result['elapsed_seconds'] = time.perf_counter() - timer
        results.append(result)

    def syntax():
        for name in python:
            ast.parse(small(ROOT / name).decode('utf-8-sig'), filename=name)
        embedded = []
        for name in shell:
            snippets = re.findall(r"<<'PY'[^\n]*\n(.*?)\nPY(?:\n|$)", small(ROOT / name).decode(), re.S)
            for number, text in enumerate(snippets):
                ast.parse(text, filename=f'{name}:embedded{number}')
                embedded.append({'file': name, 'snippet': number})
        return {'python_files': python, 'python_count': len(python), 'embedded_python': embedded,
                'shell_syntax_executed': False}

    def command(label, argv):
        timer = time.perf_counter()
        result = subprocess.run(argv, cwd=ROOT, capture_output=True, timeout=30)
        for name, raw in [('stdout', result.stdout), ('stderr', result.stderr)]:
            (HERE / (label + '.' + name + '.log')).write_bytes(raw)
        if result.returncode:
            raise ValueError(f'{label} exit{result.returncode}: {result.stderr.decode(errors="replace")}')
        return {'argv': argv, 'exit_code': result.returncode, 'elapsed_seconds': time.perf_counter() - timer,
                'stdout': pin_bytes(result.stdout), 'stderr': pin_bytes(result.stderr)}

    def changed_links():
        missing, links = [], 0
        for name in markdown:
            path = ROOT / name
            for number, line in docs.prose_lines(small(path).decode('utf-8-sig')):
                for target in docs.link_destinations(line):
                    resolved = docs.local_target(path, target, ROOT)
                    if resolved is not None:
                        links += 1
                        if not resolved.exists():
                            missing.append({'file': name, 'line': number, 'target': target})
        if missing:
            raise ValueError(json.dumps(missing))
        return {'markdown_files': markdown, 'count': len(markdown), 'local_link_destinations': links,
                'anchors_or_remote_links_checked': False}

    def ledger():
        budget = load(CLOUD / 'budget.json')
        rows = budget['reservations']
        assert len({r['id'] for r in rows}) == len(rows)
        held = sum(D(str(r['reserved_usd'])) for r in rows if not r['reservation_released'])
        limit = D(str(budget['authorized_limit']))
        assert held == 38 and limit == 40 and limit - held == 2
        new = load(CLOUD / 'preflight-vm14/reservation.json')
        current = next(r for r in rows if r['id'] == new['id'])
        assert all(current[k] == value for k, value in new.items())
        assert D(str(new['reserved_usd'])) == 3 and not new['reservation_released']
        report = load(audit / 'report.json')
        for identifier, value in report['per_reservation_proposed_holds_usd'].items():
            r = next(r for r in rows if r['id'] == identifier)
            assert D(str(r['reserved_usd'])) == D(value) and not r['reservation_released']
        group = sum(D(str(r['reserved_usd'])) for r in rows if r['id'] in report['per_reservation_proposed_holds_usd'])
        assert group == 15 and held - group - 3 == 20
        return {'authorized_usd': str(limit), 'held_usd': str(held), 'unreserved_usd': str(limit-held),
                'vm08_13_usd': str(group), 'vm14_usd': '3', 'earlier_usd': '20'}

    def reconciliation():
        for item in applied['evidence']:
            assert pin_bytes(small(CLOUD / item['path'])) == {k: item[k] for k in ['bytes', 'sha256']}
        before = small(audit / 'budget-before.json')
        assert pin_bytes(before) == {k: applied['ledger_before'][k] for k in ['bytes', 'sha256']}
        budget = load(CLOUD / 'budget.json')
        reconstruct = json.loads(before)
        for change in applied['changes']:
            row = next(r for r in reconstruct['reservations'] if r['id'] == change['id'])
            assert row['reserved_usd'] == change['old_reserved_usd']
            row['original_reserved_usd'] = row.get('original_reserved_usd', change['old_reserved_usd'])
            row['reserved_usd'] = change['new_reserved_usd']
            row.setdefault('events', []).append({k: v for k, v in change.items() if k != 'id'})
        added_authorizations = budget['authorization_events'][len(reconstruct['authorization_events']):]
        assert len(added_authorizations) == 1 and added_authorizations[0]['additional_usd'] == 0
        reconstruct['authorization_events'].extend(added_authorizations)
        event = {k: v for k, v in applied.items() if k not in ['ledger_before', 'ledger_after']}
        assert event in budget['usage_reconciliations']
        reconstruct.setdefault('usage_reconciliations', []).append(event)
        raw = (json.dumps(reconstruct, ensure_ascii=False, indent=2) + '\n').encode()
        assert pin_bytes(raw) == {k: applied['ledger_after'][k] for k in ['bytes', 'sha256']}
        current_old = [r for r in budget['reservations'] if r['id'] != 'r1-20260927-14']
        assert current_old == reconstruct['reservations']
        assert sum(D(str(c['restored_to_available_budget_usd'])) for c in applied['changes']) == 5
        return {'four_evidence_pins_match': True, 'reconstructed_after_ledger_pin_matches': True,
                'current_pre_vm14_reservations_match': True, 'restored_usd': '5',
                'final_invoice_claim': applied['actual_billed_usd'], 'guaranteed_ceiling': applied['guaranteed_cost_ceiling']}

    check('uncommitted_python_ast', syntax)
    check('existing_docs_validator', lambda: command('docs', [sys.executable, '-B', 'tools/check_docs.py']))
    check('changed_markdown_local_links', changed_links)
    check('budget38_and_unreserved2', ledger)
    check('restored_report_and_ledger_hashes', reconciliation)
    check('cost_report_recompute', lambda: command('cost', [sys.executable, '-B', str((audit / 'calculate.py').relative_to(ROOT)), '--check']))
    source_after = {str(p.relative_to(ROOT)): pin_bytes(small(p)) for p in sorted(inputs)}
    changed_during = [p for p in source_before if source_before[p] != source_after[p]]
    if changed_during:
        errors.append({'name': 'source_stability', 'changed': changed_during})
    receipt = {'schema': 'r1.vm14-local-light-checks/v1', 'started_at': started,
               'ended_at': datetime.now(timezone.utc).isoformat(), 'source_before': source_before,
               'source_after': source_after, 'source_unchanged': not changed_during, 'results': results,
               'status': 'passed' if not errors else 'failed', 'errors': errors,
               'heavy_native_hashes_compression_builds_solves_or_cloud_calls': False}
    (HERE / 'receipt.json').write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'status': receipt['status'], 'python_count': len(python), 'markdown_count': len(markdown),
                      'checks': [{'name': r['name'], 'status': r['status'], 'seconds': r['elapsed_seconds']} for r in results],
                      'source_unchanged': not changed_during, 'errors': errors}))
    return bool(errors)


if __name__ == '__main__':
    raise SystemExit(main())
