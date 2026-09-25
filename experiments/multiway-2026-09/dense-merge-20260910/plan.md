# Dense sweep rollback before scaling deep learning

Status: a concrete dense-merge error path is under regression and repair.
No new learning proposal or default is promoted. 2026-09-10.

## Finding and scope

The preceding counterfactual diagnostic pilot now covers all 169 own classes
at the fixed 3bet/4bet/5bet endpoints. Inspection of range-vector learning found
no leading-order expectation error in its two-sided opponent importance
correction: prefix importance weights descendant regrets, while returned
weighted values supply ancestors. Own strategy reach is correctly excluded
from regret updates. Full opponent enumeration would require probability
weights on both paths, and enumerating the first opponent at root would miss
the most relevant deep response nodes in five of six traversals. That candidate
needs a separate targeted design and cost argument.

Inspection did find a concrete prerequisite for larger runs: dense sweep merge
writes touched flags and earlier f32 slots before a later error can be returned.
Late numeric overflow, malformed later events or later counter validation can
therefore leave partial updates with stale progress counters. The sparse path
stages its writes, and the public driver promises complete transactional
sweeps. A bounded late-slot test must demonstrate the existing failure before
the repair. This is not a claim that the retained ordinary experiments failed.

## Intended repair and decisive checks

Validate all seat/sample IDs and all prospective progress counters before any
arena mutation. Reuse the owned event value vectors as an undo journal: once
a checked f32 addition succeeds, replace that consumed increment with its
previous finite f32 value represented exactly as f64. Preserve the existing
prune floor and arithmetic order. Defer touched flags until every update
succeeds. On a late event or slot failure, restore completed writes in strict
reverse order, including repeated overlapping events. Do not assume event
columns are unique. A successful checked addition implies that its previous
f32 was finite; conversion to f64 and back preserves it, including signed zero.

This approach should avoid full-arena cloning, a second event-sized allocation,
sorting or per-slot hash maps. It still retains the existing worker deltas and
does not make the arena limit a total process-memory cap. Commit granularity
is one sweep: a previous successful sweep in the same batch remains committed.
The driver must not claim rollback of an entire requested batch or call.

Test late finite overflow in regrets and strategy sums; floor application and
signed zero; touched and untouched columns; repeated overlap; late invalid
column/action width; invalid seat/sample ID and overflow of every progress
counter. Compare full state before/after errors. Compare successful updates
to the old rounding/floor sequence, and retain thread/resume checks. No RNG,
learning parameter, sampling distribution or wire-format change is intended.

## Fixed successful-learning cost screen

After the required checks and release build pass, run three serialized old/new
pairs with the retained preflop Simple partial-reference config. Use the
previous verified 174-file audit binary as the old implementation and a new
verified source snapshot for the candidate. Fix order before measurements:
legacy-1, transactional-1, transactional-2, legacy-2, legacy-3, transactional-3.
Each fresh process trains 8,192 sweeps at seed zero, batch four, eight threads
and 8GiB, using the warm EHS2 cache. Each has a 600-second wall timeout. Export
the same six complete preflop policy-support nodes used by the counterfactual
pilot. Incidental audit evaluation uses 128 worlds at seeds 101/202, one
candidate traversal per seat and no node-frequency sampling. Do not run the
five expensive endpoint fits: this screen tests an expected behavior-preserving
merge repair, not a new trained strategy distribution.

Freeze exact source/archive/binary/config/helper hashes, both literal job
families, all six cases and a preexecution snapshot before the first process.
Require every non-clock raw output to match across all cases, excluding only
construction, fresh-training, candidate-training and ordinary-evaluation elapsed
fields. This is whole exported-output equality, not complete production-state
or checkpoint byte equality; full-state regressions are a separate gate.
Report all individual durations, median training and whole-process times,
lifetime working set and the actual slowdown. Three identical-seed repeats
are timing replicates, not independent learning seeds or confidence intervals.
No Cargo build or other solver measurement should overlap this cohort.

Local resources are sufficient. No GCP allocation is planned, and the wider
preflop quality objective remains active after this reliability repair.
