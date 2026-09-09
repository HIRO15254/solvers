# Streaming `.mwsol` export proof (2026-09-09)

This record is based on the saved guest export result and local recovery
verification:

- `runs/multiway-convergence-round5-20260909/cloud-export-fix-state4/verified/export.stdout.json`
- `runs/multiway-convergence-round5-20260909/cloud-export-fix-state4/recovery-verification.json`

The repaired streaming writer completed a state4 K256 export with **12,297,431
strategy blocks** (not bytes). The recovered artifact is 2,171,445,586 bytes,
SHA-256 `ce46cb96469eaa20916c3849db5d7abe4268f5673ff20d2c69bf8f9f5940cf6f`,
and BLAKE3 `5f8f155c140656e23918ea0564fe63a027cba008be0f3ae735b0ff0c6d2f7639`.
The output used u16 storage, solver state version 4, and 262,144 sweeps.

Guest phase timings were startup 144.681s, restore 156.953s, snapshot
9.017s, solution construction 74.117s, write 43.280s, and verification
15.967s. These phases are recorded separately; write time is not total export
time.

The former 10,000,000-block limit was replaced by a 2 GiB index bound whose
derived maximum is 23,598,721 strategy blocks. The verification reopened
metadata and the first and last strategy pages. It did **not** read every page,
so this is not an all-pages integrity proof and makes no convergence or
strategy-quality claim.

The original checkpoint SHA-256 was unchanged:
`a54a47dabf9c3677a0fdffd8d7e5f65c9ac3134be4c57c73af9999c0cb3920f5`.
The VM was stopped after export and recovery. RSS values were sampled during
the run and are not a process peak proof.
