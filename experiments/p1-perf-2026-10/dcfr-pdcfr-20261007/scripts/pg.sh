#!/bin/bash
# VM6 phase G: DCFR candidate s2 vs default on new spots and the i16-based storages (sweep3.py).
until grep -q PHASEF_DONE ~/results/progress.txt 2>/dev/null; do sleep 30; done
python3 ~/sweep3.py > ~/results/sweep3.log 2>&1
grep -q PHASEG_DONE ~/results/progress.txt || echo "PHASEG_DONE (sweep3 exit $?)" >> ~/results/progress.txt
