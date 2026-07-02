use serde::Deserialize;

/// One experiment = one TOML file. Unknown fields are rejected so typos
/// fail loudly instead of silently running a different experiment.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct SolveConfig {
    pub game: GameSection,
    #[serde(default)]
    pub rake: RakeSection,
    #[serde(default)]
    pub utility: UtilitySection,
    #[serde(default)]
    pub algorithm: AlgorithmSection,
    pub run: RunSection,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct GameSection {
    /// "kuhn" or "leduc" (hold'em arrives with M2).
    pub kind: String,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum RakeSection {
    #[default]
    None,
    PercentCap {
        rate: f64,
        cap: f64,
        #[serde(default)]
        no_flop_no_drop: bool,
    },
    GgPreflop {
        rate: f64,
        cap: f64,
        exempt_pot: u32,
    },
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum UtilitySection {
    #[default]
    ChipEv,
    Icm {
        payouts: [f64; 2],
    },
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, tag = "schedule", rename_all = "kebab-case")]
pub enum AlgorithmSection {
    Vanilla,
    CfrPlus,
    Dcfr {
        #[serde(default = "default_alpha")]
        alpha: f64,
        #[serde(default)]
        beta: f64,
        #[serde(default = "default_gamma")]
        gamma: f64,
        #[serde(default = "default_true")]
        pow4_reset: bool,
    },
    LinearCfr,
    HsDcfr {
        #[serde(default = "default_gamma0")]
        gamma0: f64,
    },
}

impl Default for AlgorithmSection {
    fn default() -> Self {
        AlgorithmSection::Dcfr {
            alpha: default_alpha(),
            beta: 0.0,
            gamma: default_gamma(),
            pow4_reset: true,
        }
    }
}

fn default_alpha() -> f64 {
    1.5
}
fn default_gamma() -> f64 {
    3.0
}
fn default_gamma0() -> f64 {
    30.0
}
fn default_true() -> bool {
    true
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct RunSection {
    pub iterations: u64,
    /// Exploitability check cadence, in iterations.
    #[serde(default = "default_check_every")]
    pub check_every: u64,
    /// Stop once NashConv (sum of per-player exploitabilities, in chips per
    /// deal) drops below this.
    pub target_nash_conv: Option<f64>,
}

fn default_check_every() -> u64 {
    25
}
