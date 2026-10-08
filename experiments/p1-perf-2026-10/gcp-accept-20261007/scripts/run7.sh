#!/bin/bash
# VM4 tail 3: allocator A/B on d (prune+T6) p1_bench: glibc default / glibc tunables / mimalloc / jemalloc, plus syscall counts.
cd ~; source ~/.cargo/env
R=~/results/alloc; mkdir -p $R
log() { echo "$(date -u +%T) $*" >> ~/results/progress.txt; }
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq libmimalloc2.0 libjemalloc2 strace >/dev/null 2>&1
MI=$(dpkg -L libmimalloc2.0 | grep '\.so' | head -1); JE=$(dpkg -L libjemalloc2 | grep 'libjemalloc\.so' | head -1)
echo "MI=$MI JE=$JE" > $R/libs.txt
(cd ~/d && cargo build --release -p hu-postflop --example p1_bench 2>&1 | tail -1) > $R/build.log 2>&1
B=~/d/target/release/examples/p1_bench
TUN="glibc.malloc.trim_threshold=1099511627776:glibc.malloc.mmap_threshold=33554432:glibc.malloc.top_pad=67108864"
run() { # tag cfg storage iters
  local tag=$1 cfg=$2 st=$3 it=$4
  case $tag in
    base*) env $B ~/cfg/$cfg --storage $st --threads 32 --warmup 2 --iters $it --evals 1 > $R/$tag.json 2> $R/$tag.err ;;
    tun*) env GLIBC_TUNABLES=$TUN $B ~/cfg/$cfg --storage $st --threads 32 --warmup 2 --iters $it --evals 1 > $R/$tag.json 2> $R/$tag.err ;;
    mi*) env LD_PRELOAD=$MI $B ~/cfg/$cfg --storage $st --threads 32 --warmup 2 --iters $it --evals 1 > $R/$tag.json 2> $R/$tag.err ;;
    je*) env LD_PRELOAD=$JE $B ~/cfg/$cfg --storage $st --threads 32 --warmup 2 --iters $it --evals 1 > $R/$tag.json 2> $R/$tag.err ;;
  esac
}
for r in 1 2; do for v in base tun mi je; do run ${v}_flop1_f32_$r flop1_f32.toml f32 20; done; log "alloc flop1 f32 round $r"; done
for r in 1 2; do for v in base tun mi je; do run ${v}_flop1_mix_$r flop1_f32.toml i16-f32avg 20; done; log "alloc flop1 mix round $r"; done
strace -f -c -e trace=memory -o $R/strace_base.txt $B ~/cfg/flop1_f32.toml --storage f32 --threads 32 --warmup 0 --iters 5 --evals 0 > /dev/null 2>&1
log "alloc strace"
for v in base tun mi je; do run ${v}_turn_f32_1 turn_f32.toml f32 200; done; log "alloc turn"
for v in base mi tun je; do run ${v}_gtowb_f32_1 t_gtowb.toml f32 6; done; log "alloc gtowb"
echo ALLOC_DONE >> ~/results/progress.txt
