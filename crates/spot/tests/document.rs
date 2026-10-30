use economics::{CompiledRake, RakeAllocation, RakeConfig, UtilityConfig};
use nlh::script::{Condition, Script, VarKind, Vars};
use nlh::{ChipScale, MwChips, Rounding, SeatId, SizeContext, SizeSpec, Street};
use spot::{
    Code, Document, NLH_V1, Product, ProductSections, Spot, SpotError, TreeVar, other_product_key,
    parse_size_literal,
};
use std::path::Path;

const ROOT: &str = "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 6\nstack = 100\n";

// Test-only product hook. Product keys remain opaque to the common crate.
struct Stub;
impl ProductSections for Stub {
    fn normalize(
        &self,
        spot: &Spot,
        solver: &toml::Table,
        output: &toml::Table,
    ) -> Result<(toml_edit::Table, toml_edit::Table), SpotError> {
        if solver.contains_key("iso_merging") {
            return Err(other_product_key(spot, "solver.iso_merging"));
        }
        let mut solver = solver.clone();
        let mut output = output.clone();
        solver
            .entry("test_default")
            .or_insert(toml::Value::Boolean(true));
        output
            .entry("test_default")
            .or_insert(toml::Value::Integer(7));
        Ok((
            toml_edit::ser::to_document(&solver).unwrap().into_table(),
            toml_edit::ser::to_document(&output).unwrap().into_table(),
        ))
    }
}

fn parse(text: &str) -> Document {
    Document::parse(text, Path::new("config.toml")).unwrap_or_else(|e| panic!("{text}\n{e}"))
}
fn error(text: &str) -> SpotError {
    match Document::parse(text, Path::new("config.toml")) {
        Ok(_) => panic!("accepted: {text}"),
        Err(e) => e,
    }
}
fn roundtrip(text: &str) -> String {
    let first = parse(text).normalize(&Stub).unwrap();
    let second = parse(&first).normalize(&Stub).unwrap();
    assert_eq!(first.as_bytes(), second.as_bytes());
    first
}

#[test]
fn final_checkpoint_is_a_p1_boolean_default_and_not_a_p2_key() {
    let p1 = format!(
        "{ROOT}[spot]\nline = 'BTN r2.5, BB c'\nboard = 'Ks 7h 2d'\n[ranges]\nBTN = 'AA'\nBB = 'KK'\n"
    );
    assert!(parse(&p1).spot.run.final_checkpoint);
    assert!(roundtrip(&p1).contains("checkpoint_interval = \"15m\"\nfinal_checkpoint = true"));
    for value in ["true", "false"] {
        let raw = format!("{p1}[run]\nfinal_checkpoint = {value}\n");
        assert_eq!(parse(&raw).spot.run.final_checkpoint, value == "true");
        assert!(roundtrip(&raw).contains(&format!("final_checkpoint = {value}")));
        let error = error(&format!("{ROOT}[run]\nfinal_checkpoint = {value}\n"));
        assert_eq!(error.code, Code::NLH002);
        assert_eq!(error.key.as_deref(), Some("run.final_checkpoint"));
    }
    assert!(!roundtrip(ROOT).contains("final_checkpoint"));
    for value in ["1", "'false'", "[]"] {
        let error = error(&format!("{p1}[run]\nfinal_checkpoint = {value}\n"));
        assert_eq!(error.code, Code::NLH002);
        assert_eq!(error.key.as_deref(), Some("run.final_checkpoint"));
    }
}

#[test]
fn every_common_default_and_section_order() {
    let doc = parse(ROOT);
    let s = &doc.spot;
    assert_eq!(s.product, Product::MultiwayPreflop);
    assert_eq!(
        s.table.positions.as_slice(),
        ["BTN", "SB", "BB", "UTG", "HJ", "CO"]
    );
    assert!(s.table.stacks.iter().all(|v| *v == MwChips(100_000)));
    assert_eq!(s.table.sb, MwChips(500));
    assert_eq!(s.table.ante, MwChips::ZERO);
    assert_eq!(s.table.bb_ante, MwChips::ZERO);
    assert!(s.table.setup.straddles.is_empty());
    assert_eq!(s.table.setup.button, SeatId(0));
    assert_eq!(s.table.setup.preflop_first_to_act, SeatId(3));
    assert_eq!(s.table.setup.nominal_big_blind, ChipScale::DEFAULT.bb());
    assert!(matches!(s.economics.rake, RakeConfig::None));
    assert!(matches!(s.economics.utility, UtilityConfig::ChipEv));
    assert_eq!(s.economics.compiled_rake, CompiledRake::None);
    assert_eq!(s.start.street, Street::Preflop);
    assert!(
        s.ranges
            .iter()
            .all(|r| r.text == "random" && r.range.num_combos() == 1326)
    );
    assert_eq!(s.tree.script, "");
    assert!(s.tree.compiled.rules.is_empty());
    assert!(s.tree.params.is_empty());
    assert!(s.run.threads.is_none());
    assert!(s.run.memory_bytes.is_none());
    assert!(s.run.max_time_seconds.is_none());
    assert_eq!(s.run.checkpoint_interval_seconds, 900.0);
    let normalized = roundtrip(ROOT);
    let mut previous = 0;
    for section in [
        "table",
        "economics",
        "spot",
        "ranges",
        "tree",
        "solver",
        "run",
        "output",
    ] {
        let at = normalized.find(&format!("[{section}]")).unwrap();
        assert!(at > previous);
        previous = at;
    }
    assert!(normalized.contains("script = '''\n'''"));
    assert!(!normalized.contains("max_time"));
    assert!(!normalized.contains("[meta]"));
    assert!(!normalized.contains("[tree.params]"));
    assert!(!normalized.contains("[table.stacks]"));
    assert!(normalized.contains("stack = \"100000c\""));
    assert_eq!(
        parse(&normalized).solver["test_default"].as_bool(),
        Some(true)
    );
}

#[test]
fn spec_p2_example_is_idempotent() {
    roundtrip(&format!(
        "{ROOT}[tree]\nscript = '''\npreflop {{\n  when unopened {{ replace raise [2.5bb, a] remove call }}\n  when aggressions >= 1 {{ replace raise [3x, a] }}\n}}\nflop, turn, river {{ checkdown }}\n'''\n[run]\nmemory = \"6GiB\"\nmax_time = \"12h\"\n"
    ));
}

#[test]
fn every_cash_key_explicit_and_idempotent() {
    let text = format!(
        "{ROOT}sb = 0.25\nante = 0.125\nbb_ante = 0\nstraddles = [2, 4]\n[table.stacks]\nCO = 80.123\n[meta]\nname = \"six\"\ndescription = \"description\"\n[meta.derived_from]\nrun_id = \"id\"\nsolution_hash = \"hash\"\nline = \"BTN r2.5, BB c\"\nboard = \"Ks 7h 2d\"\n[economics]\nkind = \"cash\"\n[economics.rake]\nrate = 0.05\ncap = 4.001\nwhen = \"flop_dealt && players_dealt >= 2\"\nallocation = \"proportional\"\nrounding = \"nearest\"\nrounding_unit = 0.001\n[spot]\nline = \"\"\n[ranges]\nBTN = \"AA:0.5,KK\"\nBB = \"random\"\n[tree]\nscript = '''\nparam size = 2.5bb\nparam yes = true\nparam count = 1\nparam threshold = 2.5\npreflop when unopened == yes && aggressions < count && spr > threshold {{ replace raise [size] }}\n'''\n[tree.params]\nsize = \"3bb\"\nyes = false\ncount = 2\nthreshold = 3.5\n[solver]\nopaque = 5\n[solver.nested]\nkey = true\n[run]\nthreads = 2\nmemory = 2048\nmax_time = \"0.5h\"\ncheckpoint_interval = \"10s\"\n[output]\nopaque = \"kept\"\n"
    );
    let d = parse(&text);
    assert_eq!(
        d.spot.meta.derived_from.as_ref().unwrap().run_id.as_deref(),
        Some("id")
    );
    assert_eq!(d.spot.table.stacks[SeatId(5)], MwChips(80_123));
    assert_eq!(d.spot.tree.compiled.params[0].default, "3bb");
    assert_eq!(
        d.spot.tree.compiled.rules[0].sizes,
        [SizeSpec::ToChips { value: 3000 }]
    );
    assert_eq!(d.spot.run.max_time_seconds, Some(1800.0));
    let n = roundtrip(&text);
    assert!(n.contains("CO = \"80123c\""));
    assert!(n.contains("cap = \"4001c\""));
    assert_eq!(parse(&n).solver["opaque"].as_integer(), Some(5));
}

#[test]
fn source_is_relative_inlined_and_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("trees")).unwrap();
    std::fs::write(
        dir.path().join("trees/test.tree"),
        "preflop { replace raise [3bb] }\r\n",
    )
    .unwrap();
    let text = format!("{ROOT}[tree]\nsource = \"trees/test.tree\"\n");
    let d = Document::parse(&text, &dir.path().join("config.toml")).unwrap();
    let n = d.normalize(&Stub).unwrap();
    assert!(!n.contains("source ="));
    assert!(!n.contains('\r'));
    assert_eq!(n, roundtrip(&n));
    std::fs::remove_file(dir.path().join("trees/test.tree")).unwrap();
    assert!(Document::parse(&n, &dir.path().join("config.toml")).is_ok());
    let e = match Document::parse(&text, &dir.path().join("config.toml")) {
        Err(e) => e,
        Ok(_) => panic!(),
    };
    assert_eq!(e.code, Code::NLH003);
    assert_eq!(e.key.as_deref(), Some("tree.source"));
}

#[test]
fn schema_errors_and_error_rendering() {
    for old in [
        "solvers.postflop/v1",
        "solvers.multiway-preflop/v1",
        "solvers.toy/v1",
        "solvers.preflop-hu/v1",
        "solvers.nlh/v2",
        "",
    ] {
        let e = error(&format!("schema = \"{old}\"\n"));
        assert_eq!(e.code, Code::NLH001);
        assert!(e.message.contains("solvers.nlh/v1"));
        assert!(e.message.contains("docs/nlh-input-v1.jp.md"));
    }
    assert_eq!(error("[table]\nplayers = 2").code, Code::NLH001);
    assert!(
        error("[table]\nplayers = 2")
            .message
            .contains("docs/nlh-input-v1.jp.md")
    );
    assert_eq!(error("schema = 1").code, Code::NLH002);
    assert_eq!(error("schema = ").code, Code::NLH002);
    for code in [
        Code::NLH001,
        Code::NLH002,
        Code::NLH003,
        Code::NLH004,
        Code::NLH005,
    ] {
        assert_eq!(
            SpotError::new(code, "table.players", "bad").to_string(),
            format!("{code}: table.players: bad")
        );
        let e = SpotError {
            code,
            key: None,
            message: "bad".into(),
        };
        assert_eq!(e.to_string(), format!("{code}: bad"));
    }
}

#[test]
fn unknown_keys_at_every_closed_level() {
    for section in [
        "meta",
        "meta.derived_from",
        "table",
        "economics",
        "economics.rake",
        "spot",
        "tree",
        "run",
    ] {
        let text = if section == "table" {
            format!("{ROOT}bogus = 1\n")
        } else {
            format!("{ROOT}[{section}]\nbogus = 1\n")
        };
        let e = error(&text);
        assert_eq!(e.code, Code::NLH002, "{section}");
        assert_eq!(e.key, Some(format!("{section}.bogus")));
    }
    assert_eq!(
        error("schema = \"solvers.nlh/v1\"\nbogus = 1\n")
            .key
            .as_deref(),
        Some("bogus")
    );
    for (text, key) in [
        (format!("{ROOT}[ranges]\nbtn = \"random\""), "ranges.btn"),
        (
            format!("{ROOT}[table.stacks]\nUTG2 = 5"),
            "table.stacks.UTG2",
        ),
    ] {
        let e = error(&text);
        assert_eq!(e.code, Code::NLH003);
        assert_eq!(e.key.as_deref(), Some(key));
    }
}

macro_rules! invalid {
    ($name:ident, $tail:expr, $code:ident, $key:expr $(, $message:expr)?) => {
        #[test]
        fn $name() {
            let e = error(&format!("{ROOT}{}", $tail));
            assert_eq!(e.code, Code::$code);
            assert_eq!(e.key.as_deref(), Some($key));
            $(assert_eq!(e.message, $message);)?
        }
    };
}
invalid!(
    players_wrong_type,
    "[table.stacks]\nBTN = true",
    NLH002,
    "table.stacks.BTN"
);
invalid!(sb_zero, "sb = 0", NLH003, "table.sb");
invalid!(sb_too_big, "sb = 1.001", NLH003, "table.sb");
invalid!(ante_negative, "ante = -1", NLH003, "table.ante");
invalid!(
    antes_exclusive,
    "ante = 1\nbb_ante = 1",
    NLH003,
    "table.bb_ante"
);
invalid!(
    straddle_too_small,
    "straddles = [1.999]",
    NLH003,
    "table.straddles.0"
);
invalid!(
    restraddle_too_small,
    "straddles = [2,3.999]",
    NLH003,
    "table.straddles.1"
);
invalid!(
    too_many_straddles,
    "straddles = [2,4,8,16,32]",
    NLH003,
    "table.straddles"
);
invalid!(
    straddle_no_chips_left,
    "straddles = [100]",
    NLH003,
    "table.straddles.0"
);
invalid!(
    straddle_ante_no_chips_left,
    "straddles = [99]\nante = 1",
    NLH003,
    "table.straddles.0"
);
invalid!(
    straddle_bad_type,
    "straddles = [true]",
    NLH002,
    "table.straddles.0"
);
invalid!(
    bad_stack_override,
    "[table.stacks]\nBTN = 0",
    NLH003,
    "table.stacks.BTN"
);
invalid!(bad_range, "[ranges]\nBTN = \"XX\"", NLH003, "ranges.BTN");
invalid!(
    empty_range,
    "[ranges]\nBTN = \"AA:0\"",
    NLH003,
    "ranges.BTN"
);
invalid!(range_type, "[ranges]\nBTN = 2", NLH002, "ranges.BTN");
invalid!(
    bad_economics_kind,
    "[economics]\nkind = \"Cash\"",
    NLH003,
    "economics.kind"
);
invalid!(
    cash_tournament_key,
    "[economics]\npayouts = [10]",
    NLH002,
    "economics.payouts"
);
invalid!(
    missing_rake_rate,
    "[economics.rake]\ncap = 1",
    NLH002,
    "economics.rake.rate"
);
invalid!(
    rake_rate_high,
    "[economics.rake]\nrate = 1.001",
    NLH003,
    "economics.rake.rate"
);
invalid!(
    rake_rate_nan,
    "[economics.rake]\nrate = nan",
    NLH003,
    "economics.rake.rate"
);
invalid!(
    rake_when_bad,
    "[economics.rake]\nrate = 0.05\nwhen = \"players\"",
    NLH003,
    "economics.rake.when"
);

#[test]
fn arbitrary_positive_milli_bb_rake_units_normalize_and_invalid_units_fail() {
    for unit in ["0.001", "0.01", "0.5", "1", "2.501"] {
        let raw = format!("{ROOT}[economics.rake]\nrate = 0.05\nrounding_unit = {unit}\n");
        let doc = Document::parse(&raw, Path::new("unit.toml")).unwrap();
        let normalized = doc.normalize(&Stub).unwrap();
        let reparsed = Document::parse(&normalized, Path::new("unit.toml")).unwrap();
        assert_eq!(
            doc.spot.economics.compiled_rake,
            reparsed.spot.economics.compiled_rake
        );
    }
    for unit in ["0", "-0.001", "nan", "inf", "-inf"] {
        let raw = format!("{ROOT}[economics.rake]\nrate = 0.05\nrounding_unit = {unit}\n");
        let err = Document::parse(&raw, Path::new("unit.toml")).err().unwrap();
        assert_eq!(err.code, Code::NLH003, "{unit}: {err}");
        assert_eq!(err.key.as_deref(), Some("economics.rake.rounding_unit"));
    }
}

#[test]
fn chip_denomination_defaults_normalize_and_reject_bad_values() {
    let setup = parse(ROOT).spot.table.setup;
    assert_eq!(setup.scale(), ChipScale::DEFAULT);
    assert_eq!(setup.chip_rounding, Rounding::Nearest);
    assert!(roundtrip(ROOT).contains("players = 6\nbb_chips = 1000\n"));
    for (chips, rounding, mode) in [
        (100, "down", Rounding::Down),
        (4, "up", Rounding::Up),
        (2, "nearest", Rounding::Nearest),
    ] {
        let raw = format!("{ROOT}bb_chips = {chips}\nchip_rounding = \"{rounding}\"\n");
        let normalized = roundtrip(&raw);
        assert!(normalized.contains(&format!("chip_rounding = \"{rounding}\"\n")));
        let setup = parse(&normalized).spot.table.setup;
        assert_eq!(setup.nominal_big_blind, MwChips(chips));
        assert_eq!(setup.chip_rounding, mode);
    }
    for value in ["0", "-3", "1000001"] {
        let e = error(&format!("{ROOT}bb_chips = {value}\n"));
        assert_eq!(e.code, Code::NLH003, "{value}: {e}");
        assert_eq!(e.key.as_deref(), Some("table.bb_chips"));
    }
    let e = error(&format!("{ROOT}bb_chips = 2.5\n"));
    assert_eq!(e.code, Code::NLH002);
    assert_eq!(e.key.as_deref(), Some("table.bb_chips"));
    let e = error(&format!("{ROOT}chip_rounding = \"floor\"\n"));
    assert_eq!(e.code, Code::NLH003);
    assert_eq!(e.key.as_deref(), Some("table.chip_rounding"));
    let e = error(&format!("{ROOT}chip_rounding = 1\n"));
    assert_eq!(e.code, Code::NLH002);
    assert_eq!(e.key.as_deref(), Some("table.chip_rounding"));
}

/// A 1-3 game with $1 chips: 1 BB = 3 chips, so the SB is one chip and many
/// BB decimals are not whole chips.
const ONE_THREE: &str =
    "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 6\nbb_chips = 3\nsb = '1c'\n";

#[test]
fn chip_keys_express_non_decimal_denominations() {
    let raw = format!(
        "{ONE_THREE}stack = 100\nstraddles = ['6c']\n[table.stacks]\nCO = '250c'\n[tree]\nscript = \"preflop {{ add raise [40c, 20bb] }}\"\n"
    );
    let doc = parse(&raw);
    let table = &doc.spot.table;
    assert_eq!(table.sb, MwChips(1));
    assert_eq!(table.setup.forced_blinds[SeatId(2)], MwChips(3));
    assert_eq!(table.stacks[SeatId(0)], MwChips(300));
    assert_eq!(table.stacks[SeatId(5)], MwChips(250));
    assert_eq!(table.setup.straddles[0].1, MwChips(6));
    let normalized = roundtrip(&raw);
    // Non-decimal tables spell every amount group in chips.
    for expected in [
        "bb_chips = 3\n",
        "sb = \"1c\"\n",
        "straddles = [\"6c\"]\n",
        "[table.stacks]\n",
    ] {
        assert!(normalized.contains(expected), "{expected}\n{normalized}");
    }
    assert!(!normalized.contains("_bb ="), "{normalized}");
    assert_eq!(
        parse_size_literal("40c"),
        Ok(SizeSpec::ToChips { value: 40 })
    );
    for literal in ["0c", "1.5c", "c"] {
        assert!(parse_size_literal(literal).is_err(), "{literal}");
    }
}

#[test]
fn amount_values_accept_bb_and_exact_chips_and_reject_bad_spellings() {
    for value in ["100", "'100'", "'100bb'", "'300c'"] {
        let doc = parse(&format!("{ONE_THREE}stack = {value}\n"));
        assert_eq!(doc.spot.table.stacks[SeatId(0)], MwChips(300), "{value}");
        assert!(doc.warnings.is_empty(), "{value}");
        assert!(roundtrip(&format!("{ONE_THREE}stack = {value}\n")).contains("stack = \"300c\""));
    }
    for value in ["'2.5c'", "'01c'", "'100BB'", "'-1bb'", "'1e2'"] {
        let e = error(&format!("{ONE_THREE}stack = {value}\n"));
        assert_eq!(e.code, Code::NLH003, "{value}: {e}");
        assert_eq!(e.key.as_deref(), Some("table.stack"));
    }
    let e = error(&format!("{ONE_THREE}stack = true\n"));
    assert_eq!(e.code, Code::NLH002);
    assert_eq!(e.key.as_deref(), Some("table.stack"));
}

#[test]
fn bb_amounts_round_and_legacy_keys_are_rejected() {
    for (mode, expected) in [("down", 301), ("nearest", 302), ("up", 302)] {
        let raw = format!("{ONE_THREE}stack = 100.5\nchip_rounding = '{mode}'\n");
        let doc = parse(&raw);
        assert_eq!(doc.spot.table.stacks[SeatId(0)], MwChips(expected));
        assert!(doc.warnings.iter().any(|w| w.contains("table.stack")
            && w.contains("100.5")
            && w.contains(&expected.to_string())
            && w.contains(mode)));
        let effective = doc.normalize(&Stub).unwrap();
        assert!(parse(&effective).warnings.is_empty());
    }
    for (tail, key, replacement) in [
        ("stack_bb = 100", "table.stack_bb", "table.stack"),
        ("stack_chips = 300", "table.stack_chips", "table.stack"),
        (
            "[table.stacks_bb]\nCO = 80",
            "table.stacks_bb",
            "table.stacks",
        ),
        (
            "[economics.rake]\nrate = 0.05\ncap_chips = 3",
            "economics.rake.cap_chips",
            "economics.rake.cap",
        ),
    ] {
        let e = error(&format!("{ONE_THREE}{tail}"));
        assert_eq!(e.code, Code::NLH002, "{tail}: {e}");
        assert_eq!(e.key.as_deref(), Some(key), "{tail}: {e}");
        assert!(
            e.message.contains(&format!("use {replacement} instead")),
            "{tail}: {e}"
        );
    }
    let doc =
        parse("schema = \"solvers.nlh/v1\"\n[table]\nplayers = 6\nbb_chips = 3\nstack = 100\n");
    assert_eq!(doc.spot.table.sb, MwChips(2));
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.contains("table.sb") && w.contains("0.5") && w.contains("2 chips"))
    );
}

#[test]
fn rounded_inputs_warn_and_effective_config_is_idempotent() {
    let raw = format!(
        "{ONE_THREE}stack = 100.5\nante = 0.5\nstraddles = [2.5]\n[table.stacks]\nCO = 80.5\n[economics.rake]\nrate = 0.05\ncap = 0.5\nrounding_unit = 0.5\n"
    );
    let doc = parse(&raw);
    for key in [
        "table.stack",
        "table.ante",
        "table.straddles.0",
        "table.stacks.CO",
        "economics.rake.cap",
        "economics.rake.rounding_unit",
    ] {
        assert!(
            doc.warnings
                .iter()
                .any(|warning| warning.contains(key) && warning.contains("nearest")),
            "{key}: {:?}",
            doc.warnings
        );
    }
    let normalized = doc.normalize(&Stub).unwrap();
    let effective = parse(&normalized);
    assert!(effective.warnings.is_empty(), "{:?}", effective.warnings);
    assert_eq!(normalized, effective.normalize(&Stub).unwrap());
}

#[test]
fn tournament_and_big_blind_ante_round_with_original_spelling() {
    let raw = format!(
        "{ONE_THREE}stack = 100\nbb_ante = 5e-1\n[economics]\nkind = 'tournament'\npayouts = [1]\noutside_field = [20.5]\n"
    );
    let doc = parse(&raw);
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.contains("table.bb_ante: 5e-1") && w.contains("2 chips"))
    );
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.contains("economics.outside_field.0") && w.contains("62 chips"))
    );
    let effective = doc.normalize(&Stub).unwrap();
    assert!(parse(&effective).warnings.is_empty());
}

#[test]
fn rounded_zero_and_post_round_limits_are_checked() {
    let zero = format!("{ONE_THREE}stack = 100\nsb = 0.1\nchip_rounding = 'down'\n");
    // Replace the explicit small blind before testing rounding to zero.
    let zero = zero.replace("sb = '1c'\n", "");
    assert_eq!(error(&zero).key.as_deref(), Some("table.sb"));
    let straddle = format!("{ONE_THREE}stack = 100\nstraddles = [1.8]\nchip_rounding = 'down'\n");
    assert_eq!(error(&straddle).key.as_deref(), Some("table.straddles.0"));
}
invalid!(
    rake_allocation_bad,
    "[economics.rake]\nrate = 0.05\nallocation = \"main_first\"",
    NLH003,
    "economics.rake.allocation"
);
invalid!(
    rake_rounding_bad,
    "[economics.rake]\nrate = 0.05\nrounding = \"floor\"",
    NLH003,
    "economics.rake.rounding"
);
invalid!(
    tournament_rake,
    "[economics]\nkind = \"tournament\"\npayouts = [1]\n[economics.rake]\nrate = 0.1",
    NLH003,
    "economics.rake"
);
invalid!(
    tournament_missing_payouts,
    "[economics]\nkind = \"tournament\"",
    NLH002,
    "economics.payouts"
);
invalid!(
    tournament_payout_order,
    "[economics]\nkind = \"tournament\"\npayouts = [1,2]",
    NLH003,
    "economics.payouts.1"
);
invalid!(
    tournament_outside_zero,
    "[economics]\nkind = \"tournament\"\npayouts = [1]\noutside_field = [0]",
    NLH003,
    "economics.outside_field.0"
);
invalid!(
    tournament_too_many_prizes,
    "[economics]\nkind = \"tournament\"\npayouts = [7,6,5,4,3,2,1]",
    NLH003,
    "economics.payouts"
);
invalid!(
    line_without_board,
    "[spot]\nline = \"BTN r2.5, BB c\"",
    NLH005,
    "spot"
);
invalid!(
    board_without_closed_preflop,
    "[spot]\nboard = \"Ks 7h 2d\"",
    NLH004,
    "spot.board"
);
invalid!(spot_type, "[spot]\nline = 1", NLH002, "spot.line");
invalid!(
    tree_script_source_exclusive,
    "[tree]\nscript = \"\"\nsource = \"none\"",
    NLH003,
    "tree.source"
);
invalid!(
    removed_tree_threshold,
    "[tree]\nallin_threshold = 0.5",
    NLH002,
    "tree.allin_threshold",
    "unknown key"
);
invalid!(
    removed_tree_reraise_jam,
    "[tree]\npreflop_reraise_jam_above_stack = { numerator = 1, denominator = 3 }",
    NLH002,
    "tree.preflop_reraise_jam_above_stack",
    "unknown key"
);
invalid!(
    removed_tree_include_allin,
    "[tree]\ninclude_allin = true",
    NLH002,
    "tree.include_allin",
    "unknown key"
);
invalid!(
    removed_tree_aggression_limits,
    "[tree.max_aggressive_actions]\nflop = 3",
    NLH002,
    "tree.max_aggressive_actions",
    "unknown key"
);
invalid!(
    tree_param_array,
    "[tree.params]\nsize = [1]",
    NLH002,
    "tree.params.size"
);
invalid!(
    tree_param_nonfinite,
    "[tree.params]\nsize = inf",
    NLH003,
    "tree.params.size"
);
invalid!(
    tree_unknown_param,
    "[tree.params]\nsize = 5",
    NLH003,
    "tree.params.size"
);
invalid!(threads_zero, "[run]\nthreads = 0", NLH003, "run.threads");
invalid!(
    threads_unknown,
    "[run]\nthreads = \"AUTO\"",
    NLH003,
    "run.threads"
);
invalid!(threads_type, "[run]\nthreads = 1.5", NLH002, "run.threads");
invalid!(memory_zero, "[run]\nmemory = 0", NLH003, "run.memory");
invalid!(
    memory_fraction,
    "[run]\nmemory = \"1.5GiB\"",
    NLH003,
    "run.memory"
);
invalid!(
    memory_bad_suffix,
    "[run]\nmemory = \"2GB\"",
    NLH003,
    "run.memory"
);
invalid!(
    memory_overflow,
    "[run]\nmemory = \"18446744073709551615GiB\"",
    NLH003,
    "run.memory"
);
invalid!(
    max_time_zero,
    "[run]\nmax_time = \"0s\"",
    NLH003,
    "run.max_time"
);
invalid!(
    max_time_bad,
    "[run]\nmax_time = \"1d\"",
    NLH003,
    "run.max_time"
);
invalid!(
    checkpoint_negative,
    "[run]\ncheckpoint_interval = \"-1m\"",
    NLH003,
    "run.checkpoint_interval"
);
invalid!(solver_wrong_type, "solver = 1", NLH002, "table.solver");

#[test]
fn table_sizes_heads_up_positions_and_forced_posts() {
    for players in 2..=9 {
        let d = parse(&format!(
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = {players}\nstack = 100\nsb = 1\nbb_ante = 0.2\n"
        ));
        let table = &d.spot.table;
        assert_eq!(table.positions.len(), players);
        assert_eq!(table.positions[SeatId(0)], "BTN");
        let bb = SeatId(if players == 2 { 1 } else { 2 });
        assert_eq!(table.positions[bb], "BB");
        assert_eq!(d.spot.start.seats[bb].common_committed, MwChips(200));
        assert_eq!(table.setup.common_ante, MwChips(200));
        assert!(table.setup.forced_antes.iter().all(|v| *v == MwChips::ZERO));
    }
    let d = parse(&format!("{ROOT}ante = 0.125\n"));
    assert!(
        d.spot
            .start
            .seats
            .iter()
            .all(|s| s.dead_committed == MwChips(125))
    );
    let d = parse(&format!("{ROOT}straddles = [2,4]\n"));
    assert_eq!(
        d.spot.table.setup.straddles,
        [(SeatId(3), MwChips(2000)), (SeatId(4), MwChips(4000))]
    );
    assert_eq!(d.spot.table.setup.preflop_first_to_act, SeatId(5));
    assert_eq!(d.spot.start.minimum_full_target(), MwChips(8000));
    let d =
        parse("schema = \"solvers.nlh/v1\"\n[table]\nplayers = 3\nstack = 100\nstraddles = [2]");
    assert_eq!(d.spot.table.setup.straddles, [(SeatId(0), MwChips(2000))]);
    assert_eq!(d.spot.table.setup.preflop_first_to_act, SeatId(1));
}

#[test]
fn table_required_grid_and_incomplete_paths() {
    for (text, key, code) in [
        ("schema = \"solvers.nlh/v1\"", "table", Code::NLH002),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nstack = 100",
            "table.players",
            Code::NLH002,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 1",
            "table.players",
            Code::NLH003,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 10",
            "table.players",
            Code::NLH003,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\nstack = nan",
            "table.stack",
            Code::NLH003,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\nstack = 0",
            "table.stack",
            Code::NLH003,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\nstraddles = [2]",
            "table.stacks.BTN",
            Code::NLH003,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\nstack = 100\nstraddles = [2]",
            "table.straddles",
            Code::NLH003,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\n[table.stacks]\nBTN = 10",
            "table.stacks.BB",
            Code::NLH003,
        ),
    ] {
        let e = error(text);
        assert_eq!(e.code, code);
        assert_eq!(e.key.as_deref(), Some(key));
    }
    roundtrip(
        "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\n[table.stacks]\nBTN = 10.125\nBB = 20",
    );
}

#[test]
fn rake_defaults_and_variants() {
    let d = parse(&format!("{ROOT}[economics.rake]\nrate = 0.05"));
    assert!(
        matches!(d.spot.economics.rake, RakeConfig::Generic { cap_chips: None, ref when, allocation: RakeAllocation::MainFirst, rounding: Rounding::Down, .. } if when == "flop_dealt")
    );
    let n = roundtrip(&format!("{ROOT}[economics.rake]\nrate = 0.05"));
    assert!(!n.contains("cap"));
    assert!(n.contains("rounding_unit = \"1c\""));
    for rounding in ["down", "nearest", "up"] {
        roundtrip(&format!(
            "{ROOT}[economics.rake]\nrate = 1\ncap = 0\nwhen = \"true\"\nrounding = \"{rounding}\""
        ));
    }
}

#[test]
fn tournament_defaults_and_removed_sampling_keys() {
    let exact = format!(
        "{ROOT}[economics]\nkind = \"tournament\"\npayouts = [1000,600,400]\noutside_field = [18,26]"
    );
    let d = parse(&exact);
    match &d.spot.economics.utility {
        UtilityConfig::TournamentIcm {
            payouts,
            outside_field,
        } => {
            assert_eq!(payouts, &[1000.0, 600.0, 400.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
            assert_eq!(outside_field[0].name, "outside-0");
            assert_eq!(outside_field[1].name, "outside-1");
        }
        _ => panic!(),
    }
    roundtrip(&exact);
    let default_exact = roundtrip(&format!(
        "{ROOT}[economics]\nkind = \"tournament\"\npayouts = [1000]"
    ));
    assert!(default_exact.contains("outside_field = []"));
    let large = format!(
        "{ROOT}[economics]\nkind = \"tournament\"\npayouts = [1000]\noutside_field = [1,2,3,4,5,6,7,8,9,10]"
    );
    roundtrip(&large);
    // Monte Carlo ICM was replaced by deterministic quadrature; its keys
    // are rejected rather than ignored.
    for (tail, key) in [
        ("samples = 100000", "economics.samples"),
        ("seed = 0", "economics.seed"),
    ] {
        for document in [&exact, &large] {
            let e = error(&format!("{document}\n{tail}"));
            assert_eq!(e.code, Code::NLH002);
            assert_eq!(e.key.as_deref(), Some(key));
        }
    }
}

#[test]
fn operating_units_and_large_memory_roundtrip() {
    for (memory, expected) in [
        ("\"1KiB\"", 1024),
        ("\"2MiB\"", 2097152),
        ("\"6GiB\"", 6442450944),
        ("4096", 4096),
    ] {
        let text = format!(
            "{ROOT}[run]\nthreads = 1\nmemory = {memory}\nmax_time = \"1m\"\ncheckpoint_interval = \"0.5s\""
        );
        assert_eq!(parse(&text).spot.run.memory_bytes, Some(expected));
        roundtrip(&text);
    }
    assert_eq!(
        error(&format!("{ROOT}[run]\nmemory = \"9000000000GiB\"")).code,
        Code::NLH003
    );
    roundtrip(&format!(
        "{ROOT}[run]\nthreads = \"auto\"\nmemory = \"auto\"\nmax_time = \"1h\""
    ));
}

#[test]
fn product_sections_are_raw_until_hook_validation() {
    let d = parse(&format!(
        "{ROOT}[solver]\niso_merging = true\n[output]\nunknown = {{ any = [1,\"x\"] }}"
    ));
    let e = d.normalize(&Stub).unwrap_err();
    assert_eq!(e.code, Code::NLH002);
    assert_eq!(e.key.as_deref(), Some("solver.iso_merging"));
    assert!(e.message.contains("P2"));
}

#[test]
fn strict_size_literal_accept_reject_and_variants() {
    for (text, spec) in [
        ("1%", SizeSpec::PotAfterCall { fraction: 0.01 }),
        (
            "33.3%",
            SizeSpec::PotAfterCall {
                fraction: 33.3 / 100.0,
            },
        ),
        ("0.99", SizeSpec::ToBb { value: 0.99 }),
        ("2.501bb", SizeSpec::ToBb { value: 2.501 }),
        ("3x", SizeSpec::PreviousBetMultiple { factor: 3.0 }),
        ("a", SizeSpec::AllIn),
        ("e", SizeSpec::GeometricAllInRemaining),
        ("3e", SizeSpec::GeometricAllIn { streets: 3 }),
        ("min", SizeSpec::MinRaise),
        (
            "80%effective",
            SizeSpec::EffectiveStackFraction { fraction: 0.8 },
        ),
        ("60%stack", SizeSpec::StackFraction { fraction: 0.6 }),
        ("00.001bb", SizeSpec::ToBb { value: 0.001 }),
        ("255e", SizeSpec::GeometricAllIn { streets: 255 }),
    ] {
        assert_eq!(parse_size_literal(text).unwrap(), spec);
        let script = format!("preflop {{ replace raise [{text}] }}");
        assert_eq!(
            Script::compile(&script, &Default::default(), &NLH_V1)
                .unwrap()
                .rules[0]
                .sizes,
            [spec]
        );
    }
    for text in [
        "allin",
        "50%pot",
        "geometric(allin,streets=2)",
        "0c",
        "2.5c",
        "-2bb",
        "+2bb",
        "1e3",
        "1E3bb",
        ".5bb",
        "2.bb",
        "0",
        "0.99%",
        "0bb",
        "1x",
        "0e",
        "3.0e",
        "256e",
        "0%stack",
        "NaN",
        "inf",
        " 3x",
        "3x ",
    ] {
        assert!(parse_size_literal(text).is_err(), "{text}");
        if !text.starts_with(' ') && !text.ends_with(' ') {
            let e = error(&format!(
                "{ROOT}[tree]\nscript = \"preflop {{ replace raise [{text}] }}\""
            ));
            assert_eq!(e.code, Code::NLH003);
            assert!(e.message.contains("line 1"));
        }
    }
}

#[test]
fn union_variable_names_types_and_p2_board_rejection() {
    assert_eq!(TreeVar::ALL.len(), 31);
    assert_eq!(TreeVar::ALL.iter().filter(|v| v.is_board_var()).count(), 12);
    for &var in TreeVar::ALL {
        let expression = match var.kind() {
            VarKind::Bool => var.name().to_owned(),
            VarKind::Number => format!("{} >= 0", var.name()),
            VarKind::Text => format!("{} == \"BTN\"", var.name()),
        };
        assert!(Condition::parse(&expression, &NLH_V1).is_ok());
        let script = format!("flop when {expression} {{ checkdown }}");
        let text = format!("{ROOT}[tree]\nscript = '''\n{script}\n'''\n");
        if var.is_board_var() {
            let e = error(&text);
            assert_eq!(e.code, Code::NLH003);
            assert!(e.message.contains("P2"));
            assert!(e.message.contains("line 1"));
        } else {
            parse(&text);
        }
    }
    for script in [
        "flop when unopened == false && paired { checkdown }",
        "flop { when paired { } checkdown }",
        "flop { if unopened { checkdown } else if paired { checkdown } }",
    ] {
        assert_eq!(
            error(&format!("{ROOT}[tree]\nscript = '''{script}'''")).code,
            Code::NLH003
        );
    }
}

#[test]
fn non_finite_condition_literals_are_located_nlh003_errors_for_both_products() {
    for (base, street) in [
        (ROOT, "preflop"),
        (
            include_str!("../../../examples/hu-postflop/river_small.toml"),
            "river",
        ),
    ] {
        for number in ["1e999", "-1e999", "inf", "-inf", "nan", "NaN"] {
            for script in [
                format!("\n{street} when pot > {number} {{ remove bet }}"),
                format!("\n{street} when pot in [0, {number}] {{ remove bet }}"),
                format!("\ndefine bad = pot > {number}\n{street} when bad {{ remove bet }}"),
                format!("\nparam bound = {number}\n{street} when pot > bound {{ remove bet }}"),
                format!("\ndefine unused = pot > {number}\n{street} {{ remove bet }}"),
            ] {
                let mut config: toml::Value = base.parse().unwrap();
                config.as_table_mut().unwrap().insert(
                    "tree".into(),
                    toml::Value::Table(toml::Table::from_iter([(
                        "script".into(),
                        script.clone().into(),
                    )])),
                );
                let e = error(&toml::to_string(&config).unwrap());
                assert_eq!(e.code, Code::NLH003, "{script}: {e}");
                assert_eq!(e.key.as_deref(), Some("tree.script"));
                assert!(
                    e.message.contains("finite") && e.message.contains("line 2"),
                    "{script}: {e}"
                );
            }
            let mut config: toml::Value = base.parse().unwrap();
            config.as_table_mut().unwrap().insert(
                "tree".into(),
                toml::Value::Table(toml::Table::from_iter([
                    (
                        "script".into(),
                        format!("\nparam bound = 1\n{street} when pot > bound {{ remove bet }}")
                            .into(),
                    ),
                    (
                        "params".into(),
                        toml::Value::Table(toml::Table::from_iter([(
                            "bound".into(),
                            number.into(),
                        )])),
                    ),
                ])),
            );
            let e = error(&toml::to_string(&config).unwrap());
            assert_eq!(e.code, Code::NLH003);
            assert!(
                e.message.contains("finite") && e.message.contains("line 2"),
                "{e}"
            );
        }
        for number in ["1e3", "-1", "0", "1.25"] {
            let mut config: toml::Value = base.parse().unwrap();
            config.as_table_mut().unwrap().insert(
                "tree".into(),
                toml::Value::Table(toml::Table::from_iter([(
                    "script".into(),
                    format!("{street} when pot > {number} {{ remove bet }}").into(),
                )])),
            );
            parse(&toml::to_string(&config).unwrap());
        }
    }
}

#[test]
fn script_diagnostics_and_literal_delimiter() {
    let e = error(&format!(
        "{ROOT}[tree]\nscript = '''\npreflop {{\n  replace raise [allin]\n}}\n'''"
    ));
    assert!(e.message.contains("line 2"));
    let e = error(&format!("{ROOT}[tree]\nscript = \"# '''\""));
    assert_eq!(e.code, Code::NLH003);
    assert!(e.message.contains("delimiter"));
    for script in [
        "flop { remove call }",
        "flop { force bet [min] }",
        "preflop { add raise [3bb] remove fold }",
        "flop { replace check [] }",
        "define open = unopened && aggressions == 0\npreflop when open { replace raise [3bb] }",
    ] {
        roundtrip(&format!("{ROOT}[tree]\nscript = '''\n{script}\n'''"));
    }
}

#[test]
fn original_toml_bb_precision_is_rounded() {
    let doc = parse(&format!("{ROOT}ante = 0.0014"));
    assert_eq!(doc.spot.table.ante, MwChips(1));
    assert!(doc.warnings.iter().any(|w| w.contains("table.ante")));
}

#[test]
fn dotted_type_errors_for_every_common_shape() {
    for (section, key) in [
        ("meta", "name"),
        ("meta", "description"),
        ("meta.derived_from", "run_id"),
        ("meta.derived_from", "solution_hash"),
        ("meta.derived_from", "line"),
        ("meta.derived_from", "board"),
        ("economics", "kind"),
        ("economics", "rake"),
        ("economics", "payouts"),
        ("economics", "outside_field"),
        ("economics.rake", "rate"),
        ("economics.rake", "cap"),
        ("economics.rake", "when"),
        ("economics.rake", "allocation"),
        ("economics.rake", "rounding"),
        ("economics.rake", "rounding_unit"),
        ("spot", "line"),
        ("spot", "board"),
        ("tree", "script"),
        ("tree", "source"),
        ("tree", "params"),
        ("run", "threads"),
        ("run", "memory"),
        ("run", "max_time"),
        ("run", "checkpoint_interval"),
    ] {
        let e = error(&format!("{ROOT}[{section}]\n{key} = []"));
        // Payout/field arrays are valid, so use a boolean for those two keys.
        if key == "payouts" || key == "outside_field" {
            continue;
        }
        assert_eq!(e.code, Code::NLH002, "{section}.{key}");
        assert_eq!(e.key, Some(format!("{section}.{key}")));
    }
    for key in [
        "players",
        "stack",
        "sb",
        "ante",
        "bb_ante",
        "stacks",
        "straddles",
        "bb_chips",
        "chip_rounding",
    ] {
        let mut value: toml::Value = ROOT.parse().unwrap();
        value["table"]
            .as_table_mut()
            .unwrap()
            .insert(key.into(), toml::Value::Boolean(false));
        let e = error(&toml::to_string(&value).unwrap());
        assert_eq!(e.code, Code::NLH002);
        assert_eq!(e.key, Some(format!("table.{key}")));
    }
    for key in ["solver", "output"] {
        let text = format!(
            "schema = \"solvers.nlh/v1\"\n{key} = false\n[table]\nplayers = 2\nstack = 100"
        );
        assert_eq!(error(&text).key.as_deref(), Some(key));
    }
}

#[test]
fn literal_script_comments_strings_and_initial_newlines_roundtrip() {
    for script in [
        "\npreflop { remove call }\n",
        "# paired is only a comment\npreflop { remove call }",
        "preflop when position == \"paired\" { remove call }",
        "# final quote '",
        "# final quotes ''",
    ] {
        // Use basic strings to preserve initial newlines independently of TOML's literal rule.
        let mut value: toml::Value = ROOT.parse().unwrap();
        let mut tree = toml::Table::new();
        tree.insert("script".into(), toml::Value::String(script.into()));
        value
            .as_table_mut()
            .unwrap()
            .insert("tree".into(), toml::Value::Table(tree));
        let n = roundtrip(&toml::to_string(&value).unwrap());
        assert_eq!(parse(&n).spot.tree.script, script);
    }
}

#[test]
fn tree_bb_literals_round_in_sizes_and_thresholds_and_effective_config_is_stable() {
    for (mode, chips) in [("down", 4), ("nearest", 5), ("up", 5)] {
        let text = format!(
            "{ONE_THREE}stack = 100\nchip_rounding = '{mode}'\n\
             [tree]\nscript = '''\n# 1.5bb remains a comment\n\
             param sizing = 1.5bb\n\
             preflop when position == \"1.5bb\" {{ add raise [sizing, 2bb] merge raise within 1.5bb jam raise >= 1.5bb }}\n'''\n"
        );
        let doc = parse(&text);
        let rules = &doc.spot.tree.compiled.rules;
        assert_eq!(rules.len(), 3);
        assert_eq!(
            rules[0].sizes,
            vec![
                SizeSpec::ToChips { value: chips },
                SizeSpec::ToChips { value: 6 }
            ]
        );
        assert_eq!(rules[1].sizes, vec![SizeSpec::ToChips { value: chips }]);
        assert_eq!(rules[2].sizes, vec![SizeSpec::ToChips { value: chips }]);
        let context = SizeContext {
            big_blind: 3,
            called_to: 0,
            pot_after_call: 6,
            previous_bet: 3,
            minimum: 0,
            maximum: 300,
            effective_stack: 300,
            streets_remaining: 3,
            rounding: doc.spot.table.setup.chip_rounding,
        };
        assert_eq!(context.resolve_u64(rules[0].sizes[0]), chips);
        assert_eq!(context.merge_width_u64(rules[1].sizes[0]), chips);
        assert_eq!(context.threshold_u64(rules[2].sizes[0]), chips);
        assert!(doc.spot.tree.script.contains("# 1.5bb remains a comment"));
        assert!(doc.spot.tree.script.contains("\"1.5bb\""));
        assert_eq!(
            doc.warnings
                .iter()
                .filter(|w| w.contains("tree.script")
                    && w.contains("1.5")
                    && w.contains(&format!("{chips} chips")))
                .count(),
            3
        );
        let effective = doc.normalize(&Stub).unwrap();
        let reread = parse(&effective);
        assert!(reread.warnings.iter().any(|w| w.contains("tree.script")));
        assert_eq!(reread.spot.tree.compiled.rules, *rules);
        assert_eq!(effective, reread.normalize(&Stub).unwrap());
    }
}

#[test]
fn tree_bb_parameter_overrides_round_and_warn_again_on_replay() {
    let text = format!(
        "{ONE_THREE}stack = 100\nchip_rounding = 'up'\n[tree]\nscript = 'param size = 2bb preflop {{ add raise [size] }}'\n[tree.params]\nsize = '1.5bb'\n"
    );
    let doc = parse(&text);
    assert_eq!(
        doc.spot.tree.compiled.rules[0].sizes,
        vec![SizeSpec::ToChips { value: 5 }]
    );
    let context = SizeContext {
        big_blind: 3,
        called_to: 0,
        pot_after_call: 6,
        previous_bet: 3,
        minimum: 0,
        maximum: 300,
        effective_stack: 300,
        streets_remaining: 3,
        rounding: doc.spot.table.setup.chip_rounding,
    };
    assert_eq!(
        context.resolve_u64(doc.spot.tree.compiled.rules[0].sizes[0]),
        5
    );
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.contains("tree.params.size") && w.contains("1.5") && w.contains("5 chips"))
    );
    let effective = doc.normalize(&Stub).unwrap();
    let reread = parse(&effective);
    assert!(
        reread
            .warnings
            .iter()
            .any(|w| w.contains("tree.params.size"))
    );
    assert_eq!(
        reread.spot.tree.compiled.rules,
        doc.spot.tree.compiled.rules
    );
}

#[test]
fn tree_diagnostics_display_absolute_sizes_in_table_units() {
    for (bb_chips, add, merge, jam) in [(1000, "2.5", "0.001", "3"), (3, "2500c", "1c", "3000c")] {
        let text = format!(
            "schema = 'solvers.nlh/v1'\n[table]\nplayers = 6\nbb_chips = {bb_chips}\nstack = 10000\n\
             [tree]\nscript = '''\npreflop {{ add raise [2500c, 50%, 2x, a] \
             merge raise within 1c jam raise >= 3000c }}\n'''\n"
        );
        let summary = parse(&text).summary();
        let expected = vec![
            format!("preflop {{ add raise [{add}, 50%, 2x, a] }}"),
            format!("preflop {{ merge raise within {merge} }}"),
            format!("preflop {{ jam raise >= {jam} }}"),
        ];
        assert_eq!(summary.tree.rules, expected);
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["tree"]["rules"], serde_json::json!(expected));
    }
}
