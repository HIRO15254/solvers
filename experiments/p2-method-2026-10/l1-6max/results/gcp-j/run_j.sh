#!/bin/bash
# Paired timing of J (7b8d704) against I (46060c6) in the order i,j,j,i, on B7 (30 iterations) and B4 Simple
# (12 iterations) with the solver's K4 at 256 samples, at least 16, and no evaluation; with 32 threads (all of
# c2d-highcpu-32) and with 16 threads as on the local machine. /usr/bin/time -v records CPU time, max RSS and page faults;
# the average profiles of i and j are compared byte for byte.
cd ~/j
ehs=~/ehs2/v2-f32-t32-r32.postcard
r=~/results
for threads in 32 16; do
  extra=(--threads $threads)
  for run in b7:examples/bench/6max_20bb.toml:30 b4s:examples/bench/6max_100bb_nl50_partial_simple_reference.toml:12; do
    IFS=: read -r name config iterations <<< "$run"
    for pair in 1:i 2:j 3:j 4:i; do
      n=${pair%%:*}
      x=${pair#*:}
      tag=$name-t$threads-$n-$x
      /usr/bin/time -v -o $r/$tag.time ~/trunk_solve-$x --config $config --leaf-model l1 --ehs2-cache $ehs \
        --solver-k4-samples 256 --solver-k4-min-samples 16 --iterations $iterations --eval-every 0 --print-every 1 \
        "${extra[@]}" --output $r/$tag.json --output-profile $r/$tag.profile.json > $r/$tag.log 2>&1
      echo "$(date -u +%T) $tag exit $?" >> $r/progress.txt
    done
    for n in 1 2 3 4; do sha256sum $r/$name-t$threads-$n-*.profile.json; done > $r/$name-t$threads.profiles.sha256
    rm -f $r/$name-t$threads-*.profile.json
  done
done
echo "$(date -u +%T) ALL_DONE" >> $r/progress.txt
