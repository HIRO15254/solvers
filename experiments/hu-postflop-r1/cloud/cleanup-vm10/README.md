# VM10 cleanup evidence

`solvers-r1-20260926-10` (instance ID `9019841781761201633`) and its
40 GiB auto-delete boot disk (ID `2211921492040111585`) were explicitly
deleted after the immutable source, validation, comparison and diagnostic
evidence had been recovered and verified locally. No experiment service was
running at the pre-delete check.

The Compute operation completed successfully at **2026-09-26T17:45:17.305Z**.
[reconciliation.json](reconciliation.json) records the operation, raw-evidence
hashes and recovered proof hashes. Subsequent instance/disk queries for the
exact name and reserved-address query for the former external IPv4 all returned
empty lists. These are scoped checks, not an inventory claim about unrelated
resources. The empty-resource filter warnings are retained in stderr.
The seven stderr files were renamed from `.txt` to `.log` without changing
their bytes, using the repository's existing raw-log whitespace policy.

[r1-vm10-lifecycle.txt](r1-vm10-lifecycle.txt) retains the service journals,
including failed preparation/validation attempts and subsequent successful
candidate04 runs. The final results are described in
[the experiment report](../../exact-mass/report04.jp.md).

Actual billing remains unavailable. The $3 VM10 reservation is still held;
the cumulative conservative reservation is **$32 of the authorized $40**.
Deletion does not release estimated spend as though it were a reconciled bill.
