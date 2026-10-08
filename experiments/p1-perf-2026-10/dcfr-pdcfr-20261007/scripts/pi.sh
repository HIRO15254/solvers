#!/bin/bash
# VM6 phase I: legacy i16 floor on Turn, pow4_reset vs cfr_precision (sweep4.py).
until grep -q PHASEH_DONE ~/results/progress.txt 2>/dev/null; do sleep 30; done
python3 ~/sweep4.py > ~/results/sweep4.log 2>&1
grep -q PHASEI_DONE ~/results/progress.txt || echo "PHASEI_DONE (sweep4 failed)" >> ~/results/progress.txt
