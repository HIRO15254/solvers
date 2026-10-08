#!/bin/bash
# Template-tree measurements: time/iterations/peak RSS to the target at 32 and 16 threads,
# plus GW Single Size replica node exports. Runs on a GCP c2d-highcpu-32 Spot VM (Ubuntu 24.04).
set -e
sudo apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config git python3 time >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.0 --profile minimal >/dev/null 2>&1
source ~/.cargo/env
mkdir -p ~/src ~/cfg ~/results && tar -xf ~/src.tar -C ~/src && tar -xzf ~/cfg.tgz -C ~/cfg
(cd ~/src && cargo build --release -p cli --bin solvers 2>&1 | tail -1) > ~/results/build.log 2>&1
lscpu | head -20 > ~/results/lscpu.txt; free -g >> ~/results/lscpu.txt
echo "$(date -u +%T) BUILT" >> ~/results/progress.txt
set +e
S=~/src/target/release/solvers
run() {  # name threads
  local d=~/work/$1_t$2; rm -rf $d; mkdir -p ~/work
  /usr/bin/time -v $S solve ~/cfg/$1.toml --out $d --threads $2 > ~/results/$1_t$2.log 2> ~/results/$1_t$2.time
  cp $d/progress.jsonl ~/results/$1_t$2.progress.jsonl; cp $d/run.json ~/results/$1_t$2.run.json
  echo "$(date -u +%T) $1_t$2 exit=$?" >> ~/results/progress.txt
}
for t in 32 16; do
  for n in t3 t2 t1one srp_single srp_general gw_single t1s flop_srp; do
    run $n $t
    if [ $n = gw_single ] && [ $t = 32 ]; then
      for node in "" "x" "xx[3c]" "xx[3c]x" "xx[3c]xx[8d]"; do
        tag=$(echo "root$node" | tr -c 'A-Za-z0-9\n' '_')
        $S export ~/work/gw_single_t32/solution.sol actions --node "$node" --format json > ~/results/gw_actions_$tag.json 2>/dev/null
        $S export ~/work/gw_single_t32/solution.sol strategy --node "$node" --format csv > ~/results/gw_strategy_$tag.csv 2>/dev/null
      done
      $S export ~/work/gw_single_t32/solution.sol summary --format json > ~/results/gw_summary.json 2>/dev/null
      $S export ~/work/gw_single_t32/solution.sol range --node "x" --format csv > ~/results/gw_range_x.csv 2>/dev/null
    fi
    rm -f ~/work/${n}_t$t/solution.sol ~/work/${n}_t$t/*.ckpt
  done
done
echo "$(date -u +%T) ALL_DONE" >> ~/results/progress.txt
