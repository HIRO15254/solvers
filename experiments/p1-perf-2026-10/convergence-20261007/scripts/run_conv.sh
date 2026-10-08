#!/bin/bash
# T4 session B driver: convergence to 0.1% pot per schedule (new build), then CLI A/B after T3.
cd ~
until grep -q SETUP_DONE ~/setup.log; do sleep 10; done
source ~/.cargo/env
mkdir -p ~/results/conv
log() { echo "$(date -u +%T) $*" >> ~/results/progress.txt; }
S=~/new/target/release/solvers
conv() { # name
  rm -rf /tmp/conv_$1
  /usr/bin/time -v $S solve ~/conv/$1.toml --out /tmp/conv_$1 --threads 32 > ~/results/conv/$1.out 2> ~/results/conv/$1.err
  cp /tmp/conv_$1/progress.jsonl ~/results/conv/$1.progress.jsonl
  rm -rf /tmp/conv_$1
  log "conv $1"
}
log start
for s in dcfr hs-dcfr linear-cfr cfr-plus dcfr-i16; do conv turn_$s; done
for s in dcfr hs-dcfr linear-cfr cfr-plus dcfr-i16; do conv flop1_$s; done
bash ~/cli_ab.sh flop1 flop_srp1_cli.toml 16; log cli16
bash ~/cli_ab.sh flop1_32 flop_srp1_cli.toml 32; log cli32
conv gtowb_dcfr
conv gtowb_dcfr-i16
conv gtowb_hs-dcfr
echo ALL_DONE >> ~/results/progress.txt
