"""Cloud-only compact inspection of failed profile; no replay or native solve."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

assert sys.platform == 'linux'
proof = Path('/opt/r1/flop-cpu-profile-proof01')
execution = json.loads((proof / 'execution.json').read_bytes())
failed, = [row for row in execution['stages'] if row['status'] == 'failed']
directory = proof / failed['name']
files = {p.relative_to(directory).as_posix(): p.stat().st_size for p in directory.rglob('*') if p.is_file()}
logs = {}
for p in [directory / 'script.stderr.log', directory / 'dump.stderr.log',
          Path('/opt/r1/flop-cpu-profile-measure-wrapper01/measure.stderr.log')]:
    if p.exists():
        with p.open('rb') as stream:
            stream.seek(max(0, p.stat().st_size - 8192))
            logs[str(p)] = stream.read().decode(errors='replace')
plan = json.loads((proof / 'measurement.json').read_bytes())
perf = plan['tools']['perf']['path']
help_text = subprocess.run([perf, 'script', '-h'], capture_output=True, timeout=10)
help_bytes = help_text.stdout + help_text.stderr
print(json.dumps({'execution_status': execution['status'], 'counts': execution['counts'],
    'failed_row': failed, 'files': files, 'log_tails': logs,
    'perf_script_help_returncode': help_text.returncode,
    'perf_script_help_sha256': hashlib.sha256(help_bytes).hexdigest(),
    'perf_script_help': help_bytes.decode(errors='replace')}, indent=2))
