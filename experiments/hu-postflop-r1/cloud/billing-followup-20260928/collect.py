"""Two bounded read-only metadata GETs. No query jobs, export setup, or VM access."""
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time
import urllib.error
import urllib.request

HERE = Path(__file__).resolve().parent
PROJECT = 'solvers-abstraction-20260723'
SDK = r'C:\Program Files (x86)\Google\Cloud SDK\google-cloud-sdk\lib\gcloud.py'
URLS = {
    'project-billing': f'https://cloudbilling.googleapis.com/v1/projects/{PROJECT}/billingInfo',
    'datasets': f'https://bigquery.googleapis.com/bigquery/v2/projects/{PROJECT}/datasets?all=true&maxResults=50',
}


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def save(name, raw):
    with (HERE / name).open('xb') as stream:
        stream.write(raw)
    return {'path': name, **pin(raw)}


def main():
    if (HERE / 'acquisition.json').exists():
        raise ValueError('No overwrite or automatic retry')
    record = {'schema': 'r1.billing-metadata-followup/v1', 'project': PROJECT,
              'started_at': now(), 'source': pin(Path(__file__).read_bytes()), 'requests': [],
              'maximum_gets': 2, 'maximum_response_bytes': 256 * 1024,
              'credential_material_retained': False, 'query_jobs': False, 'resource_mutations': False,
              'ledger_mutations': False, 'vm_access': False, 'billed_amount': None}
    deadline, token = time.monotonic() + 75, None
    try:
        auth = subprocess.run([sys.executable, SDK, 'auth', 'print-access-token', '--quiet'],
                              capture_output=True, timeout=25)
        record['authentication'] = {'returncode': auth.returncode, 'stderr_bytes': len(auth.stderr)}
        if auth.returncode != 0:
            raise ValueError('Existing authentication unavailable')
        token = auth.stdout.decode('ascii').strip()
        if not token or any(c.isspace() for c in token):
            raise ValueError('Unexpected credential response')
        for name, url in URLS.items():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError('Metadata acquisition deadline exceeded')
            entry = {'name': name, 'method': 'GET', 'url': url, 'requested_at': now()}
            record['requests'].append(entry)
            request = urllib.request.Request(url, headers={'Authorization': 'Bearer ' + token})
            try:
                response = urllib.request.urlopen(request, timeout=min(20, remaining))
            except urllib.error.HTTPError as error:
                response = error
            with response:
                raw = response.read(256 * 1024 + 1)
                if len(raw) > 256 * 1024:
                    raise ValueError('Metadata response size exceeded')
                entry.update(http_status=response.status, ended_at=now(), response=save(name + '.json', raw))
            parsed = json.loads(raw)
            entry['next_page_present'] = bool(parsed.get('nextPageToken'))
            entry['unreachable'] = parsed.get('unreachable', [])
        record['status'] = 'responses_captured'
    except Exception as error:
        record.update(status='unavailable_or_incomplete', error_type=type(error).__name__)
    finally:
        token = None
        record['ended_at'] = now()
        save('acquisition.json', (json.dumps(record, indent=2) + '\n').encode())
    print(json.dumps({'status': record['status'], 'requests': [
        {'name': r['name'], 'http_status': r.get('http_status'), 'bytes': r.get('response', {}).get('bytes')}
        for r in record['requests']]}))


if __name__ == '__main__':
    main()
