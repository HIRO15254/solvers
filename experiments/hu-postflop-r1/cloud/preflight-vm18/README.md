# VM18 price proposal

The first restricted-network attempt failed; its original receipt is preserved
as `pricing-sources-attempt01.json`. The subsequent public GETs acquired the
regular compute, disk, network and Spot tables. `cost-proposal.py --check`
verifies27 byte-pinned excerpts and reproduces the exact rational arithmetic.

One60-minute VM, plus120 seconds of pricing slack, is charged throughout at
the larger regular$1.15/hour ceiling even though build and recovery use2CPU.
With40GiB disk for24h,0.5GiB egress at$0.30/GiB, Spot IPv4, and$1 uncertainty,
the modeled envelope is$2.472436667. This prices a finite resource plan; it is
not a bill, guaranteed cost ceiling, or permission to expand the$40 budget.

The captured budget snapshot precedes the later usage reconciliation. Reservation
must use the live shared ledger and verify the reconciled receipt separately.
