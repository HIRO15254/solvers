# Historical abstraction experiments (2026-07-23–24)

Everything in this directory is retained as research evidence. The TOML files
exercise retired rollout and/or full-recall semantics and are not production
inputs. A production binary rejects them with `MWP001`, `MWP002`, or `MWP003`;
do not copy their abstraction sections into a canonical v1 config.

Reproduce legacy solve/evaluate runs only with the isolated research binary:

```sh
CARGO_TARGET_DIR=target/research-release \
  cargo build --release -p cli --features research --bin solvers
```

The action-tree CSV uses a 6 GiB dense-policy-arena estimate. It is not a
6 GiB process-RSS result: the public tree, EHS²/rollout data, worker scratch,
evaluation, checkpoint staging, and allocator overhead are outside that
number. Any 8 GiB process boundary must be enforced separately with a
cgroup/container or external RSS watchdog.

The current production decision and measured removal rationale are documented
in
[`docs/validation/multiway-abstraction-optimization-2026-07-25.md`](../../docs/validation/multiway-abstraction-optimization-2026-07-25.md).
