use economics::{CompiledRake, RakeAllocation, RakeConfig, RakeRounding, UtilityConfig};
use nlh::script::{Condition, Script, VarKind, Vars};
use nlh::{MwChips, SeatId, SizeSpec, Street};
use spot::{
    Code, Document, NLH_V1, Product, ProductSections, Spot, SpotError, TreeVar, other_product_key,
    parse_size_literal,
};
use std::path::Path;

const ROOT: &str = "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 6\nstack_bb = 100\n";

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
    assert_eq!(s.table.setup.nominal_big_blind, MwChips::ONE_BB);
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
    assert!(!s.tree.include_allin);
    assert!(s.tree.allin_threshold.is_none());
    assert!(s.tree.preflop_reraise_jam_above_stack.is_none());
    assert_eq!(s.tree.max_aggressive_actions.preflop, 4);
    assert_eq!(
        (
            s.tree.max_aggressive_actions.flop,
            s.tree.max_aggressive_actions.turn,
            s.tree.max_aggressive_actions.river
        ),
        (3, 3, 3)
    );
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
    assert!(!normalized.contains("allin_threshold"));
    assert!(!normalized.contains("max_time"));
    assert!(!normalized.contains("[meta]"));
    assert!(!normalized.contains("[tree.params]"));
    assert!(!normalized.contains("[table.stacks_bb]"));
    assert!(normalized.contains("stack_bb = 100"));
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
        "{ROOT}sb_bb = 0.25\nante_bb = 0.125\nbb_ante_bb = 0\nstraddles_bb = [2, 4]\n[table.stacks_bb]\nCO = 80.123\n[meta]\nname = \"six\"\ndescription = \"description\"\n[meta.derived_from]\nrun_id = \"id\"\nsolution_hash = \"hash\"\nline = \"BTN r2.5, BB c\"\nboard = \"Ks 7h 2d\"\n[economics]\nkind = \"cash\"\n[economics.rake]\nrate = 0.05\ncap_bb = 4.001\nwhen = \"flop_dealt && players_dealt >= 2\"\nallocation = \"proportional\"\nrounding = \"nearest\"\nrounding_unit_bb = 0.001\n[spot]\nline = \"\"\n[ranges]\nBTN = \"AA:0.5,KK\"\nBB = \"random\"\n[tree]\nscript = '''\nparam size = 2.5bb\nparam yes = true\nparam count = 1\nparam threshold = 2.5\npreflop when unopened == yes && aggressions < count && spr > threshold {{ replace raise [size] }}\n'''\ninclude_allin = true\nallin_threshold = 1\npreflop_reraise_jam_above_stack = {{ numerator = 1, denominator = 3 }}\n[tree.max_aggressive_actions]\npreflop = 5\nflop = 2\nturn = 1\nriver = 0\n[tree.params]\nsize = \"3bb\"\nyes = false\ncount = 2\nthreshold = 3.5\n[solver]\nopaque = 5\n[solver.nested]\nkey = true\n[run]\nthreads = 2\nmemory = 2048\nmax_time = \"0.5h\"\ncheckpoint_interval = \"10s\"\n[output]\nopaque = \"kept\"\n"
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
        [SizeSpec::ToBb { value: 3.0 }]
    );
    assert_eq!(d.spot.tree.max_aggressive_actions.river, 0);
    assert_eq!(d.spot.run.max_time_seconds, Some(1800.0));
    let n = roundtrip(&text);
    assert!(n.contains("CO = 80.123"));
    assert!(n.contains("cap_bb = 4.001"));
    assert!(n.contains("preflop_reraise_jam_above_stack = { numerator = 1, denominator = 3 }"));
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
        "tree.max_aggressive_actions",
        "tree.preflop_reraise_jam_above_stack",
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
            format!("{ROOT}[table.stacks_bb]\nUTG2 = 5"),
            "table.stacks_bb.UTG2",
        ),
    ] {
        let e = error(&text);
        assert_eq!(e.code, Code::NLH003);
        assert_eq!(e.key.as_deref(), Some(key));
    }
}

macro_rules! invalid {
    ($name:ident, $tail:expr, $code:ident, $key:expr) => {
        #[test]
        fn $name() {
            let e = error(&format!("{ROOT}{}", $tail));
            assert_eq!(e.code, Code::$code);
            assert_eq!(e.key.as_deref(), Some($key));
        }
    };
}
invalid!(
    players_wrong_type,
    "[table.stacks_bb]\nBTN = true",
    NLH002,
    "table.stacks_bb.BTN"
);
invalid!(sb_zero, "sb_bb = 0", NLH003, "table.sb_bb");
invalid!(sb_too_big, "sb_bb = 1.001", NLH003, "table.sb_bb");
invalid!(sb_off_grid, "sb_bb = 0.5001", NLH003, "table.sb_bb");
invalid!(ante_negative, "ante_bb = -1", NLH003, "table.ante_bb");
invalid!(
    bbante_off_grid,
    "bb_ante_bb = 0.0001",
    NLH003,
    "table.bb_ante_bb"
);
invalid!(
    antes_exclusive,
    "ante_bb = 1\nbb_ante_bb = 1",
    NLH003,
    "table.bb_ante_bb"
);
invalid!(
    straddle_too_small,
    "straddles_bb = [1.999]",
    NLH003,
    "table.straddles_bb.0"
);
invalid!(
    restraddle_too_small,
    "straddles_bb = [2,3.999]",
    NLH003,
    "table.straddles_bb.1"
);
invalid!(
    straddle_off_grid,
    "straddles_bb = [2.0001]",
    NLH003,
    "table.straddles_bb.0"
);
invalid!(
    too_many_straddles,
    "straddles_bb = [2,4,8,16,32]",
    NLH003,
    "table.straddles_bb"
);
invalid!(
    straddle_no_chips_left,
    "straddles_bb = [100]",
    NLH003,
    "table.straddles_bb.0"
);
invalid!(
    straddle_ante_no_chips_left,
    "straddles_bb = [99]\nante_bb = 1",
    NLH003,
    "table.straddles_bb.0"
);
invalid!(
    straddle_bad_type,
    "straddles_bb = [\"2\"]",
    NLH002,
    "table.straddles_bb.0"
);
invalid!(
    bad_stack_override,
    "[table.stacks_bb]\nBTN = 0",
    NLH003,
    "table.stacks_bb.BTN"
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
    "[economics.rake]\ncap_bb = 1",
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
    rake_cap_grid,
    "[economics.rake]\nrate = 0.05\ncap_bb = 4.0001",
    NLH003,
    "economics.rake.cap_bb"
);
invalid!(
    rake_when_bad,
    "[economics.rake]\nrate = 0.05\nwhen = \"players\"",
    NLH003,
    "economics.rake.when"
);
invalid!(
    rake_rounding_unit,
    "[economics.rake]\nrate = 0.05\nrounding_unit_bb = 0.0015",
    NLH003,
    "economics.rake.rounding_unit_bb"
);

#[test]
fn arbitrary_positive_milli_bb_rake_units_normalize_and_invalid_units_fail() {
    for unit in ["0.001", "0.01", "0.5", "1", "2.501"] {
        let raw = format!("{ROOT}[economics.rake]\nrate = 0.05\nrounding_unit_bb = {unit}\n");
        let doc = Document::parse(&raw, Path::new("unit.toml")).unwrap();
        let normalized = doc.normalize(&Stub).unwrap();
        let reparsed = Document::parse(&normalized, Path::new("unit.toml")).unwrap();
        assert_eq!(
            doc.spot.economics.compiled_rake,
            reparsed.spot.economics.compiled_rake
        );
    }
    for unit in ["0", "-0.001", "0.0015", "nan", "inf", "-inf"] {
        let raw = format!("{ROOT}[economics.rake]\nrate = 0.05\nrounding_unit_bb = {unit}\n");
        let err = Document::parse(&raw, Path::new("unit.toml")).err().unwrap();
        assert_eq!(err.code, Code::NLH003, "{unit}: {err}");
        assert_eq!(err.key.as_deref(), Some("economics.rake.rounding_unit_bb"));
    }
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
    tournament_exact_samples,
    "[economics]\nkind = \"tournament\"\npayouts = [1]\nsamples = 100000",
    NLH003,
    "economics.samples"
);
invalid!(
    tournament_exact_seed,
    "[economics]\nkind = \"tournament\"\npayouts = [1]\nseed = 0",
    NLH003,
    "economics.seed"
);
invalid!(
    tournament_outside_zero,
    "[economics]\nkind = \"tournament\"\npayouts = [1]\noutside_field_bb = [0]",
    NLH003,
    "economics.outside_field_bb.0"
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
    tree_threshold_low,
    "[tree]\nallin_threshold = 0",
    NLH003,
    "tree.allin_threshold"
);
invalid!(
    tree_threshold_high,
    "[tree]\nallin_threshold = 1.01",
    NLH003,
    "tree.allin_threshold"
);
invalid!(
    tree_threshold_inf,
    "[tree]\nallin_threshold = inf",
    NLH003,
    "tree.allin_threshold"
);
invalid!(
    tree_ratio_zero,
    "[tree]\npreflop_reraise_jam_above_stack = { numerator = 0, denominator = 1 }",
    NLH003,
    "tree.preflop_reraise_jam_above_stack.numerator"
);
invalid!(
    tree_ratio_den_negative,
    "[tree]\npreflop_reraise_jam_above_stack = { numerator = 1, denominator = -1 }",
    NLH003,
    "tree.preflop_reraise_jam_above_stack.denominator"
);
invalid!(
    tree_ratio_den_missing,
    "[tree]\npreflop_reraise_jam_above_stack = { numerator = 1 }",
    NLH002,
    "tree.preflop_reraise_jam_above_stack.denominator"
);
invalid!(
    tree_limit_negative,
    "[tree.max_aggressive_actions]\nflop = -1",
    NLH003,
    "tree.max_aggressive_actions.flop"
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
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = {players}\nstack_bb = 100\nsb_bb = 1\nbb_ante_bb = 0.2\n"
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
    let d = parse(&format!("{ROOT}ante_bb = 0.125\n"));
    assert!(
        d.spot
            .start
            .seats
            .iter()
            .all(|s| s.dead_committed == MwChips(125))
    );
    let d = parse(&format!("{ROOT}straddles_bb = [2,4]\n"));
    assert_eq!(
        d.spot.table.setup.straddles,
        [(SeatId(3), MwChips(2000)), (SeatId(4), MwChips(4000))]
    );
    assert_eq!(d.spot.table.setup.preflop_first_to_act, SeatId(5));
    assert_eq!(d.spot.start.minimum_full_target(), MwChips(8000));
    let d = parse(
        "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 3\nstack_bb = 100\nstraddles_bb = [2]",
    );
    assert_eq!(d.spot.table.setup.straddles, [(SeatId(0), MwChips(2000))]);
    assert_eq!(d.spot.table.setup.preflop_first_to_act, SeatId(1));
}

#[test]
fn table_required_grid_and_incomplete_paths() {
    for (text, key, code) in [
        ("schema = \"solvers.nlh/v1\"", "table", Code::NLH002),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nstack_bb = 100",
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
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\nstack_bb = nan",
            "table.stack_bb",
            Code::NLH003,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\nstack_bb = 100.00000000000001",
            "table.stack_bb",
            Code::NLH003,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\nstack_bb = 0",
            "table.stack_bb",
            Code::NLH003,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\nstraddles_bb = [2]",
            "table.stacks_bb.BTN",
            Code::NLH003,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\nstack_bb = 100\nstraddles_bb = [2]",
            "table.straddles_bb",
            Code::NLH003,
        ),
        (
            "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\n[table.stacks_bb]\nBTN = 10",
            "table.stacks_bb.BB",
            Code::NLH003,
        ),
    ] {
        let e = error(text);
        assert_eq!(e.code, code);
        assert_eq!(e.key.as_deref(), Some(key));
    }
    roundtrip(
        "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\n[table.stacks_bb]\nBTN = 10.125\nBB = 20",
    );
}

#[test]
fn rake_defaults_and_variants() {
    let d = parse(&format!("{ROOT}[economics.rake]\nrate = 0.05"));
    assert!(
        matches!(d.spot.economics.rake, RakeConfig::Generic { cap_bb: None, ref when, allocation: RakeAllocation::MainFirst, rounding: RakeRounding::Down, .. } if when == "flop_dealt")
    );
    let n = roundtrip(&format!("{ROOT}[economics.rake]\nrate = 0.05"));
    assert!(!n.contains("cap_bb"));
    assert!(n.contains("rounding_unit_bb = 0.001"));
    for rounding in ["down", "nearest", "up"] {
        roundtrip(&format!(
            "{ROOT}[economics.rake]\nrate = 1\ncap_bb = 0\nwhen = \"true\"\nrounding = \"{rounding}\""
        ));
    }
}

#[test]
fn tournament_exact_and_sampled_defaults_and_explicit() {
    let exact = format!(
        "{ROOT}[economics]\nkind = \"tournament\"\npayouts = [1000,600,400]\noutside_field_bb = [18,26]"
    );
    let d = parse(&exact);
    match &d.spot.economics.utility {
        UtilityConfig::TournamentIcm {
            payouts,
            outside_field,
            samples,
            seed,
        } => {
            assert_eq!(payouts, &[1000.0, 600.0, 400.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
            assert_eq!(*samples, 100000);
            assert_eq!(*seed, 0);
            assert_eq!(outside_field[0].name, "outside-0");
            assert_eq!(outside_field[1].name, "outside-1");
        }
        _ => panic!(),
    }
    let n = roundtrip(&exact);
    assert!(!n.contains("samples ="));
    assert!(!n.contains("seed ="));
    let default_exact = roundtrip(&format!(
        "{ROOT}[economics]\nkind = \"tournament\"\npayouts = [1000]"
    ));
    assert!(default_exact.contains("outside_field_bb = []"));
    let sampled = format!(
        "{ROOT}[economics]\nkind = \"tournament\"\npayouts = [1000]\noutside_field_bb = [1,2,3,4,5,6,7,8,9,10]"
    );
    let n = roundtrip(&sampled);
    assert!(n.contains("samples = 100000"));
    assert!(n.contains("seed = 0"));
    roundtrip(&format!("{sampled}\nsamples = 2\nseed = 5"));
    for (tail, key) in [
        ("samples = 1", "economics.samples"),
        ("seed = -1", "economics.seed"),
    ] {
        let e = error(&format!("{sampled}\n{tail}"));
        assert_eq!(e.code, Code::NLH003);
        assert_eq!(e.key.as_deref(), Some(key));
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
        ("1", SizeSpec::PotAfterCall { fraction: 0.01 }),
        (
            "33.3",
            SizeSpec::PotAfterCall {
                fraction: 33.3 / 100.0,
            },
        ),
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
        "20c",
        "-2bb",
        "+2bb",
        "1e3",
        "1E3bb",
        ".5bb",
        "2.bb",
        "2.0001bb",
        "0",
        "0.99",
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
fn original_toml_bb_precision_is_never_rounded() {
    for number in [
        "0.00100000000000000001",
        "0.99999999999999999999",
        "0.10000000000000000001",
        "1.00000000000000000001e0",
        "9.999999999999999999e-4",
        "9007199254741.001",
    ] {
        let e = error(&format!("{ROOT}ante_bb = {number}"));
        assert_eq!(e.code, Code::NLH003, "{number}");
        assert_eq!(e.key.as_deref(), Some("table.ante_bb"));
    }
    for number in [
        "1e-3", "+0.001", "0.00_1", "1.000e-3", "-0.0", "0x0", "0o1", "0b1",
    ] {
        roundtrip(&format!("{ROOT}ante_bb = {number}"));
    }
    let text =
        format!("{ROOT}[economics.rake]\nrate = 0.05\nrounding_unit_bb = 0.00100000000000000001");
    assert_eq!(
        error(&text).key.as_deref(),
        Some("economics.rake.rounding_unit_bb")
    );
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
        ("economics", "outside_field_bb"),
        ("economics", "samples"),
        ("economics", "seed"),
        ("economics.rake", "rate"),
        ("economics.rake", "cap_bb"),
        ("economics.rake", "when"),
        ("economics.rake", "allocation"),
        ("economics.rake", "rounding"),
        ("economics.rake", "rounding_unit_bb"),
        ("spot", "line"),
        ("spot", "board"),
        ("tree", "script"),
        ("tree", "source"),
        ("tree", "include_allin"),
        ("tree", "allin_threshold"),
        ("tree", "preflop_reraise_jam_above_stack"),
        ("tree", "max_aggressive_actions"),
        ("tree", "params"),
        ("tree.max_aggressive_actions", "preflop"),
        ("tree.max_aggressive_actions", "flop"),
        ("tree.max_aggressive_actions", "turn"),
        ("tree.max_aggressive_actions", "river"),
        ("tree.preflop_reraise_jam_above_stack", "numerator"),
        ("tree.preflop_reraise_jam_above_stack", "denominator"),
        ("run", "threads"),
        ("run", "memory"),
        ("run", "max_time"),
        ("run", "checkpoint_interval"),
    ] {
        let e = error(&format!("{ROOT}[{section}]\n{key} = []"));
        // Payout/field arrays are valid, so use a boolean for those two keys.
        if key == "payouts" || key == "outside_field_bb" {
            continue;
        }
        assert_eq!(e.code, Code::NLH002, "{section}.{key}");
        assert_eq!(e.key, Some(format!("{section}.{key}")));
    }
    for key in [
        "players",
        "stack_bb",
        "sb_bb",
        "ante_bb",
        "bb_ante_bb",
        "stacks_bb",
        "straddles_bb",
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
            "schema = \"solvers.nlh/v1\"\n{key} = false\n[table]\nplayers = 2\nstack_bb = 100"
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
