# Bounded Multiway public-tree census

`mw_tree_census` lowers a production Multiway Preflop v1 config and walks its
deterministic public betting tree without allocating the policy arena, loading
EHS2 tables, sampling cards, or running MCCFR. It reports decision nodes,
public action edges, policy columns, and policy slots for each
`(street, bucket_active_opponents)` group.

The research-only `--buckets FLOP,TURN,RIVER` argument changes only the arena
estimate. It is not a v1 config field or a solver override. Preflop always uses
169 classes. Likewise, `--postflop-cap` replaces the lowered flop/turn/river
aggression caps only inside this census; the report records all effective caps.

```text
cargo run --release -p cli --example mw_tree_census -- \
  --config examples/bench_multiway/6max_100bb_nl50_partial_reference.toml \
  --buckets 128,64,32 \
  --postflop-cap 2 \
  --max-nodes 10000000 \
  --max-seconds 60 \
  --arena-limit-bytes 171798691840
```

Run the same tree with `256,64,32`, `128,64,32`, and `64,32,16`. A completed
report has concrete `fitsArenaLimit` and `fitsU32Columns` values. If either
bound fires, `complete` is false, `stopReason` identifies the bound, and all
counts describe only the observed deterministic DFS prefix. Partial counts and
bytes are lower bounds: they can prove non-fit once a limit is exceeded, but an
otherwise unproven fit stays null.

`estimatedArenaPayloadBytes` uses the dense arena's current payload formula:
two `f32` arrays per policy slot, one touched bit per column, 24 bytes per
decision-node table row, and two trailing `u64` sentinels. The public tree,
allocator slack, EHS2 tables, worker scratch, evaluation, and artifact staging
remain outside this number. Leave operating-system and non-arena headroom when
comparing it with machine RAM.
