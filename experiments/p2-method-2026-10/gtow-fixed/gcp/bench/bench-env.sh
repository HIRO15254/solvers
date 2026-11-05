EXP=experiments/p2-method-2026-10/gtow-fixed
REPLAN=$EXP/results/replan
V2=$EXP/results/v2
BENCH=$EXP/results/bench
BENCH_INPUT=$EXP/gcp/bench
CACHE=.cache/p2-trunk/gtow-fixed
BENCH_CACHE=$CACHE/bench
SCRATCH=../_goal/bench
LINES36=$REPLAN/all36-lines.txt
OLD_FLOPS8=$EXP/gcp/replan/p1f/flops.txt
GTOW_PROFILE=$CACHE/gtow-raise5.profile.json
GTOW_REFERENCE=experiments/p2-method-2026-10/b4s-8h/references/gtowizard-simple-100bb-preflop-tree-2026-10-09.json
E2_PROFILE=$CACHE/v2/e2/final/traise5+p10+g25+hd.profile.json
E2_POSTFLOP=$CACHE/v2/e2/final/traise5+p10+g25+hd.postflop.bin
E2SD1_PROFILE=$CACHE/v2/e2sd1/final/traise5+p10+g25+hd+sd1.profile.json
E2SD1_POSTFLOP=$CACHE/v2/e2sd1/final/traise5+p10+g25+hd+sd1.postflop.bin
E2_L1=rel8x2@raise5+g25+hd+sd1
E2_L1_SNAPSHOT=$CACHE/replan/gtowflop-sd1/$E2_L1/postflop.bin
SOLVE_L1=rel8x2@raise5+g25+hd
DRAWS_ONLY=rel8x2@raise5+hd+sd1
IMP=$EXP/results/improve
RULER_INPUT=$BENCH_CACHE/ruler-input

prep() {
  up=$SCRATCH/$1-up; rm -rf "$up"; mkdir -p "$up"
  cp $EXP/{buckets.sh,tokens.sh,bench_diag.sh,p1_classes.py,p1_leaf.py,flop_compare.py} $EXP/subsets/{k25,k49}-groups.txt \
     experiments/p2-method-2026-10/b4s-8h/{compare.py,b4s-gtow-raise5.toml} $BENCH_INPUT/{trunk_eval.sh,e2_flags.sh,override_solve.sh} \
     $LINES36 $BENCH_INPUT/{bands.txt,lines-srp.txt,lines-3bet.txt,lines-4bet.txt,lines-5bet.txt,flops16.txt,flops32.txt,flops64.txt} "$up/"
  cp "$GTOW_PROFILE" "$up/gtow.profile.json"
  if [[ -d $RULER_INPUT ]]; then cp "$RULER_INPUT"/*.json "$up/"; fi
  cp "$BENCH_INPUT/steps/$1.txt" "$up/steps.txt"
}
launch() {
  ( cd experiments/p2-method-2026-10 && AWS_PROFILE="${AWS_PROFILE:-solvers-spot}" INSTANCE_TYPES="${INSTANCE_TYPES:-c6a.8xlarge c6i.8xlarge c7a.8xlarge c7i.8xlarge}" \
    bash b4s-8h/aws/aws_run.sh "p2-bench-$1" "$2" "../../$SCRATCH/$1" gtow-fixed/gcp/bench/job_bench.sh ../../$SCRATCH/$1-up/* )
}
# prep_full <run> <variant> <iterations> <seconds>: inputs of one full solve (relative-reach/gcp/job.sh) with the
# ruler evaluations (eval-overrides.txt) and without uploading the final Postflop.
prep_full() {
  up=$SCRATCH/$1-up; rm -rf "$up"; mkdir -p "$up"
  cp experiments/p2-method-2026-10/b4s-8h/{b4s-gtow.toml,b4s-gtow-raise5.toml,b4s-gtow-gt5c.toml,compare.py} \
     experiments/p2-method-2026-10/b4s-8h/references/gtowizard-simple-100bb-preflop-tree-2026-10-09.json \
     $EXP/subsets/{k25,k49}-groups.txt "$RULER_INPUT"/{truth-p1.json,truth-multi.json} "$up/"
  echo "$2" > "$up/variants.txt"; echo "$3" > "$up/iterations.txt"; echo "$4" > "$up/seconds.txt"
  echo '100000 2000 0' > "$up/plateau.txt"
  echo 'truth-p1.json truth-multi.json' > "$up/eval-overrides.txt"
  : > "$up/drop_postflop_snapshots"; : > "$up/drop_final_postflop"
}
launch_full() {
  ( cd experiments/p2-method-2026-10 && AWS_PROFILE="${AWS_PROFILE:-solvers-spot}" INSTANCE_TYPES="${INSTANCE_TYPES:-c6a.8xlarge c6i.8xlarge c7a.8xlarge c7i.8xlarge}" \
    bash b4s-8h/aws/aws_run.sh "p2-full-$1" "$2" "../../$SCRATCH/$1" relative-reach/gcp/job.sh ../../$SCRATCH/$1-up/* )
}
# collect <run>: after ALL_DONE, keep logs in $BENCH/aws-<run>, unpack outputs into $BENCH_CACHE, print hours and cost.
collect() {
  local S=$SCRATCH/$1
  grep -q ALL_DONE $S/results/progress.txt || { echo "no ALL_DONE in $1"; return 1; }
  mkdir -p $BENCH/aws-$1 $BENCH_CACHE/aws
  cp $S/results/*.txt $S/results/*.log $S/vm.log $S/delete.log $BENCH/aws-$1/ 2>/dev/null
  cp $S/final.tgz $BENCH_CACHE/aws/$1.final.tgz && tar -xzf $BENCH_CACHE/aws/$1.final.tgz -C $BENCH_CACHE --strip-components=1
  rm -rf $SCRATCH/$1-up
  echo "$1 $(grep -o 'spot-price-usd-per-hour=[0-9.]*' $S/vm.log | tail -1 | cut -d= -f2) $(python experiments/p2-method-2026-10/b4s-8h/aws/vm_hours.py $S/vm.log | tr -d '\r')"
}
# collect_full <run>: after ALL_DONE, keep logs in $IMP/aws-<run>, unpack final.tgz into $CACHE/improve/<run>, print hours and cost.
collect_full() {
  local S=$SCRATCH/$1 D=$IMP/aws-$1
  grep -q ALL_DONE $S/results/progress.txt || { echo "no ALL_DONE in $1"; return 1; }
  mkdir -p $D $CACHE/improve/aws $CACHE/improve/$1
  cp $S/results/*.json $S/results/*.txt $S/results/*.log $S/results/*.time $S/vm.log $S/delete.log $SCRATCH/$1-up/variants.txt $D/ 2>/dev/null
  cp $S/final.tgz $CACHE/improve/aws/$1.final.tgz && tar -xzf $CACHE/improve/aws/$1.final.tgz -C $CACHE/improve/$1
  echo "$1 $(grep -o 'spot-price-usd-per-hour=[0-9.]*' $S/vm.log | tail -1 | cut -d= -f2) $(python experiments/p2-method-2026-10/b4s-8h/aws/vm_hours.py $S/vm.log | tr -d '\r')"
}
