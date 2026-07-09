use std::path::PathBuf;

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
    Preflop {
        /// Per-player starting stack, in big blinds.
        effective_stack_bb: f64,
        /// Small blind size, in big blinds (SB = `Player::P0`).
        #[serde(default = "default_sb_bb")]
        sb_bb: f64,
        /// SB's (or BB's iso-raise) first-raise raise-to sizes, in big
        /// blinds.
        #[serde(default = "default_open_sizes_bb")]
        open_sizes_bb: Vec<f64>,
        /// Reraise-to factors per raise level (see `preflop::PreflopConfig`
        /// docs); an empty outer list means sized reraises are never offered
        /// (all-in only).
        #[serde(default = "default_raise_factors")]
        raise_factors: Vec<Vec<f64>>,
        #[serde(default = "default_preflop_max_raises")]
        max_raises: u32,
        #[serde(default = "default_true")]
        include_allin: bool,
        #[serde(default = "default_true")]
        allow_limp: bool,
        /// SB's range spec (e.g. "22+,A2s+"); `None` is the full range.
        #[serde(default)]
        sb_range: Option<String>,
        /// BB's range spec; `None` is the full range.
        #[serde(default)]
        bb_range: Option<String>,
        /// Per-player equity-realization factors for non-all-in
        /// continuations (see `preflop::EquityShowdown`).
        #[serde(default = "default_equity_realization")]
        equity_realization: [f64; 2],
        /// Disk cache path for the exact 169x169 equity table; `None`
        /// recomputes it in memory every run.
        #[serde(default)]
        equity_cache: Option<PathBuf>,
    },
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct BetsSection {
    #[serde(default)]
    pub flop: StreetBets,
    #[serde(default)]
    pub turn: StreetBets,
    #[serde(default)]
    pub river: StreetBets,
}

#[derive(Deserialize, Debug, Clone)]
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
/// `pub(crate)` (rather than private) so `bench` can build an `HsDcfr`
/// section with the same default `gamma0` the config schema would use.
pub(crate) fn default_gamma0() -> f64 {
    30.0
}
fn default_true() -> bool {
    true
}

fn default_sb_bb() -> f64 {
    0.5
}
fn default_open_sizes_bb() -> Vec<f64> {
    vec![2.5]
}
fn default_raise_factors() -> Vec<Vec<f64>> {
    vec![vec![3.0]]
}
fn default_preflop_max_raises() -> u32 {
    4
}
fn default_equity_realization() -> [f64; 2] {
    [1.0, 1.0]
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct RunSection {
    pub iterations: u64,
    /// Exploitability check cadence, in iterations.
    #[serde(default = "default_check_every")]
    pub check_every: u64,
    /// Storage backend for regrets/strategy sums. `f32` is the plain
    /// backend; `i16` is the quantized backend (see `engine::I16Storage`),
    /// trading precision for ~4x less memory on large postflop trees.
    #[serde(default)]
    pub storage: StorageKind,
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

/// Which `engine::Storage` backend a solve uses. Chosen once from the
/// config (`[run] storage = "f32" | "i16"`) and threaded through as a
/// generic parameter, so the solve path never pays for a `dyn` indirection
/// on the hot per-hand loop just to support both backends.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum StorageKind {
    #[default]
    F32,
    I16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflop_config_minimal_applies_defaults() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 100.0

[run]
iterations = 10
"#;
        let config: SolveConfig = toml::from_str(raw).expect("parse minimal preflop config");
        match config.game {
            GameSection::Preflop {
                effective_stack_bb,
                sb_bb,
                open_sizes_bb,
                raise_factors,
                max_raises,
                include_allin,
                allow_limp,
                sb_range,
                bb_range,
                equity_realization,
                equity_cache,
            } => {
                assert_eq!(effective_stack_bb, 100.0);
                assert_eq!(sb_bb, 0.5);
                assert_eq!(open_sizes_bb, vec![2.5]);
                assert_eq!(raise_factors, vec![vec![3.0]]);
                assert_eq!(max_raises, 4);
                assert!(include_allin);
                assert!(allow_limp);
                assert_eq!(sb_range, None);
                assert_eq!(bb_range, None);
                assert_eq!(equity_realization, [1.0, 1.0]);
                assert_eq!(equity_cache, None);
            }
            other => panic!("expected GameSection::Preflop, got {other:?}"),
        }
    }

    #[test]
    fn preflop_config_fully_specified_overrides_every_default() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 10.0
sb_bb = 0.5
open_sizes_bb = []
raise_factors = []
max_raises = 1
include_allin = true
allow_limp = false
sb_range = "22+,A2s+"
bb_range = "QQ+"
equity_realization = [0.9, 1.1]
equity_cache = ".cache/preflop_equity.bin"

[run]
iterations = 10
"#;
        let config: SolveConfig =
            toml::from_str(raw).expect("parse fully-specified preflop config");
        match config.game {
            GameSection::Preflop {
                effective_stack_bb,
                sb_bb,
                open_sizes_bb,
                raise_factors,
                max_raises,
                include_allin,
                allow_limp,
                sb_range,
                bb_range,
                equity_realization,
                equity_cache,
            } => {
                assert_eq!(effective_stack_bb, 10.0);
                assert_eq!(sb_bb, 0.5);
                assert_eq!(open_sizes_bb, Vec::<f64>::new());
                assert_eq!(raise_factors, Vec::<Vec<f64>>::new());
                assert_eq!(max_raises, 1);
                assert!(include_allin);
                assert!(!allow_limp);
                assert_eq!(sb_range.as_deref(), Some("22+,A2s+"));
                assert_eq!(bb_range.as_deref(), Some("QQ+"));
                assert_eq!(equity_realization, [0.9, 1.1]);
                assert_eq!(
                    equity_cache,
                    Some(PathBuf::from(".cache/preflop_equity.bin"))
                );
            }
            other => panic!("expected GameSection::Preflop, got {other:?}"),
        }
    }

    #[test]
    fn preflop_config_rejects_unknown_field() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 100.0
typo_field = 1

[run]
iterations = 10
"#;
        let result: Result<SolveConfig, _> = toml::from_str(raw);
        assert!(
            result.is_err(),
            "an unknown field in a preflop config must fail to parse"
        );
    }
}
