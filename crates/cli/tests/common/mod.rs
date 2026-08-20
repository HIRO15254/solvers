//! Configs shared by the CLI integration tests.
//!
//! These are rejection fixtures: the binary must refuse them with a stable
//! error code. They live here rather than in `examples/` so that directory
//! only holds configs a user can actually run.

/// The lowered internal config shape. `solve` rejects hand-written lowered
/// configs with MWP003; only `solvers.multiway-preflop/v1` is accepted.
pub const LOWERED_LEGACY: &str = r#"[game]
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

/// A v1-schema config naming the retired rollout abstraction. Rejected with
/// MWP001.
pub const V1_RETIRED_ROLLOUT: &str = r#"schema = "solvers.multiway-preflop/v1"

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
