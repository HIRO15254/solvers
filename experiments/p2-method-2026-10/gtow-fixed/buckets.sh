#!/usr/bin/env bash
# Solve the leaves of results/lines.txt (or the file $LINES) under GTO Wizard's preflop (gtow.profile.json) with other Postflop card
# buckets or menus, then diagnose the saved Postflop values. Run from the repository root.
# usage: bash experiments/p2-method-2026-10/gtow-fixed/buckets.sh <gtow.profile.json> <out dir> <iterations> VARIANT...
#   VARIANT: BUCKETS[@MENU][+OPTION...]
#   BUCKETS: rel<N>x<L> (range-relative N buckets x L strength levels, prior 10 combos) | ehs<N> (fixed EHS2 N
#            buckets, N = 32 or 128)
#   MENU: the Postflop menu of experiments/p2-method-2026-10/*/b4s-gtow-<MENU>.toml (default b4s-gtow.toml)
#   OPTION: tex<T> (--l1-refine texture:T,recall:1) | b<N> (--l1-boards N) | pb<X> (--l1-postflop-beta X) |
#           g<K> (--l1-flop-groups subsets/k<K>-groups.txt) | gf<K> (the same, flop only) |
#           gp<K> / gpf<K> (the same with $PIO_SUBSETS/k<K>-groups.txt of pio_subsets.py) | it<N> (N iterations instead of <iterations>) |
#           pr<M> (--l1-relative-prior M instead of 10) | pf<F> (--l1-relative-prior-fraction F: add F x the actor's
#           reach total; pair with pr0 for a purely proportional prior) | h[mN][d][pN][f][q][b][g] (--l1-hand made:N, draws, potential:N, flop-draws, pairs, backdoor, gutshot; tokens.sh) |
#           sd<N> (--l1-seed N --seed N: replicate with another training seed) |
#           ro<KINDS> (--l1-runout; letters f flush, p pair, s straight, o over, e.g. rofp = flush,pair) |
#           lspr<MAX>r<N>x<L>g<K> (--l1-low-spr MAX:NxL:subsets/kK-groups.txt; only selected leaves)
#           rc<N> (--l1-refine recall:N; persistent coarse flop strength in relative mode) |
#           sub<K> (--fixed-flops subsets/k<K>.txt: train only on the K weighted representatives, B01; pair with g<K>)
#           sspr<X> (--l1-flop-subset-max-spr X: sub<K> only at flop SPR <= X; higher SPR uses standard boards)
#           own (--l1-leaf-flop-groups own:<profile>: each leaf's own 49 flop groups from the fixed profile; P2D12) |
#           ow (--l1-regret-weight own: postflop regrets weighted by own reach within the bucket) |
#           st<K> (rel only: the K-bucket EHS2 table v2-fK-tK-rK.postcard orders strength instead of K = 128)
# SNAPSHOTS=1000,2000,4000 retains fixed-profile Postflop states at these iterations.
# STATE_DIR=DIR resumes fixed-profile training from DIR/<variant>.fixed.state; saves every 300 seconds and at completion.
# POSTFLOP=FILE skips the solve and diagnoses that saved Postflop average (e.g. postflop.bin.it4000) into <out>/VARIANT.
# DIAG_FLOPS=FILE prints the diagnosis per flop of FILE (l1_leaf_diag --flops); DIAG_BOARDS=N
# overrides the 2048 sampled boards and DIAG_SEED=N their sampling seed.
# NODIAG=1 only solves (no l1_leaf_diag loop), e.g. to time a variant.
# DIAG_RIVERS=R diagnoses R rivers per turn (l1_leaf_diag --rivers); DIAG_JOBS=N runs up to N line diagnoses at once.
# DIAG_DUMP=strategy prints each hero decision's class frequencies instead of the values (l1_leaf_diag --dump strategy).
# REBIND=1 (with POSTFLOP, raise5 only) diagnoses a Postflop average trained before the 2026-11-05 merge (the 2026-10-29
#   and 2026-11-05 merges changed the game fingerprint, not the game): it is copied once to FILE.v3 under the current
#   game fingerprint (l1_leaf_diag --rebind-postflop; same game, README) and FILE.v3 is read.
# EXE_SUFFIX (default .exe), EHS2_DIR and TABLES_DIR override the local binaries and tables.
set -uo pipefail
profile=${1:?profile}; out=${2:?out}; iters=${3:?iterations}; shift 3
here=experiments/p2-method-2026-10/gtow-fixed
tables=${EHS2_DIR:-$LOCALAPPDATA/solvers/ehs2}
lines_file=${LINES:-$here/results/lines.txt}
lines=$(cat "$lines_file")
source "$here/tokens.sh"
mkdir -p "$out"
for v in "$@"; do
  mkdir -p "$out/$v"
  base=${v%%+*}
  solve_args=()
  iters_override=
  shared_args=()
  strength=128
  if [ "$base" != "$v" ]; then
    IFS='+' read -ra parts <<< "${v#*+}"
    for part in "${parts[@]}"; do
      case $part in
        tex*) shared_args+=(--l1-refine "texture:${part#tex},recall:1") ;;
        rc*) shared_args+=(--l1-refine "recall:${part#rc}") ;;
        lspr*) spec=$(low_spr_spec "$part" "$here/subsets") || { echo "unknown low-SPR split $part"; exit 2; }; shared_args+=(--l1-low-spr "$spec") ;;
        b*) solve_args+=(--l1-boards "${part#b}") ;;
        sd*) solve_args+=(--l1-seed "${part#sd}" --seed "${part#sd}") ;;
        sub*) solve_args+=(--fixed-flops "$here/subsets/k${part#sub}.txt") ;;
        sspr*) solve_args+=(--l1-flop-subset-max-spr "${part#sspr}") ;;
        pb*) solve_args+=(--l1-postflop-beta "${part#pb}") ;;
        ow) solve_args+=(--l1-regret-weight own) ;;
        own) shared_args+=(--l1-leaf-flop-groups "own:$profile") ;;
        st*) strength=${part#st}; shared_args+=(--l1-strength-buckets "$strength") ;;
        h*) hs=$(hand_spec "${part#h}") || { echo "unknown hand split $part"; exit 2; }; shared_args+=(--l1-hand "$hs") ;;
        gpf*) shared_args+=(--l1-flop-groups "${PIO_SUBSETS:-../_goal/fixed/pio}/k${part#gpf}-groups.txt:flop") ;;
        gp*) shared_args+=(--l1-flop-groups "${PIO_SUBSETS:-../_goal/fixed/pio}/k${part#gp}-groups.txt") ;;
        gf*) shared_args+=(--l1-flop-groups "$here/subsets/k${part#gf}-groups.txt:flop") ;;
        g*) shared_args+=(--l1-flop-groups "$here/subsets/k${part#g}-groups.txt") ;;
        ro*) rs=$(runout_spec "${part#ro}") || { echo "unknown runout $part"; exit 2; }; shared_args+=(--l1-runout "$rs") ;;
        pr*) shared_args+=(--l1-relative-prior "${part#pr}") ;;
        pf*) shared_args+=(--l1-relative-prior-fraction "${part#pf}") ;;
        it*) iters_override=${part#it} ;;
        *) echo "unknown part $part of $v"; exit 2 ;;
      esac
    done
  fi
  buckets=${base%%@*}
  if [ "$buckets" = "$base" ]; then
    cfg=experiments/p2-method-2026-10/b4s-8h/b4s-gtow.toml
  else
    cfg=$(ls experiments/p2-method-2026-10/*/b4s-gtow-"${base#*@}".toml) || exit 2
  fi
  case $buckets in
    rel*) args=(--ehs2-cache "$tables/v2-f32-t32-r32.postcard"
                --l1-relative "${buckets#rel}:$tables/v2-f$strength-t$strength-r$strength.postcard" --l1-relative-prior 10) ;;
    ehs32) args=(--ehs2-cache "$tables/v2-f32-t32-r32.postcard") ;;
    ehs128) sed -E "/^\[solver\.abstraction\.buckets\]/,/^\[/ s/^(flop|turn|river) = 32$/\1 = 128/" "$cfg" \
              > "$out/$v/config.toml"
            cfg=$out/$v/config.toml
            args=(--ehs2-cache "$tables/v2-f128-t128-r128.postcard") ;;
    *) echo "unknown variant $v"; exit 2 ;;
  esac
  [[ -z ${SNAPSHOTS:-} ]] || solve_args+=(--fixed-snapshots "$SNAPSHOTS")
  start=$(date +%s)
  postflop=${POSTFLOP:-$out/$v/postflop.bin}
  if [[ -z ${POSTFLOP:-} ]]; then
    if [[ -n ${STATE_DIR:-} ]]; then
      mkdir -p "$STATE_DIR" || exit 1
      solve_args+=(--state "$STATE_DIR/$v.fixed.state" --state-every 300)
    fi
    target/release/examples/trunk_solve${EXE_SUFFIX-.exe} --config "$cfg" --leaf-model l1 "${args[@]}" "${shared_args[@]}" \
      "${solve_args[@]}" --tables-dir "${TABLES_DIR:-../_goal/gtowtree/tables}" --fixed-profile "$profile" --fixed-lines "$lines" \
      --iterations "${iters_override:-$iters}" --output-postflop "$postflop" --output "$out/$v/solve.json" \
      > "$out/$v/solve.log" 2>&1 || exit 1
  fi
  if [[ -z ${NODIAG:-} ]]; then
    if [[ -n ${REBIND:-} ]]; then
      [[ -n ${POSTFLOP:-} && $cfg == */b4s-gtow-raise5.toml ]] || { echo "REBIND needs POSTFLOP and the raise5 config"; exit 2; }
      if [[ ! -f $postflop.v3 ]]; then
        target/release/examples/l1_leaf_diag${EXE_SUFFIX-.exe} --config "$cfg" "${args[@]}" "${shared_args[@]}" \
          --tables-dir "${TABLES_DIR:-../_goal/gtowtree/tables}" --postflop "$postflop" --rebind-postflop "$postflop.v3" || exit 1
      fi
      postflop=$postflop.v3
    fi
    diag_args=(--boards "${DIAG_BOARDS:-2048}")
    [[ -z ${DIAG_FLOPS:-} ]] || diag_args+=(--flops "$DIAG_FLOPS")
    [[ -z ${DIAG_SEED:-} ]] || diag_args+=(--seed "$DIAG_SEED")
    [[ -z ${DIAG_RIVERS:-} ]] || diag_args+=(--rivers "$DIAG_RIVERS")
    [[ -z ${DIAG_DUMP:-} ]] || diag_args+=(--dump "$DIAG_DUMP")
    i=0
    running=0
    IFS=';' read -ra ls <<< "$lines"
    for line in "${ls[@]}"; do
      i=$((i + 1))
      if (( running >= ${DIAG_JOBS:-1} )); then
        wait -n || exit 1
        running=$((running - 1))
      fi
      ( target/release/examples/l1_leaf_diag${EXE_SUFFIX-.exe} --config "$cfg" "${args[@]}" "${shared_args[@]}" \
          --tables-dir "${TABLES_DIR:-../_goal/gtowtree/tables}" \
          --profile "$profile" --postflop "$postflop" --line "$line" "${diag_args[@]}" > "$out/$v/diag$i.txt" 2>&1 ) &
      running=$((running + 1))
    done
    while (( running > 0 )); do
      wait -n || exit 1
      running=$((running - 1))
    done
  fi
  echo "$v $(( $(date +%s) - start ))s $(grep -h 'L1:' "$out/$v/solve.log" 2>/dev/null)"
done
