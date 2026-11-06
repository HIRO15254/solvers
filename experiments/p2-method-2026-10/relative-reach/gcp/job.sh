#!/bin/bash
# Relative-bucket reach variants on the GTO Wizard-menu B4 tree (~/b4s-gtow.toml from b4s-8h/gtow_tree.py): each
# token of ~/variants.txt (gcp/variants.txt) is solved up to ~/seconds.txt (gcp/seconds.txt) seconds with the 8x2
# range-relative buckets (evaluation and class-profile snapshot every 1000 iterations).
# 全体のsolveは最初のt >= MIN_ITERでloss(t-WINDOW)-loss(t) < DELTA bb/100なら停止する。
# ~/plateau.txtの「MIN_ITER WINDOW DELTA」で変更可（既定2000 2000 0.02）。seconds.txtはhard cap。
# Runner extras (uploaded flat to ~), in addition to trees, variants.txt, seconds.txt and any group files:
# b4s-8h/compare.py, b4s-8h/references/gtowizard-simple-100bb-preflop-tree-2026-10-09.json.
# No comparison helper module is needed (python3 stdlib only). plateau.txt is optional.
# iterations.txt is optional (fixed iteration count).
# eval-overrides.txt optionally lists uploaded override JSONs to evaluate after each solve (including resumed runs).
# drop_final_postflop removes final Postflop only after all requested evaluations succeed; hashes remain in results.
# Uploaded text inputs accept LF or CRLF. JOB_FAILED is terminal for both cloud runners.
# Optional loss-cap.txt: MIN_ITER MAX_LOSS bb/100, stop a clearly worse experimental solve.
# Missing/broken comparison inputs disable the watcher, log to results/progress.txt, and retain the hard cap.
# Local watcher: HOME=<flat inputs dir> bash job.sh --watch-plateau VARIANT SNAP_DIR STOP_FILE DONE_FILE
# (touch DONE_FILE after the solver exits; watcher interval defaults to 2 seconds).
# A token is base or '+'-joined parts: pM = --l1-relative-prior M, avg = --l1-relative-reach average,
# gK = --l1-flop-groups ~/kK-groups.txt (gtow-fixed/subsets), gfK = the same on the flop only,
# h[mN][d][pN][f][q][b][g] = --l1-hand made:N, draws, potential:N, flop-draws, pairs, backdoor, gutshot
# (hdq = draws,pairs; hdqbg = draws,pairs,backdoor,gutshot),
# hc[N] = --l1-hand category[:N],
# roKINDS = --l1-runout (letters f flush, p pair, s straight, o over; rofp = flush,pair), relNxL = --l1-relative NxL
# instead of 8x2. lsprMAXrNxLgK = --l1-low-spr MAX:NxL:~/kK-groups.txt (only low-SPR leaves).
# rcN = persistent coarse flop-strength recall via --l1-refine recall:N;
# hdf = --l1-hand draws,flop-draws (binary flop flush draw/OESD recall).
# qpLAMBDA = --l1-qre-prior LAMBDA (mass-preserving logit prior; requires pM, disables train regression).
# subK = --l1-flop-subset ~/kK.txt (B01: train on the weighted representatives only; pair with gK, e.g.
# g49+sub49-fit uploads k49-groups.txt and k49-fit.txt).
# ssprX = --l1-flop-subset-max-spr X (B02: subK only at flop SPR <= X; higher SPR uses standard boards).
# Example: traise5+p10+g49+hd+sub49-fit+sspr2.5.
# sdN = --l1-seed N --seed N (E2 replicate).
# ow = --l1-regret-weight own (postflop regrets weighted by own reach within the bucket).
# own = --l1-leaf-flop-groups own:~/leaf.profile.json (each L1 leaf's own 49 flop groups from the uploaded class
# profile, e.g. the 1000-iteration snapshot of the same solve without own; P2D12 stage 2).
# Idempotent like b4s-8h/gcp/job_postflop.sh: after a preemption the finished setup and variants are skipped and
# the running variant resumes from its state file.
set -u

watch_plateau() {
  "${PYTHON:-python3}" - "$@" <<'PY'
import importlib.util
import json
import math
import os
from pathlib import Path
import sys
import time

variant, snapshot_dir, stop_file, done_file = sys.argv[1:]
home = Path(os.environ["HOME"])
results = home / "results"
results.mkdir(parents=True, exist_ok=True)

def log(message):
    with (results / "progress.txt").open("a", encoding="utf-8") as out:
        out.write(f"{time.strftime('%H:%M:%S', time.gmtime())} PLATEAU {variant} {message}\n")

try:
    spec = importlib.util.spec_from_file_location("plateau_compare", home / "compare.py")
    metric = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(metric)
    reference = json.loads((home / "gtowizard-simple-100bb-preflop-tree-2026-10-09.json").read_text(encoding="utf-8"))
    minimum, window, delta = 2000, 2000, 0.02
    if (home / "plateau.txt").exists():
        a, b, c = (home / "plateau.txt").read_text(encoding="utf-8").replace("\r", "").split()
        minimum, window, delta = int(a), int(b), float(c)
    if minimum < 0 or window <= 0 or not math.isfinite(delta) or delta < 0:
        raise ValueError("plateau.txt requires MIN_ITER >= 0, WINDOW > 0 and finite DELTA >= 0")
    loss_cap = None
    if (home / "loss-cap.txt").exists():
        a, b = (home / "loss-cap.txt").read_text(encoding="utf-8").replace("\r", "").split()
        loss_cap = int(a), float(b)
        if loss_cap[0] < 0 or not math.isfinite(loss_cap[1]) or loss_cap[1] < 0:
            raise ValueError("loss-cap.txt requires MIN_ITER >= 0 and finite MAX_LOSS >= 0")
except Exception as error:
    log(f"DISABLED {error}; solving to hard cap")
    sys.exit(0)

losses = {}
with (results / f"{variant}.plateau.txt").open("w", encoding="utf-8") as out:
    while not Path(done_file).exists():
        snapshots = sorted(Path(snapshot_dir).glob("it*.profile.json"),
                           key=lambda p: int(p.name[2:].split(".")[0]))
        for snapshot in snapshots:
            iteration = int(snapshot.name[2:].split(".")[0])
            if iteration in losses:
                continue
            try:
                measured = metric.compare(reference, metric.load(snapshot))
                if not any(row["matched"] for row in measured):
                    raise ValueError(f"{snapshot.name}: no reference nodes matched")
                loss = metric.summary(measured)["all"]["loss_bb100"]
                if not math.isfinite(loss):
                    raise ValueError(f"{snapshot.name}: nonfinite EV loss")
            except json.JSONDecodeError:
                # The solver may still be writing the newly visible profile.
                break
            except Exception as error:
                log(f"DISABLED {error}; solving to hard cap")
                sys.exit(0)
            losses[iteration] = loss
            out.write(f"{iteration} {loss:.12g}\n")
            out.flush()
            if loss_cap is not None and iteration >= loss_cap[0] and loss > loss_cap[1]:
                Path(stop_file).touch()
                log(f"STOP_LOSS_CAP iteration={iteration} loss={loss:.12g} rule={loss_cap}")
                sys.exit(0)
            previous = losses.get(iteration - window)
            if iteration >= minimum and previous is not None and previous - loss < delta:
                Path(stop_file).touch()
                log(f"STOP iteration={iteration} loss={loss:.12g} improvement={previous-loss:.12g} "
                    f"rule={minimum} {window} {delta}")
                sys.exit(0)
        time.sleep(float(os.environ.get("PLATEAU_POLL_SECONDS", "2")))
PY
}

if [[ ${1:-} == --watch-plateau ]]; then
  shift
  watch_plateau "$@"
  exit 0
fi
R=~/results
mkdir -p $R ~/src ~/ehs2 ~/state ~/final
echo "$(date -u +%T) JOB_START" >> $R/progress.txt
if ! grep -q SETUP_DONE $R/progress.txt; then
  sudo apt-get update -qq >/dev/null
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config python3 time >/dev/null
  curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
  source ~/.cargo/env
  tar -xf ~/src.tar -C ~/src
  cd ~/src
  cargo build --release -p mw-preflop --example trunk_solve > $R/build.log 2>&1
  rustc --version >> $R/build.log
  sha256sum target/release/examples/trunk_solve >> $R/build.log
  lscpu > $R/lscpu.txt; free -g >> $R/lscpu.txt
  S=target/release/examples/trunk_solve
  $S --config examples/bench/hu_20bb_postflop.toml --leaf-model l1 --ehs2-cache ~/ehs2/v2-f32-t32-r32.postcard \
    --iterations 1 --eval-every 0 > $R/tables32.log 2>&1
  code=$?
  if [ "$code" != 0 ]; then
    echo "$(date -u +%T) JOB_FAILED tables32 $code" >> $R/progress.txt
    exit "$code"
  fi
  sed -E "/^\[solver\.abstraction\.buckets\]/,/^\[/ s/^(flop|turn|river) = 32$/\1 = 128/" \
    examples/bench/hu_20bb_postflop.toml > /tmp/b6-128.toml
  $S --config /tmp/b6-128.toml --leaf-model l1 --ehs2-cache ~/ehs2/v2-f128-t128-r128.postcard \
    --iterations 1 --eval-every 0 > $R/tables128.log 2>&1
  code=$?
  if [ "$code" != 0 ]; then
    echo "$(date -u +%T) JOB_FAILED tables128 $code" >> $R/progress.txt
    exit "$code"
  fi
  sha256sum ~/ehs2/* >> $R/tables.log
  echo "$(date -u +%T) SETUP_DONE" >> $R/progress.txt
fi
source ~/.cargo/env
cd ~/src
seconds=$(tr -d '\r' < ~/seconds.txt)
# Optional ~/iterations.txt: stop at exactly N iterations (positive integer); seconds.txt stays the hard cap.
iterations=1000000
if [ -f ~/iterations.txt ]; then
  iterations=$(tr -d '\r' < ~/iterations.txt)
  [[ $iterations =~ ^[1-9][0-9]*$ ]] || { echo "$(date -u +%T) JOB_FAILED iterations.txt" >> $R/progress.txt; exit 2; }
fi
# ~/config.txt names another uploaded tree (lowspr-menu: b4s-gtow-lowspr.toml); a part t<NAME> solves
# ~/b4s-gtow-<NAME>.toml instead (geo-menu).
default_config=$(tr -d '\r' 2>/dev/null < ~/config.txt || echo b4s-gtow.toml)
for v in $(tr -d '\r' < ~/variants.txt); do
  done_before=false
  grep -q "END $v 0" $R/progress.txt && done_before=true
  args=()
  relative=8x2
  config=$default_config
  for part in ${v//+/ }; do
    case $part in
      base) ;;
      avg) args+=(--l1-relative-reach average) ;;
      ow) args+=(--l1-regret-weight own) ;;
      own) args+=(--l1-leaf-flop-groups "own:$HOME/leaf.profile.json") ;;
      p*) args+=(--l1-relative-prior "${part#p}") ;;
      qp*) args+=(--l1-qre-prior "${part#qp}") ;;
      t*) config=b4s-gtow-${part#t}.toml ;;
      lspr*) [[ $part =~ ^lspr([0-9]+([.][0-9]+)?)r([0-9]+x[0-9]+)g([0-9]+)$ ]] || { echo "unknown part $part" >> $R/progress.txt; exit 2; }
             args+=(--l1-low-spr "${BASH_REMATCH[1]}:${BASH_REMATCH[3]}:$HOME/k${BASH_REMATCH[4]}-groups.txt") ;;
      h*) if [[ ${part#h} =~ ^c([0-9]*)$ ]]; then
            hs=category${BASH_REMATCH[1]:+:${BASH_REMATCH[1]}}
          else
            [[ ${part#h} =~ ^(m([0-9]+))?(d)?(p([0-9]+))?(f)?(q)?(b)?(g)?$ ]] || { echo "unknown part $part" >> $R/progress.txt; exit 2; }
            hs=; [[ -n ${BASH_REMATCH[2]} ]] && hs=made:${BASH_REMATCH[2]}
            [[ -n ${BASH_REMATCH[3]} ]] && hs=${hs:+$hs,}draws
            [[ -n ${BASH_REMATCH[5]} ]] && hs=${hs:+$hs,}potential:${BASH_REMATCH[5]}
            [[ -n ${BASH_REMATCH[6]} ]] && hs=${hs:+$hs,}flop-draws
            [[ -n ${BASH_REMATCH[7]} ]] && hs=${hs:+$hs,}pairs
            [[ -n ${BASH_REMATCH[8]} ]] && hs=${hs:+$hs,}backdoor
            [[ -n ${BASH_REMATCH[9]} ]] && hs=${hs:+$hs,}gutshot
          fi
          args+=(--l1-hand "$hs") ;;
      rel*) relative=${part#rel} ;;
      rc*) args+=(--l1-refine "recall:${part#rc}") ;;
      gf*) args+=(--l1-flop-groups "$HOME/k${part#gf}-groups.txt:flop") ;;
      g*) args+=(--l1-flop-groups "$HOME/k${part#g}-groups.txt") ;;
      sd*) args+=(--l1-seed "${part#sd}" --seed "${part#sd}") ;;
      sub*) args+=(--l1-flop-subset "$HOME/k${part#sub}.txt") ;;
      sspr*) args+=(--l1-flop-subset-max-spr "${part#sspr}") ;;
      ro*) rs=; spec=${part#ro}
           for ((k = 0; k < ${#spec}; k++)); do
             case ${spec:k:1} in f) rs+=flush, ;; p) rs+=pair, ;; s) rs+=straight, ;; o) rs+=over, ;; esac
           done
           args+=(--l1-runout "${rs%,}") ;;
      *) echo "unknown part $part of $v" >> $R/progress.txt; exit 2 ;;
    esac
  done
  if ! $done_before; then
  mkdir -p ~/snap-$v
  echo "$(date -u +%T) START $v" >> $R/progress.txt
  stop_file=$HOME/state/$v.stop
  done_file=$HOME/state/$v.watcher-done
  rm -f "$stop_file" "$done_file"
  watch_plateau "$v" "$HOME/snap-$v" "$stop_file" "$done_file" >> "$R/$v.log" 2>&1 &
  watcher=$!
  /usr/bin/time -v -o $R/$v.$(date +%s).time target/release/examples/trunk_solve \
    --config ~/$config --leaf-model l1 \
    --ehs2-cache ~/ehs2/v2-f32-t32-r32.postcard --l1-relative $relative:$HOME/ehs2/v2-f128-t128-r128.postcard "${args[@]}" \
    --solver-k4-samples 256 --solver-k4-min-samples 16 --l1-eval-boards 1024 \
    --iterations $iterations --eval-every 1000 --print-every 100 --max-seconds $seconds --stop-file "$stop_file" \
    --state ~/state/$v.state --state-every 300 --snapshot-dir ~/snap-$v \
    --output $R/$v.json --output-profile ~/final/$v.profile.json --output-postflop ~/final/$v.postflop.bin \
    >> $R/$v.log 2>&1
  code=$?
  touch "$done_file"
  wait "$watcher" || echo "$(date -u +%T) PLATEAU $v DISABLED watcher failed; used hard cap" >> "$R/progress.txt"
  echo "$(date -u +%T) END $v $code" >> $R/progress.txt
  if [ "$code" != 0 ]; then
    echo "$(date -u +%T) JOB_FAILED $v $code" >> $R/progress.txt
    exit "$code"
  fi
  fi
  evaluations_complete=true
  if [ -f ~/eval-overrides.txt ]; then
    for file in $(tr -d '\r' < ~/eval-overrides.txt); do
      base=$(basename "$file" .json)
      output=$R/$v.$base.eval.json
      if [ ! -f "$output" ]; then
        if target/release/examples/trunk_solve \
          --config ~/$config --leaf-model l1 --ehs2-cache ~/ehs2/v2-f32-t32-r32.postcard \
          --l1-relative $relative:$HOME/ehs2/v2-f128-t128-r128.postcard "${args[@]}" \
          --solver-k4-samples 256 --solver-k4-min-samples 16 --l1-eval-boards 1024 \
          --evaluate-profile ~/final/$v.profile.json --evaluate-postflop ~/final/$v.postflop.bin \
          --l1-eval-override "$HOME/$file" --output "$output.tmp" > "$R/$v.$base.eval.log" 2>&1 \
          && mv "$output.tmp" "$output"; then
          :
        else
          echo "$(date -u +%T) EVAL_FAILED $v $file" >> "$R/progress.txt"
          evaluations_complete=false
        fi
      fi
    done
  fi
  if [ -e ~/drop_final_postflop ] && $evaluations_complete && [ -f ~/final/$v.postflop.bin ]; then
    sha256sum ~/final/$v.postflop.bin >> "$R/postflop.sha256" && rm ~/final/$v.postflop.bin
  fi
  # The class profiles are what compare.py reads; with ~/drop_postflop_snapshots present only the final Postflop is kept.
  [ -e ~/drop_postflop_snapshots ] && rm -f ~/snap-$v/*.postflop.bin
done
(cd ~ && sha256sum snap-*/* final/* > $R/artifacts.sha256 && tar -czf ~/final.tgz snap-* final)
echo "$(date -u +%T) ALL_DONE" >> $R/progress.txt
