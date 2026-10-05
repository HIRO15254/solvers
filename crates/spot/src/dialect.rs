//! The v1 union vocabulary and strict size grammar. Evaluation belongs to products.
use nlh::script::{ActionKind, Dialect, VarKind, Vars};
use nlh::{ParseSizeError, SizeSpec, SizeUnit, Street};

macro_rules! variables {
    ($( $variant:ident => ($name:literal, $kind:ident, $board:literal) ),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum TreeVar { $( $variant, )* }
        impl Vars for TreeVar {
            fn name(self) -> &'static str { match self { $( Self::$variant => $name, )* } }
            fn kind(self) -> VarKind { match self { $( Self::$variant => VarKind::$kind, )* } }
        }
        impl TreeVar {
            pub const ALL: &'static [Self] = &[$( Self::$variant, )*];
            /// Board-dependent variables are forbidden in P2 public trees.
            pub fn is_board_var(self) -> bool { match self { $( Self::$variant => $board, )* } }
        }
    }
}

variables! {
    Aggressions => ("aggressions", Number, false),
    Raises => ("raises", Number, false),
    Unopened => ("unopened", Bool, false),
    Players => ("players", Number, false),
    Position => ("position", Text, false),
    InPosition => ("in_position", Bool, false),
    Spr => ("spr", Number, false),
    Pot => ("pot", Number, false),
    ToCall => ("to_call", Number, false),
    FacingPct => ("facing_pct", Number, false),
    Cbet => ("cbet", Bool, false),
    Donk => ("donk", Bool, false),
    Limpers => ("limpers", Number, false),
    Flats => ("flats", Number, false),
    Squeeze => ("squeeze", Bool, false),
    OpenColdCalls => ("open_cold_calls", Number, false),
    PreflopParticipant => ("preflop_participant", Bool, false),
    InPositionToLastAggressor => ("in_position_to_last_aggressor", Bool, false),
    LastPreflopAggressorPosition => ("last_preflop_aggressor_position", Text, false),
    BoardCards => ("board_cards", Number, true),
    BoardSuits => ("board_suits", Number, true),
    BoardRanks => ("board_ranks", Number, true),
    StraightRanks => ("straight_ranks", Number, true),
    Paired => ("paired", Bool, true),
    Monotone => ("monotone", Bool, true),
    TwoTone => ("two_tone", Bool, true),
    Rainbow => ("rainbow", Bool, true),
    FlushPossible => ("flush_possible", Bool, true),
    StraightPossible => ("straight_possible", Bool, true),
    HighCard => ("high_card", Text, true),
    LowCard => ("low_card", Text, true),
}

pub static NLH_V1: Dialect<TreeVar> = Dialect {
    vars: TreeVar::ALL,
    streets: &[
        ("preflop", Street::Preflop),
        ("flop", Street::Flop),
        ("turn", Street::Turn),
        ("river", Street::River),
    ],
    actions: &[
        ActionKind::Fold,
        ActionKind::Check,
        ActionKind::Call,
        ActionKind::Bet,
        ActionKind::Raise,
    ],
    unit: SizeUnit::Bb,
    size_parser: Some(parse_size_literal),
};

// Validate P2 conditions before lowering can simplify dead branches away.
pub(crate) const PREFLOP_VARS: &[TreeVar] = &[
    TreeVar::Aggressions,
    TreeVar::Raises,
    TreeVar::Unopened,
    TreeVar::Players,
    TreeVar::Position,
    TreeVar::InPosition,
    TreeVar::Spr,
    TreeVar::Pot,
    TreeVar::ToCall,
    TreeVar::FacingPct,
    TreeVar::Cbet,
    TreeVar::Donk,
    TreeVar::Limpers,
    TreeVar::Flats,
    TreeVar::Squeeze,
    TreeVar::OpenColdCalls,
    TreeVar::PreflopParticipant,
    TreeVar::InPositionToLastAggressor,
    TreeVar::LastPreflopAggressorPosition,
];

pub(crate) fn plain_decimal(text: &str) -> bool {
    let mut parts = text.split('.');
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    digits(parts.next().unwrap_or_default())
        && parts.next().is_none_or(digits)
        && parts.next().is_none()
}

/// Parse only the v1 spellings; legacy aliases never reach `SizeSpec::parse`.
pub fn parse_size_literal(text: &str) -> Result<SizeSpec, ParseSizeError> {
    match text {
        "a" => return Ok(SizeSpec::AllIn),
        "e" => return Ok(SizeSpec::GeometricAllInRemaining),
        "min" => return Ok(SizeSpec::MinRaise),
        _ => {}
    }
    let (number, suffix) = ["%effective", "%stack", "bb", "x", "e", ""]
        .into_iter()
        .find_map(|s| text.strip_suffix(s).map(|n| (n, s)))
        .unwrap();
    let invalid = || ParseSizeError(format!("invalid v1 size literal {text:?}"));
    if !plain_decimal(number) || (suffix == "e" && number.contains('.')) {
        return Err(invalid());
    }
    if suffix == "bb" {
        let exact =
            crate::parse::decimal_chips(number, "tree.script", false).map_err(|_| invalid())?;
        let value = number.parse::<f64>().map_err(|_| invalid())?;
        if crate::parse::decimal_chips(&value.to_string(), "tree.script", false)
            .map_err(|_| invalid())?
            != exact
        {
            return Err(invalid());
        }
    }
    if suffix.is_empty() && number.parse::<f64>().unwrap_or(0.0) < 1.0 {
        return Err(ParseSizeError(
            "bare pot percentages must be at least 1; use a percentage of at least 1".into(),
        ));
    }
    SizeSpec::parse(text, SizeUnit::Bb)
}
