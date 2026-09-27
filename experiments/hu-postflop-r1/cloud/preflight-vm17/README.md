# VM17 pricing before reservation

Four unauthenticated official public GETs refresh normal compute, Spot compute, disk and
network prices. Captured HTML excerpts retain their original bytes, source URLs, retrieval
time and full-response hash. The full HTTP response bodies are not retained.

[cost-proposal.json](cost-proposal.json) checks all27 retained excerpts and records the
exact rational estimate:1.9922283333 USD including the original1 USD uncertainty reserve.
The35-minute original STOP is timed from the creation request; the120-second pricing slack
never extends execution.40 GiB disk24h, Spot IPv4 and all outbound512 MiB are included.
No credits, free tiers or Spot discounts are assumed in the conservative compute amount.
This is a pre-launch estimate, not a guaranteed invoice ceiling or a cloud reservation.

VM/bootstrap/build/solve operations are not performed by these pricing tools. Current
resource inventory and reservation validation are recorded separately under ../vm17/.
