//! Lowered-shape multiway configs used by the unit tests.
//!
//! `build_multiway_session` and friends consume the *lowered* internal
//! config shape, not the user-facing `solvers.multiway-preflop/v1` schema
//! that `multiway_v1` normalizes into it. These fixtures are that lowered
//! shape, so they live here rather than in `examples/`: the CLI rejects
//! hand-written lowered configs with MWP003, so they are not runnable
//! examples and must not look like ones.

/// Three seats, 2bb stacks, current-street recall, two sweeps. The cheapest
/// config that still builds a real session, tree, and solver.
pub(crate) const LOWERED_3MAX: &str = r#"[game]
kind = "preflop-multiway"
button = 0

[[game.seats]]
name = "BTN"
stack_bb = 2.0
range = ""

[[game.seats]]
name = "SB"
stack_bb = 2.0
range = ""

[[game.seats]]
name = "BB"
stack_bb = 2.0
range = ""

[game.blinds]
small_bb = 0.5
big_bb = 1.0

[game.ante]
kind = "none"

[game.betting]
allow_limp = false

[game.betting.preflop]
bet_sizes = []
raise_sizes = []
max_aggressive_actions = 1
include_allin = true

[game.betting.flop]
bet_sizes = []
raise_sizes = []
max_aggressive_actions = 1
include_allin = false

[game.betting.turn]
bet_sizes = []
raise_sizes = []
max_aggressive_actions = 1
include_allin = false

[game.betting.river]
bet_sizes = []
raise_sizes = []
max_aggressive_actions = 1
include_allin = false

[game.abstraction]
recall = "street"
flop_buckets = 8
turn_buckets = 8
river_buckets = 8
seed = 17

[rake]
kind = "none"

[utility]
kind = "chip-ev"

[algorithm]
schedule = "external-sampling-mccfr"
seed = 19
exploration_epsilon = 0.06
discount_every = 100000
discount_until = 10000000

[run]
sweeps = 2
seed = 19
check_every = 1
storage = "f32"
max_memory_bytes = 67108864
checkpoint_every = 1
evaluation_samples = 8
evaluation_cadence = 1
"#;

/// Nine seats with blinds, antes, and tournament ICM, for the serialization
/// round-trip tests that need every optional section populated.
pub(crate) const LOWERED_9MAX_ICM: &str = r#"[game]
kind = "preflop-multiway"
button = 6

[[game.seats]]
name = "UTG"
stack_bb = 20.0
range = ""

[[game.seats]]
name = "UTG+1"
stack_bb = 20.0
range = ""

[[game.seats]]
name = "MP"
stack_bb = 20.0
range = ""

[[game.seats]]
name = "LJ"
stack_bb = 20.0
range = ""

[[game.seats]]
name = "HJ"
stack_bb = 20.0
range = ""

[[game.seats]]
name = "CO"
stack_bb = 20.0
range = ""

[[game.seats]]
name = "BTN"
stack_bb = 20.0
range = ""

[[game.seats]]
name = "SB"
stack_bb = 20.0
range = ""

[[game.seats]]
name = "BB"
stack_bb = 20.0
range = ""

[game.blinds]
small_bb = 0.5
big_bb = 1.0

[game.ante]
kind = "big-blind"
amount_bb = 1.0

[game.betting]
allow_limp = true

[game.betting.preflop]
bet_sizes = [{ kind = "to-bb", value = 2.2 }]
isolate_sizes = [{ kind = "to-bb", value = 4.0 }]
raise_sizes = [{ kind = "previous-bet-multiple", factor = 3.0 }]
max_aggressive_actions = 3
include_allin = true

[game.betting.flop]
bet_sizes = [{ kind = "pot-after-call", fraction = 0.5 }]
raise_sizes = [{ kind = "pot-after-call", fraction = 0.75 }]
max_aggressive_actions = 2
include_allin = true

[game.betting.turn]
bet_sizes = [{ kind = "pot-after-call", fraction = 0.75 }]
raise_sizes = [{ kind = "pot-after-call", fraction = 0.75 }]
max_aggressive_actions = 2
include_allin = true

[game.betting.river]
bet_sizes = [{ kind = "pot-after-call", fraction = 0.75 }]
raise_sizes = [{ kind = "pot-after-call", fraction = 0.75 }]
max_aggressive_actions = 2
include_allin = true

[game.abstraction]
kind = "ehs2-table"
recall = "street"
flop_buckets = 32
turn_buckets = 32
river_buckets = 32

[rake]
kind = "none"

[utility]
kind = "tournament-icm"
payouts = [1000.0, 650.0, 450.0, 300.0, 200.0, 125.0, 75.0, 50.0, 0.0]
samples = 100000
seed = 11

[algorithm]
schedule = "external-sampling-mccfr"
seed = 7
exploration_epsilon = 0.06
discount_every = 100000
discount_until = 10000000

[run]
sweeps = 1000
seed = 7
check_every = 100
storage = "f32"
max_memory_bytes = 2147483648
checkpoint_every = 500
evaluation_samples = 32
evaluation_cadence = 100
"#;

/// A `solvers.multiway-preflop/v1` config naming the retired rollout
/// abstraction. Only artifact-reading paths accept it; `validate` and
/// `solve` reject it with MWP001.
pub(crate) const V1_RETIRED_ROLLOUT: &str = r#"schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 3
button = 0

[game.defaults]
stack_bb = 2.0
range = "random"

[game.abstraction]
kind = "multiway-rollout"
rollouts_per_state = 8
seed = 17

[game.abstraction.buckets]
flop = 2
turn = 2
river = 2

[solver]
kind = "range-vector"
seed = 19
opponent_exploration = 0.0
batch_sweeps = 1

[run]
max_sweeps = 2

[run.stop]
target = 1000000.0
check_every_sweeps = 1
confirmations = 1
evaluation_samples = 8
deviator_traversals = 1

[run.resources]
threads = 1
memory = "64MiB"

[run.checkpoint]
interval = "15m"
"#;

/// [`LOWERED_3MAX`] with the retired `full` recall, for the artifact-reading
/// tests that must still recognize historical solutions. Nothing solves it:
/// production rejects `full` recall with MWP002.
pub(crate) fn lowered_3max_full_recall() -> String {
    LOWERED_3MAX.replace(r#"recall = "street""#, r#"recall = "full""#)
}
