"""Running hours (and USD by each instance's launch Spot price) from an aws_run.sh vm.log.

usage: python vm_hours.py VM_LOG  -> "<hours> <usd>"
A running interval closes at stopping/stopped/shutting-down/terminated, at the abandonment or deletion of its
instance, or at the last log line. Several runners may append to one log (a stopped run relaunched under the same name).
"""
import datetime as D
import re
import sys

price, start, hours, usd, last = {}, {}, 0.0, 0.0, None


def close(instance, at):
    global hours, usd
    h = (at - start.pop(instance)).total_seconds() / 3600
    hours += h
    usd += h * price[instance]


for line in open(sys.argv[1], encoding='utf-8', errors='replace'):
    stamp = re.match(r'(\S+Z) ', line)
    if not stamp:
        continue
    last = D.datetime.fromisoformat(stamp[1].replace('Z', '+00:00'))
    using = re.search(r'using region=\S+ instance=(\S+) .*spot-price-usd-per-hour=(\S+)', line)
    if using:
        price[using[1]] = float(using[2])
        continue
    gone = re.search(r'(?:abandoned .*|deleted )instance=(i-\w+)', line)
    if gone and gone[1] in start:
        close(gone[1], last)
        continue
    status = re.search(r'instance=(i-\w+) status=(\w+)', line)
    if not status:
        continue
    if status[2] == 'running':
        start.setdefault(status[1], last)
    elif status[1] in start:
        close(status[1], last)
for instance in list(start):
    close(instance, last)
print(f'{hours:.2f} {usd:.3f}')
