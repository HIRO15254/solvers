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
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
// This is a config struct deserialized once per run and then destructured
// away; the size difference between variants never sits on a hot path, so
// boxing `bets` to appease the lint would just add indirection for nothing.
#[allow(clippy::large_enum_variant)]
pub enum GameSection {
    Kuhn,
    Leduc,
    Postflop {
        /// Whitespace-separated cards, e.g. "Ks 7h 2d" (flop), "...  Js"
        /// (turn), or "... Tc" (river). Board length picks the starting
        /// street.
        board: String,
        /// Out-of-position player's range (acts first postflop), e.g.
        /// "22+,A2s+,AA:0.5".
        oop_range: String,
        /// In-position player's range.
        ip_range: String,
        pot: u32,
        effective_stack: u32,
        #[serde(default = "default_true")]
        iso_merging: bool,
        bets: BetsSection,
    },
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct BetsSection {
    #[serde(default)]
    pub flop: StreetBets,
    #[serde(default)]
    pub turn: StreetBets,
    #[serde(default)]
    pub river: StreetBets,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct StreetBets {
    /// Out-of-position bet/raise sizes, as fractions of the pot after a call.
    #[serde(default)]
    pub oop: Vec<f64>,
    /// In-position bet/raise sizes, as fractions of the pot after a call.
    #[serde(default)]
    pub ip: Vec<f64>,
    #[serde(default = "default_max_raises")]
    pub max_raises: u32,
}

impl Default for StreetBets {
    fn default() -> Self {
        StreetBets {
            oop: Vec::new(),
            ip: Vec::new(),
            max_raises: default_max_raises(),
        }
    }
}

fn default_max_raises() -> u32 {
    2
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
    /// Rayon global thread pool size (postflop only). Defaults to rayon's
    /// own choice (all cores) when unset.
    pub threads: Option<usize>,
    /// Overrides `ParConfig::chance_depth` (postflop only; default 2).
    pub par_chance_depth: Option<u32>,
    /// Overrides `ParConfig::min_children` (postflop only; default 12).
    pub par_min_children: Option<usize>,
}

fn default_check_every() -> u64 {
    25
}
