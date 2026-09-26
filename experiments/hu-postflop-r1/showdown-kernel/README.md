# Prepared terminal evaluation comparison

The candidate is adopted on the evidence in [the measured report](report.jp.md).
All 32 old/new executions matched exactly. Median solver time fell 28.27% on
River, 25.37% on Turn, 10.51% on Flop and 29.70% on narrow River. These are
single-worker results on one four-vCPU Spot VM, not renewed 32-worker results.
The [portable verification](verification.json) also checks the source/build
chain, 933 workspace tests and four explicitly selected release tests.

The existing showdown algorithm already precomputes hand strengths once per
unique completed board, sorts the initial-support union by `(rank, global combo)`,
and evaluates changing opponent reach by a linear equal-rank sweep. It uses
total and per-card weights to remove colliding hands, with a same-combo add-back
for ties and total compatible reach. Rank computation and sorting are outside
the repeated evaluation path. For fixed 52-card deck size, work is linear in
the retained support union, rather than the product of both range sizes.

This candidate removes constant overhead: repeated `combo_cards` inverse
searches and two temporary 1,326-element f32 arrays in each compact terminal
evaluation. A prepared eight-byte entry stores a u16 rank, two u8 card numbers
and two u16 seat-local indices (absent sentinel where applicable). It replaces
the historical eight-byte `(HandRank, u32)` entry. Entries keep their original
order and all floating-point additions keep their original grouping/order.
Dense diagnostic games use identity local indices. Reporting equity keeps its
global 1,326-combo API and historical kernels.

The production tree's stored hand counts and SOL/CKPT formats do not change.
The 10,608 bytes removed from source-level stack arrays are not a claim about
observed process RSS. Preparing each table requires a transient allocation;
the measurement retains construction and initialization time separately from
solver time. The existing tiny-weight subtraction/cancellation limitation
described in `../range-scaling/source06/normalizer-edge-audit.md` is unchanged.

Six direct kernel tests compare both seats with the old global kernels bit for
bit and with an independent quadratic pairwise reference. Win, tie and loss
utilities are checked separately, alongside nonzero tie utility, overlapping
asymmetric support, full support, zero/sparse reach and later public-card masks.
Existing compact/dense tests check layout/index equivalence; the old/new binary
comparison and direct global-kernel tests check the changed evaluation path.

`protocol.json`, `run.py`, `verify.py` define the separate, prospectively fixed comparison.
The original range-scaling and action-scaling protocols remain unchanged.
Use exact old/new source manifests and fresh native builds on one small Linux
Spot VM; do not merge timings with the earlier 32-vCPU hosts. This campaign is
an internal performance screen, not external-reference or R1 acceptance.

The VM09 reservation in `../cloud/budget.json` holds USD 3 within the user's
cumulative USD 40 allowance (USD 29 held, USD 11 unreserved; invoice unknown).
The VM is `e2-standard-4`, 4 vCPU / 16 GiB with a 40-GiB balanced disk. Its
absolute cloud STOP deadline is 2026-09-26 18:43:07 UTC; validation's earlier
deadline is 18:25 UTC. Disk retention is bounded by 24 hours from launch and
the VM/disk must be deleted after evidence recovery. Build and measurement
use 12-GiB outer cgroups, zero swap and no concurrent experiment.

Rates checked on 2026-09-26: [E2 on-demand](https://cloud.google.com/products/compute/pricing/general-purpose)
USD 0.13402284/h, conservatively rounded to 0.14;
[balanced disk](https://cloud.google.com/compute/disks-image-pricing)
USD 0.000137/GiB/h rounded;
[Spot external IPv4](https://cloud.google.com/vpc/network-pricing)
USD 0.0025/h. Using on-demand compute as a reservation estimate, 4.02 hours of
compute/IP, 40 GiB for 24 hours, 1 GiB transfer reserved at USD 0.30, and USD 1
margin total USD 2.00437, covered by the USD 3 reservation. This is not a Spot
price quotation or an observed bill.
