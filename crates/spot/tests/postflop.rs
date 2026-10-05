use nlh::betting::{Action, SeatStatus};
use nlh::script::{Script, VarKind, Vars};
use nlh::{MwChips, SeatId, Street};
use spot::{Code, Document, NLH_V1, Product, ProductSections, Spot, SpotError, TreeVar};
use std::path::Path;

struct Empty;
impl ProductSections for Empty {
    fn normalize(
        &self,
        _: &Spot,
        _: &toml::Table,
        _: &toml::Table,
    ) -> Result<(toml_edit::Table, toml_edit::Table), SpotError> {
        Ok((toml_edit::Table::new(), toml_edit::Table::new()))
    }
}

fn config(
    players: usize,
    table: &str,
    line: Option<&str>,
    board: Option<&str>,
    ranges: &str,
) -> String {
    let mut text = format!(
        "schema = 'solvers.nlh/v1'\n[table]\nplayers = {players}\nstack_bb = 100\n{table}\n[spot]\n"
    );
    for (key, v) in [("line", line), ("board", board)] {
        if let Some(v) = v {
            text.push_str(&format!("{key} = {}\n", toml_edit::Value::from(v)));
        }
    }
    text.push_str(&format!("[ranges]\n{ranges}\n"));
    text
}
fn standard(line: &str, board: &str) -> String {
    config(
        6,
        "",
        Some(line),
        Some(board),
        "BTN = 'random'\nBB = 'random'",
    )
}
fn parse(text: &str) -> Document {
    Document::parse(text, Path::new("config.toml")).unwrap_or_else(|e| panic!("{e}\n{text}"))
}
fn error(text: &str) -> SpotError {
    Document::parse(text, Path::new("config.toml"))
        .err()
        .unwrap_or_else(|| panic!("accepted {text}"))
}
fn roundtrip(text: &str) -> String {
    let first = parse(text).normalize(&Empty).unwrap();
    let second = parse(&first).normalize(&Empty).unwrap();
    assert_eq!(first, second);
    first
}

#[test]
fn draft_btn_bb_flop_and_full_action_evidence() {
    let doc = parse(&standard("BTN r2.5, BB c", "Ks 7h 2d"));
    let s = &doc.spot;
    assert_eq!(s.product, Product::HuPostflop);
    assert_eq!(s.start.street, Street::Flop);
    assert_eq!(s.context.pot, MwChips(5500));
    assert_eq!(s.context.effective_stack, Some(MwChips(97_500)));
    assert_eq!(s.context.oop.as_ref().unwrap().position, "BB");
    assert_eq!(s.context.ip.as_ref().unwrap().position, "BTN");
    assert_eq!(s.start.previous_street_aggressor, Some(SeatId(0)));
    assert_eq!(s.context.previous_street_aggressor, Some(SeatId(0)));
    for seat in [SeatId(0), SeatId(2)] {
        assert_eq!(
            s.context.seats[seat.index()].starting_stack,
            MwChips(100_000)
        );
        assert_eq!(
            s.context.seats[seat.index()].remaining_stack,
            MwChips(97_500)
        );
        assert_eq!(
            s.context.seats[seat.index()].total_contribution,
            MwChips(2500)
        );
    }
    assert_eq!(
        s.context
            .actions
            .iter()
            .map(|a| a.position.as_str())
            .collect::<Vec<_>>(),
        ["UTG", "HJ", "CO", "BTN", "SB", "BB"]
    );
    assert_eq!(
        s.context
            .actions
            .iter()
            .map(|a| a.implicit)
            .collect::<Vec<_>>(),
        [true, true, true, false, true, false]
    );
    assert!(
        s.context
            .actions
            .iter()
            .filter(|a| a.implicit)
            .all(|a| a.action == Action::Fold)
    );
    assert_eq!(
        s.context.folded_seats,
        [SeatId(1), SeatId(3), SeatId(4), SeatId(5)]
    );
    assert_eq!(s.ranges[SeatId(0)].range.num_combos(), 1176);
}

#[test]
fn draft_utg_btn_flop_implicit_end_folds() {
    let text = config(
        6,
        "",
        Some("UTG r2.5, BTN c"),
        Some("Ks 7h 2d"),
        "UTG = 'random'\nBTN = 'random'",
    );
    let d = parse(&text);
    assert_eq!(d.spot.context.pot, MwChips(6500));
    assert_eq!(d.spot.context.oop.as_ref().unwrap().position, "UTG");
    assert_eq!(d.spot.context.ip.as_ref().unwrap().position, "BTN");
    assert!(
        d.spot
            .context
            .actions
            .iter()
            .rev()
            .take(2)
            .all(|a| a.implicit)
    );
    roundtrip(&text);
}

#[test]
fn draft_turn_start_and_river_start() {
    let turn = "BTN r2.5, BB c / BB x, BTN b1.8, BB c";
    let d = parse(&standard(turn, "Ks 7h 2d Ac"));
    assert_eq!(d.spot.start.street, Street::Turn);
    assert_eq!(d.spot.context.pot, MwChips(9100));
    assert_eq!(d.spot.context.effective_stack, Some(MwChips(95_700)));
    assert_eq!(d.spot.context.previous_street_aggressor, Some(SeatId(0)));
    let river = format!("{turn} / BB b3, BTN r9, BB c");
    let d = parse(&standard(&river, "Ks 7h 2d Ac 9s"));
    assert_eq!(d.spot.context.street, Street::River);
    assert_eq!(d.spot.context.pot, MwChips(27_100));
    assert_eq!(d.spot.context.effective_stack, Some(MwChips(86_700)));
    roundtrip(&standard(&river, "Ks 7h 2d Ac 9s"));
}

#[test]
fn checked_street_clears_previous_aggressor_but_keeps_preflop_history() {
    let d = parse(&standard("BTN r2.5, BB c / BB x, BTN x", "Ks 7h 2d Ac"));
    assert_eq!(d.spot.context.previous_street_aggressor, None);
    assert_eq!(
        d.spot
            .context
            .ip
            .as_ref()
            .unwrap()
            .preflop
            .last_preflop_aggressor_position,
        "BTN"
    );
}

#[test]
fn draft_straddle_pot_and_unraised_straddler_check() {
    let text = config(
        6,
        "straddles_bb = [2,4]",
        Some("BTN r10, HJ c"),
        Some("Ks 7h 2d"),
        "BTN = 'random'\nHJ = 'random'",
    );
    let d = parse(&text);
    assert_eq!(d.spot.context.pot, MwChips(23_500));
    assert_eq!(d.spot.context.effective_stack, Some(MwChips(90_000)));
    assert_eq!(d.spot.context.oop.as_ref().unwrap().position, "HJ");
    roundtrip(&text);
    let d = parse(&config(
        6,
        "straddles_bb = [2,4]",
        Some("BTN c, HJ x"),
        Some("Ks 7h 2d"),
        "BTN = 'random'\nHJ = 'random'",
    ));
    assert_eq!(d.spot.context.pot, MwChips(11_500));
    assert_eq!(d.spot.context.previous_street_aggressor, None);
    assert!(
        !d.spot
            .context
            .oop
            .as_ref()
            .unwrap()
            .preflop
            .preflop_participant
    );
}

#[test]
fn ante_and_bb_ante_dead_money_and_asymmetric_remaining_stacks() {
    for (table, btn, bb) in [
        ("ante_bb = 0.1", 97_400, 97_400),
        ("bb_ante_bb = 0.6", 97_500, 96_900),
    ] {
        let d = parse(&config(
            6,
            table,
            Some("BTN r2.5, BB c"),
            Some("Ks 7h 2d"),
            "BTN = 'random'\nBB = 'random'",
        ));
        assert_eq!(d.spot.context.pot, MwChips(6100));
        assert_eq!(d.spot.context.seats[0].remaining_stack, MwChips(btn));
        assert_eq!(d.spot.context.seats[2].remaining_stack, MwChips(bb));
        assert_eq!(d.spot.context.effective_stack, Some(MwChips(btn.min(bb))));
        let total = d
            .spot
            .context
            .seats
            .iter()
            .map(|s| s.remaining_stack.0 + s.total_contribution.0)
            .sum::<u64>();
        assert_eq!(total, 600_000);
    }
}

#[test]
fn heads_up_limp_and_shortest_grid_amount() {
    let d = parse(&config(
        2,
        "",
        Some("BTN c, BB x"),
        Some("Ks 7h 2d"),
        "BTN = 'random'\nBB = 'random'",
    ));
    assert_eq!(d.spot.context.pot, MwChips(2000));
    assert_eq!(d.spot.context.oop.as_ref().unwrap().seat, SeatId(1));
    let text = standard("BTN r2.501, BB c", "Ks 7h 2d");
    assert_eq!(parse(&text).spot.context.pot, MwChips(5502));
    assert!(roundtrip(&text).contains("line = \"BTN r2.501, BB c\""));
}

#[test]
fn every_preflop_fact_matches_legacy_postflop_definitions() {
    for (line, names, limpers, flats, cold, last, participants) in [
        (
            "BTN r2.5, BB c",
            ["BB", "BTN"],
            0,
            1,
            0,
            "BTN",
            [true, true],
        ),
        (
            "UTG r2.5, BTN c",
            ["UTG", "BTN"],
            0,
            1,
            1,
            "UTG",
            [true, true],
        ),
        (
            "UTG c, BTN r3, UTG c",
            ["UTG", "BTN"],
            1,
            1,
            0,
            "BTN",
            [true, true],
        ),
        (
            "UTG r2.5, HJ c, BTN r10, UTG c",
            ["UTG", "BTN"],
            0,
            1,
            1,
            "BTN",
            [true, true],
        ),
        ("BTN c, BB x", ["BB", "BTN"], 1, 0, 0, "", [false, true]),
    ] {
        let ranges = format!("{} = 'random'\n{} = 'random'", names[0], names[1]);
        let d = parse(&config(6, "", Some(line), Some("Ks 7h 2d"), &ranges));
        let players = [
            d.spot.context.oop.as_ref().unwrap(),
            d.spot.context.ip.as_ref().unwrap(),
        ];
        for (i, p) in players.iter().enumerate() {
            assert_eq!(p.position, names[i]);
            let facts = &p.preflop;
            assert_eq!(
                (facts.limpers, facts.flats, facts.open_cold_calls),
                (limpers, flats, cold),
                "{line}"
            );
            assert!(!facts.squeeze);
            assert!(!facts.in_position_to_last_aggressor);
            assert_eq!(facts.last_preflop_aggressor_position, last);
            assert_eq!(facts.preflop_participant, participants[i]);
        }
    }
}

#[test]
fn p1_ranges_required_and_folded_positions_forbidden() {
    for (ranges, key) in [
        ("BTN = 'random'", "ranges.BB"),
        ("BB = 'random'", "ranges.BTN"),
        ("BTN = 'random'\nBB = 'random'\nSB = 'random'", "ranges.SB"),
    ] {
        let e = error(&config(
            6,
            "",
            Some("BTN r2.5, BB c"),
            Some("Ks 7h 2d"),
            ranges,
        ));
        assert_eq!(e.code, Code::NLH003);
        assert_eq!(e.key.as_deref(), Some(key));
    }
}

#[test]
fn p1_ranges_filter_board_cards_and_require_disjoint_pair() {
    for (ranges, accepted) in [
        ("BTN = 'KsKh,AsAh'\nBB = 'QcQd'", true),
        ("BTN = 'KsKh'\nBB = 'QcQd'", false),
        ("BTN = 'AsAh'\nBB = 'AsAc'", false),
        ("BTN = 'AsAh'\nBB = 'AsAc,QcQd'", true),
        ("BTN = 'AA:0'\nBB = 'random'", false),
    ] {
        let text = config(6, "", Some("BTN r2.5, BB c"), Some("Ks 7h 2d"), ranges);
        if accepted {
            let d = parse(&text);
            assert_eq!(d.spot.ranges[SeatId(0)].range.num_combos(), 1);
            roundtrip(&text);
        } else {
            assert_eq!(error(&text).code, Code::NLH003);
        }
    }
}

#[test]
fn all_alternative_line_spellings_are_nlh004() {
    for line in [
        "BTN r100, BB c",
        "BTN b100, BB c",
        "BTN b2.5, BB c",
        "BTN f, BB c",
        "btn r2.5, BB c",
        "UTG2 r2.5, BB c",
        "BTN r2.5; BB c",
        "BTN  r2.5, BB c",
        "BTN r2.5,  BB c",
        " BTN r2.5, BB c",
        "BTN r2.5, BB c ",
        "BTN r2.5,BB c",
        "BTN r2.5, BB c / ",
        "BTN r2.50, BB c",
        "BTN r3.0, BB c",
        "BTN r03, BB c",
        "BTN r+3, BB c",
        "BTN r-3, BB c",
        "BTN r3e0, BB c",
        "BTN r2.5001, BB c",
        "BTN raise 2.5, BB call",
        "BTN R2.5, BB C",
        "F-F-R2.5-C",
        "BTN r2.5 / BB c",
        "BTN r2.5, BB c, BB x",
        "BTN r2.5, BB c / BB r2, BTN c",
        "BTN r2.5, BB c / BB b2, BTN b6, BB c",
        "BTN r2.5, BB c / BB f",
        "BTN r2.5, BB c / BTN x, BB x",
        "BTN r2.5, BB c / BB x",
        "BTN r2.5, BB c / BB x, BTN b1.8",
        "BTN r2.5, BB c / BB x, BTN c",
        "BTN r1.9, BB c",
        "BTN r101, BB c",
        "BTN x, BB c",
    ] {
        let e = error(&standard(line, "Ks 7h 2d"));
        assert_eq!(e.code, Code::NLH004, "{line}: {e}");
        assert_eq!(e.key.as_deref(), Some("spot.line"));
    }
}

#[test]
fn errors_name_correct_spelling() {
    for (line, correct) in [
        ("BTN r100, BB c", "use a"),
        ("BTN b100, BB c", "use a"),
        ("BTN b2.5, BB c", "use r2.5"),
        ("BTN r2.50, BB c", "use r2.5"),
        ("BTN r3.0, BB c", "use r3"),
        ("BTN r03, BB c", "use r3"),
        ("BTN r+3, BB c", "use r3"),
        ("btn r2.5, BB c", "use BTN"),
        ("BTN r2.5, BB c / BB r2, BTN c", "use b2"),
        ("BTN raise 2.5, BB c", "use r2.5"),
        ("BTN R2.5, BB c", "use r2.5"),
    ] {
        assert!(
            error(&standard(line, "Ks 7h 2d")).message.contains(correct),
            "{line}"
        );
    }
    let e = error(&config(
        2,
        "[table.stacks_bb]\nBB = 10",
        Some("BTN r20, BB a"),
        Some("Ks 7h 2d"),
        "BTN = 'random'\nBB = 'random'",
    ));
    assert_eq!(e.code, Code::NLH004);
    assert!(e.message.contains("use c"));
}

#[test]
fn implicit_folds_cannot_skip_a_check() {
    for line in ["BTN c", "BTN c, BB c"] {
        let e = error(&standard(line, "Ks 7h 2d"));
        assert_eq!(e.code, Code::NLH004);
        assert!(e.message.contains("BB x") || e.message.contains("use x"));
    }
    let e = error(&config(
        6,
        "straddles_bb = [2,4]",
        Some("BTN c"),
        Some("Ks 7h 2d"),
        "BTN = 'random'\nHJ = 'random'",
    ));
    assert_eq!(e.code, Code::NLH004);
    assert!(e.message.contains("HJ x"));
}

#[test]
fn board_spelling_duplicates_and_count_boundaries() {
    for board in [
        "ks 7h 2d",
        "KS 7h 2d",
        "Ks7h2d",
        "Ks  7h 2d",
        " Ks 7h 2d",
        "Ks 7h 2d ",
        "Ks, 7h, 2d",
        "Ks 7h Ks",
        "Ks 7h",
        "Ks 7h 2d Ac 9s Tc",
        "",
        "Ks\t7h 2d",
    ] {
        let e = error(&standard("BTN r2.5, BB c", board));
        assert_eq!(e.code, Code::NLH003, "{board:?}: {e}");
        assert_eq!(e.key.as_deref(), Some("spot.board"));
    }
    for (line, board) in [
        ("", "Ks 7h 2d"),
        ("BTN r2.5, BB c", "Ks 7h 2d Ac"),
        ("BTN r2.5, BB c / BB x, BTN x", "Ks 7h 2d"),
        ("BTN r2.5, BB c / BB x, BTN x / BB x, BTN x", "Ks 7h 2d Ac"),
    ] {
        let e = error(&standard(line, board));
        assert_eq!(e.code, Code::NLH004);
        assert_eq!(e.key.as_deref(), Some("spot.board"));
    }
}

#[test]
fn every_unsupported_product_case_is_explained() {
    for (text, message) in [
        (
            config(2, "[table.stacks_bb]\nBTN = 0.5\nBB = 0.5", None, None, ""),
            "no decision left",
        ),
        (
            config(6, "", Some("BTN r2.5, BB c"), None, ""),
            "line without board",
        ),
        (
            config(6, "", Some("UTG r2.5, BTN c, BB c"), Some("Ks 7h 2d"), ""),
            "3+ players",
        ),
        (
            config(6, "", Some("BTN r2.5"), Some("Ks 7h 2d"), ""),
            "no decision left",
        ),
        (
            config(2, "", Some("BTN a, BB c"), Some("Ks 7h 2d"), ""),
            "no decision left",
        ),
        (
            config(
                2,
                "[table.stacks_bb]\nBB = 10",
                Some("BTN r20, BB c"),
                Some("Ks 7h 2d"),
                "",
            ),
            "no decision left",
        ),
        (
            config(
                3,
                "[table.stacks_bb]\nBTN = 5\nSB = 10",
                Some("BTN a, SB a, BB c"),
                Some("Ks 7h 2d"),
                "",
            ),
            "no decision left",
        ),
        (
            config(
                6,
                "[table.stacks_bb]\nUTG = 5",
                Some("UTG a, BTN c, BB c"),
                Some("Ks 7h 2d"),
                "",
            ),
            "3+ players",
        ),
        (
            standard("BTN r2.5, BB c / BB a, BTN c", "Ks 7h 2d Ac"),
            "no decision left",
        ),
        (
            standard(
                "BTN r2.5, BB c / BB x, BTN x / BB x, BTN x / BB x, BTN x",
                "Ks 7h 2d Ac 9s",
            ),
            "no decision left",
        ),
    ] {
        let e = error(&text);
        assert_eq!(e.code, Code::NLH005, "{e}\n{text}");
        assert!(e.message.contains(message), "{e}");
    }
    for line in [None, Some("")] {
        assert_eq!(
            parse(&config(2, "", line, None, "")).spot.product,
            Product::MultiwayPreflop
        );
    }
}

#[test]
fn short_allin_reopen_is_enforced_during_line_replay() {
    let table = "[table.stacks_bb]\nSB = 3";
    let text = config(
        6,
        table,
        Some("BTN r2.5, SB a, BB c, BTN r6"),
        Some("Ks 7h 2d"),
        "",
    );
    let e = error(&text);
    assert_eq!(e.code, Code::NLH004);
    assert!(e.message.contains("not reopened"));
    let text = config(
        6,
        table,
        Some("BTN r2.5, SB a, BB r6, BTN c"),
        Some("Ks 7h 2d"),
        "",
    );
    assert_eq!(error(&text).code, Code::NLH005); // legal re-raise, but 3 players including SB remain
}

#[test]
fn p1_accepts_every_board_variable_and_preserves_preflop_tree_settings() {
    for &var in TreeVar::ALL.iter().filter(|v| v.is_board_var()) {
        let condition = match var.kind() {
            VarKind::Bool => var.name().to_owned(),
            VarKind::Number => format!("{} >= 0", var.name()),
            VarKind::Text => format!("{} == \"K\"", var.name()),
        };
        let text = format!(
            "{}[tree]\nscript = '''preflop {{ replace raise [3bb] }}\nflop when {condition} {{ checkdown }}'''\npreflop_reraise_jam_above_stack = {{ numerator = 1, denominator = 3 }}\n[tree.max_aggressive_actions]\npreflop = 7\n",
            standard("BTN r2.5, BB c", "Ks 7h 2d")
        );
        let d = parse(&text);
        assert_eq!(d.spot.tree.max_aggressive_actions.preflop, 7);
        assert_eq!(d.spot.tree.compiled.rules[0].street, Street::Preflop);
        assert_eq!(d.spot.tree.compiled.rules.len(), 2);
        assert!(roundtrip(&text).contains("preflop = 7"));
    }
}

#[test]
fn validate_summary_is_serializable_and_rules_recompile() {
    let text = format!(
        "{}[tree]\nscript = '''param z = 3bb\nparam alpha = 33\npreflop {{ remove call }}\nflop {{ if paired {{ replace bet [alpha, e, 3e, min, 80%effective] }} else {{ replace raise [z, a] }} }}'''\n[tree.params]\nz = '4bb'\nalpha = 75",
        standard("BTN r2.5, BB c", "Ks 7h 2d")
    );
    let d = parse(&text);
    let summary = d.summary();
    assert_eq!(summary.product, Product::HuPostflop);
    assert_eq!(summary.actions.len(), 6);
    assert_eq!(summary.tree.params[0].name, "z");
    assert_eq!(summary.tree.params[0].value, "4bb");
    assert_eq!(summary.tree.params[1].value, "75");
    assert!(summary.warnings.is_empty());
    let value = toml::Value::try_from(&summary).unwrap();
    assert_eq!(
        value["start"]["board"].as_array().unwrap()[0].as_str(),
        Some("Ks")
    );
    let flattened = summary.tree.rules.join("\n");
    assert_eq!(
        Script::compile(&flattened, &Default::default(), &NLH_V1)
            .unwrap()
            .rules,
        d.spot.tree.compiled.rules
    );
    assert!(d.spot.start.seats[SeatId(1)].status == SeatStatus::Folded);
    roundtrip(&text);
}

#[test]
fn normalized_key_order_stacks_units_empty_tables_and_meta_inline() {
    let text = format!(
        "{}[meta]\nname = 'name'\ndescription = 'description'\nderived_from = {{ board = 'Ks 7h 2d', line = 'BTN c', solution_hash = 'hash', run_id = 'id' }}\n[economics.rake]\nrate = 0.05\ncap_bb = 4\n[tree]\nscript = '''param z = 3bb\nparam alpha = true\nflop when unopened == alpha {{ replace bet [z] }}'''\nallin_threshold = 0.85\npreflop_reraise_jam_above_stack = {{ denominator = 3, numerator = 1 }}\n[tree.params]\nalpha = false\nz = '4bb'\n[run]\nthreads = 2\nmemory = '6GiB'\nmax_time = '12h'\ncheckpoint_interval = '15m'",
        config(6, "[table.stacks_bb]\nCO = 80", None, None, "")
    );
    let n = roundtrip(&text);
    for keys in [
        vec!["name =", "description =", "derived_from ="],
        vec![
            "players =",
            "sb_bb =",
            "ante_bb =",
            "bb_ante_bb =",
            "straddles_bb =",
            "[table.stacks_bb]",
        ],
        vec![
            "rate =",
            "cap_bb =",
            "when =",
            "allocation =",
            "rounding =",
            "rounding_unit_bb =",
        ],
        vec![
            "script =",
            "include_allin =",
            "allin_threshold =",
            "preflop_reraise_jam_above_stack =",
            "[tree.max_aggressive_actions]",
            "[tree.params]",
        ],
        vec![
            "threads =",
            "memory =",
            "max_time =",
            "checkpoint_interval =",
        ],
    ] {
        let indices = keys.iter().map(|k| n.find(k).unwrap()).collect::<Vec<_>>();
        assert!(indices.windows(2).all(|p| p[0] < p[1]), "{keys:?}\n{n}");
    }
    assert!(n.contains("derived_from = { run_id = \"id\", solution_hash = \"hash\", line = \"BTN c\", board = \"Ks 7h 2d\" }"));
    assert!(!n.contains("[meta.derived_from]"));
    assert!(!n.contains("stack_bb ="));
    assert!(n.contains("[tree.params]\nz = \"4bb\"\nalpha = false"));
    assert!(n.contains("memory = \"6GiB\""));
    assert!(n.contains("max_time = \"12h\""));
    assert!(n.contains("checkpoint_interval = \"15m\""));
    for (memory, expected) in [
        ("4096", "\"4KiB\""),
        ("2097152", "\"2MiB\""),
        ("1025", "1025"),
        ("9223372036854775807", "9223372036854775807"),
    ] {
        let text = format!("{}[run]\nmemory = {memory}", config(2, "", None, None, ""));
        assert!(roundtrip(&text).contains(&format!("memory = {expected}")));
    }
    for (duration, expected) in [
        ("43200s", "12h"),
        ("900s", "15m"),
        ("90s", "90s"),
        ("0.5s", "0.5s"),
    ] {
        let text = format!(
            "{}[run]\nmax_time = '{duration}'",
            config(2, "", None, None, "")
        );
        assert!(roundtrip(&text).contains(&format!("max_time = \"{expected}\"")));
    }
    let n = roundtrip(&config(2, "", None, None, ""));
    for section in ["meta", "tree.params", "solver", "output", "table.stacks_bb"] {
        assert!(!n.contains(&format!("[{section}]")));
    }
}

#[test]
fn positions_follow_documentation_order_and_equal_stacks_use_one_key() {
    for (players, order) in [
        (2, "BTN BB"),
        (3, "BTN SB BB"),
        (4, "CO BTN SB BB"),
        (5, "HJ CO BTN SB BB"),
        (6, "UTG HJ CO BTN SB BB"),
        (7, "UTG LJ HJ CO BTN SB BB"),
        (8, "UTG UTG1 LJ HJ CO BTN SB BB"),
        (9, "UTG UTG1 UTG2 LJ HJ CO BTN SB BB"),
    ] {
        let n = roundtrip(&config(
            players,
            "[table.stacks_bb]\nBTN = 99",
            None,
            None,
            "",
        ));
        let value: toml_edit::DocumentMut = n.parse().unwrap();
        for table in [&value["table"]["stacks_bb"], &value["ranges"]] {
            assert_eq!(
                table
                    .as_table()
                    .unwrap()
                    .iter()
                    .map(|(k, _)| k)
                    .collect::<Vec<_>>(),
                order.split(' ').collect::<Vec<_>>()
            );
        }
    }
    let uniform = config(2, "", None, None, "");
    let explicit =
        "schema = 'solvers.nlh/v1'\n[table]\nplayers = 2\n[table.stacks_bb]\nBB = 100\nBTN = 100";
    assert_eq!(roundtrip(&uniform), roundtrip(explicit));
}

#[test]
fn product_hook_field_order_is_preserved() {
    struct Ordered;
    impl ProductSections for Ordered {
        fn normalize(
            &self,
            _: &Spot,
            _: &toml::Table,
            _: &toml::Table,
        ) -> Result<(toml_edit::Table, toml_edit::Table), SpotError> {
            #[derive(serde::Serialize)]
            struct Settings {
                z: bool,
                a: u64,
            }
            Ok((
                toml_edit::ser::to_document(&Settings { z: true, a: 3 })
                    .unwrap()
                    .into_table(),
                toml_edit::Table::new(),
            ))
        }
    }
    let n = parse(&config(2, "", None, None, ""))
        .normalize(&Ordered)
        .unwrap();
    assert!(n.contains("[solver]\nz = true\na = 3"));
    assert_eq!(parse(&n).normalize(&Ordered).unwrap(), n);
}

#[test]
fn old_schemas_have_a_dedicated_migration_message_and_unknown_params_have_a_key() {
    for schema in [
        "solvers.postflop/v1",
        "solvers.multiway-preflop/v1",
        "solvers.toy/v1",
        "solvers.preflop-hu/v1",
    ] {
        let e = error(&format!("schema = '{schema}'"));
        assert_eq!(e.code, Code::NLH001);
        assert!(e.message.contains("replaced by solvers.nlh/v1"));
        assert!(e.message.contains("not converted automatically"));
    }
    assert!(
        error("schema = 'unknown'")
            .message
            .contains("unknown schema")
    );
    let e = error(&format!(
        "{}[tree.params]\nundeclared = 1",
        config(2, "", None, None, "")
    ));
    assert_eq!(e.key.as_deref(), Some("tree.params.undeclared"));
    assert!(!e.message.contains("line 0"));
}
