# Multiway state4 convergence evidence (2026-09-09)

This report is a source-backed diagnostic record for corrected solver state4.
The earlier state3 results are invalid accuracy evidence because the street-only
combo-bucket cache reused tables across different active-opponent counts. No
result here is an achievement claim, a full-GTOW match, exploitability bound, or
convergence certificate.

## Matched K32 pair

The K32 thread pair at 16,384 sweeps matched exactly between 8 and 24 solver
threads: state version 4, configuration fingerprint
`ce5341b88015273e9b9ba728589b622749dd45b1fcde99b5aaf4a089846424b5`, abstraction
fingerprint `1562cd5fc04d838fecfbc15ad25b382effbea9ea42417dbb76c74e546a19d313`,
and policy arena 2,671,933 nodes / 87,818,800 columns / 181,382,705 slots /
1,526,165,400 bytes. The exact per-seat and hand comparisons are recorded in
the JSON companion and were produced with `tools/multiway_reference_compare.py`.

The K32 comparison contains five RFI nodes and the ten-hand observed panel
(UTG AJo, KQo, ATs, 55; BTN K8o, Q8o, J7s, 54s; SB K8o, Q8o). The measurements
are conditional physical-world estimates and the hand panel is descriptive;
neither is a claim about the complete range.

Held-out fixed-candidate gains used 4,096 samples per seed and 20,000 deviator
traversals. Maximum CI95 upper gains were 1.2562893546 bb/hand (seed 101) and
1.3116176739 bb/hand (seed 202). These are finite candidate diagnostics, not a
best response or exploitability result.

## K256 comparison

The K256 run reached 65,536 sweeps with arena 2,671,933 nodes / 682,543,504
columns / 1,407,251,601 slots / 11,407,457,160 bytes. Its abstraction
fingerprint was `29e5f5f110b46a9fcdb2ea953d878604267dc2d0d9d0ca484982ed1107a41dad`.
Comparison against K32 is confounded by both the 4x sweep count (65,536 versus
16,384) and the changed bucket abstraction, so it is a descriptive sensitivity
check only. Its maximum CI95 upper gains were 0.6595787661 and 0.6178105872
bb/hand for seeds 101 and 202.

## Census and memory

The completed cap2 census has 9,262,677 decision nodes and a mixed 128/64/32
arena estimate of 7,280,620,448 bytes; the 409,317,488 columns fit u32. The
per-street totals are:

| street | decision nodes | columns | slots | public edges |
|---|---:|---:|---:|---:|
| preflop | 16,912 | 2,858,128 | 6,258,577 | 37,033 |
| flop | 405,033 | 51,844,224 | 115,515,264 | 902,463 |
| turn | 2,240,991 | 143,423,424 | 307,082,944 | 4,798,171 |
| river | 6,599,741 | 211,191,712 | 447,037,152 | 13,969,911 |

Using the core aggregate formula `8*policy_slots + ceil(policy_columns/64)*8
+ 24*decision_nodes + 16`, the mixed schedule estimates are 7,280,620,448
bytes for 128/64/32, 8,211,223,416 bytes for 256/64/32, and 3,776,675,632 bytes
for 64/32/16 after proportional bucket scaling. The companion JSON records the
derived schedule figures and the exact formula; per-group sums include their
own fixed overhead and therefore differ slightly from the core aggregate.
The 7.28 GB arena does not establish that 160 GB is necessary: process memory
and runtime working sets are additional. The next cloud shape is 8 vCPU / 62
GiB; local K32 batch8 and batch12 experiments are active.

## Sources and limits

- [Corrected K32 8-thread audit](../../runs/multiway-convergence-round4-20260909/cloud-state4-k32-pair/k32-t8/audit.stdout.json) — SHA-256 `bc47cd9357305798fe34b7b6996f53c74e0e052f66675ff4b02edbe99643f2e0`.
- [Corrected K32 24-thread audit](../../runs/multiway-convergence-round4-20260909/cloud-state4-k32-pair/k32-t24/audit.stdout.json) — SHA-256 `f188a813a8fd01548b5ba45800a8b26c7ca45ef4b6d438f72af712ddb1149e54`.
- [Corrected K256 audit](../../runs/multiway-convergence-round4-20260909/cloud-state4-k256/k256-t24/audit.stdout.json) — SHA-256 `33a38bdd3e400001db64428d1b367d9ab5fcf1edef611f9e8f76aceffd382338`.
- [Cap2 census](../../runs/gcp-convergence-20260909-control/research-state4/census-cap2.json) — SHA-256 `44bebf7e6a77be8e9996f3b9b2ea9f15490085f1eaa86a7b48f2eddd82eba3f8` (UTF-8 BOM).
- [GTOW preflop reference](gtowizard-preflop-2026-09-08.json) and [boundary hand panel](gtowizard-boundary-hands-2026-09-09.json).

The measured solver remains a partial tree with different abstraction and rake
conventions from GTOW. Rounded UI references, finite samples, and fixed
candidate deviations are retained as evidence for diagnosis rather than proof
of full strategic equivalence.
