#!/bin/bash
# Round 3: repeat the c_gtowb f32 32-thread solve (single samples drifted in rounds 1-2) and the
# c_flop1 f32 32-thread bench, alternating old and s5 three times. Inputs as in setup.sh.
set -e
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config git python3 time >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
V="old s5"
mkdir -p ~/cfg ~/results/accept ~/work
for s in $V; do mkdir -p ~/$s && tar -xf ~/src-$s.tar -C ~/$s; done
tar -xzf ~/cfg-accept.tgz -C ~/cfg
for s in $V; do
  (cd ~/$s && cargo build --release -p cli --bin solvers 2>&1 | tail -1 \
    && cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) >> ~/results/build.log 2>&1
done
lscpu | head -20 > ~/results/lscpu.txt; free -g >> ~/results/lscpu.txt
echo "$(date -u +%T) BUILT" >> ~/results/progress.txt
set +e
R=~/results/accept
for rep in 1 2 3; do
  for s in $V; do
    ~/$s/target/release/examples/p1_bench ~/cfg/c_flop1.toml --threads 32 --warmup 3 --iters 10 --evals 2 \
      --storage f32 --json $R/bench_c_flop1_f32_t32_${s}_$rep.json > /dev/null 2> $R/bench_c_flop1_f32_t32_${s}_$rep.err
  done
  V=$(echo $V | awk '{print $2, $1}')
done
echo "$(date -u +%T) bench done" >> ~/results/progress.txt
c=~/work/gtowb.toml
sed -e '/^max_iterations/d' -e '/^check_every/d' -e 's/^target = .*/target = "0.1%pot"/' ~/cfg/c_gtowb.toml > $c
grep -q '^\[run\]' $c && sed -i 's/^\[run\]/[run]\nfinal_checkpoint = false/' $c || printf '\n[run]\nfinal_checkpoint = false\n' >> $c
V="old s5"
for rep in 1 2 3; do
  for s in $V; do
    d=~/work/gtowb_$s; rm -rf $d
    /usr/bin/time -v ~/$s/target/release/solvers solve $c --out $d --threads 32 > $R/solve_gtowb_f32_t32_${s}_$rep.log 2> $R/solve_gtowb_f32_t32_${s}_$rep.time
    cp $d/progress.jsonl $R/solve_gtowb_f32_t32_${s}_$rep.progress.jsonl; cp $d/run.json $R/solve_gtowb_f32_t32_${s}_$rep.run.json
    rm -rf $d
    echo "$(date -u +%T) solve gtowb $s $rep" >> ~/results/progress.txt
  done
  V=$(echo $V | awk '{print $2, $1}')
done
echo "$(date -u +%T) ALL_DONE" >> ~/results/progress.txt
