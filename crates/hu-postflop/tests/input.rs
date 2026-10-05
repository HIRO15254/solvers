use hu_postflop::game::{ChipEv, NoRake, PayoffPipeline};
use hu_postflop::input::{Algorithm, P1Sections, Settings, SolutionStreets, Storage, lower};
use hu_postflop::{
    PostflopConfig, PostflopGame, StreetTree, TreeBuildError, build_postflop_game, memory_usage,
    try_build_postflop_game, try_memory_usage,
};
use nlh::script::{POSTFLOP, Script, Vars};
use nlh::{Chips, Player, Street};
use spot::{Code, Document, TreeVar};
use std::path::Path;

fn text(table: &str, line: &str, board: &str, script: &str) -> String {
    format!(
        "schema = 'solvers.nlh/v1'\n[table]\nplayers = 6\nstack_bb = 100\n{table}\n[spot]\nline = '{line}'\nboard = '{board}'\n[ranges]\nBTN = 'AA'\nBB = 'KK'\n[tree]\nscript = '''{script}'''\n"
    )
}
fn standard(script: &str) -> String {
    text("", "BTN r2.5, BB c", "Ks 7h 2d", script)
}
fn parse(text: &str) -> Document {
    Document::parse(text, Path::new("input.toml")).unwrap()
}
fn config(text: &str) -> PostflopConfig {
    let doc = parse(text);
    let settings = Settings::parse(&doc.spot, &doc.solver, &doc.output).unwrap();
    lower(&doc.spot, &settings).unwrap()
}
fn pipeline() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}
fn game(config: &PostflopConfig) -> PostflopGame {
    try_build_postflop_game(config, pipeline()).unwrap()
}
fn actions(game: &PostflopGame, history: &str) -> Vec<String> {
    let node = game.node_by_history(history).unwrap();
    // Compare action amounts using the builder's history-token spelling.
    game.node_info[game.game.tree.tags[node as usize] as usize]
        .actions
        .iter()
        .map(|label| match label.as_str() {
            "fold" => "f".into(),
            "check" => "x".into(),
            "call" => "c".into(),
            other => format!(
                "r{}",
                other
                    .strip_prefix("bet ")
                    .or_else(|| other.strip_prefix("raise to "))
                    .unwrap()
            ),
        })
        .collect()
}

#[test]
fn sections_defaults_order_and_idempotence() {
    let doc = parse(&standard(""));
    let s = Settings::parse(&doc.spot, &doc.solver, &doc.output).unwrap();
    assert!(s.solver.iso_merging);
    assert_eq!(s.solver.storage, Storage::F32);
    assert_eq!(s.output.solution_streets, SolutionStreets::Full);
    assert_eq!(s.solver.stop.max_iterations, 1_000_000);
    assert_eq!(s.solver.stop.check_every, 25);
    assert_eq!(s.solver.stop.target, None);
    assert_eq!(s.solver.parallel.chance_depth, 2);
    assert_eq!(s.solver.parallel.min_children, 12);
    assert_eq!(
        s.solver.algorithm,
        Algorithm::Dcfr {
            alpha: 1.5,
            beta: 0.0,
            gamma: 3.0,
            pow4_reset: true
        }
    );
    let effective = doc.normalize(&P1Sections).unwrap();
    assert_eq!(effective, parse(&effective).normalize(&P1Sections).unwrap());
    let keys = [
        "iso_merging =",
        "storage =",
        "[solver.algorithm]",
        "schedule =",
        "alpha =",
        "beta =",
        "gamma =",
        "pow4_reset =",
        "[solver.stop]",
        "max_iterations =",
        "check_every =",
        "[solver.parallel]",
        "chance_depth =",
        "min_children =",
        "[output]",
        "solution_streets =",
    ];
    let offsets: Vec<_> = keys
        .iter()
        .map(|k| {
            effective
                .find(k)
                .unwrap_or_else(|| panic!("missing {k}: {effective}"))
        })
        .collect();
    assert!(offsets.windows(2).all(|w| w[0] < w[1]), "{effective}");
}

#[test]
fn strict_sections_codes_and_other_product_diagnostics() {
    for (extra, code, other) in [
        ("[solver]\nunknown = 1", Code::NLH002, false),
        ("[solver]\nseed = 0", Code::NLH002, true),
        ("[solver.abstraction]\nbuckets = 128", Code::NLH002, true),
        ("[solver.stop]\nmax_sweeps = 1", Code::NLH002, true),
        ("[output]\nprobability_encoding = 'u16'", Code::NLH002, true),
        (
            "[solver.algorithm]\nschedule = 'dcfr'\nunknown = 1",
            Code::NLH002,
            false,
        ),
        ("[solver.parallel]\nunknown = 1", Code::NLH002, false),
        ("[solver]\nstorage = 'u16'", Code::NLH003, false),
        ("[solver]\niso_merging = 'true'", Code::NLH003, false),
        (
            "[solver.algorithm]\nschedule = 'external-sampling-mccfr'",
            Code::NLH003,
            false,
        ),
        ("[solver.algorithm]\nalpha = inf", Code::NLH003, false),
        ("[solver.stop]\nmax_iterations = 0", Code::NLH003, false),
        ("[solver.stop]\ncheck_every = -1", Code::NLH003, false),
        ("[solver.stop]\ntarget = '0.01%prizes'", Code::NLH003, false),
        ("[solver.stop]\ntarget = 'nanbb'", Code::NLH003, false),
        ("[solver.parallel]\nchance_depth = -1", Code::NLH003, false),
        ("[solver.parallel]\nmin_children = 0", Code::NLH003, false),
        ("[output]\nsolution_streets = 'river'", Code::NLH003, false),
    ] {
        let doc = parse(&format!("{}\n{extra}", standard("")));
        let err = doc.normalize(&P1Sections).unwrap_err();
        assert_eq!(err.code, code, "{extra}: {err}");
        if other {
            assert!(err.message.contains("this spot is solved by P1"), "{err}");
        }
    }
    for schedule in ["vanilla", "cfr-plus", "dcfr", "linear-cfr", "hs-dcfr"] {
        let doc = parse(&format!(
            "{}\n[solver]\nstorage = 'i16'\n[solver.algorithm]\nschedule = '{schedule}'\n[solver.parallel]\nchance_depth = 0\n[output]\nsolution_streets = 'no-rivers'",
            standard("")
        ));
        let effective = doc.normalize(&P1Sections).unwrap();
        assert_eq!(effective, parse(&effective).normalize(&P1Sections).unwrap());
        if schedule == "hs-dcfr" {
            assert!(effective.contains("gamma0 = 30.0"));
        }
    }
}

#[test]
fn draft_example_lowering_and_sizes() {
    let draft = include_str!("../../../docs/plans/nlh-input-v1.jp.md");
    let example = draft
        .split("### P1:")
        .nth(1)
        .unwrap()
        .split("```toml\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let doc = parse(example);
    let settings = Settings::parse(&doc.spot, &doc.solver, &doc.output).unwrap();
    let c = lower(&doc.spot, &settings).unwrap();
    assert_eq!(c.pot, Chips(5500));
    assert_eq!(c.effective_stack, Chips(97_500));
    assert_eq!(c.min_bet, Chips(1000));
    assert_eq!(c.preflop_aggressor, Some(Player::P1));
    let r = c.streets.flop.nlh_rules.as_ref().unwrap();
    assert_eq!(r.players[Player::P0].position, "BB");
    assert_eq!(r.players[Player::P1].position, "BTN");
    assert_eq!(
        c.ranges[Player::P0].weights(),
        doc.spot.ranges[r.players[Player::P0].seat].range.weights()
    );
    // A river with the same starting pot tests resolution without a large flop allocation.
    let g = game(&config(&text(
        "",
        "BTN r2.5, BB c / BB x, BTN x / BB x, BTN x",
        "Ks 7h 2d Ac 9s",
        "river { replace bet [33, 75] replace raise [3x] }",
    )));
    assert_eq!(actions(&g, ""), ["x", "r1815", "r4125"]);
    assert_eq!(actions(&g, "r1815"), ["f", "c", "r5445"]);
}

#[test]
fn turn_start_previous_aggressor_and_earlier_rules_skipped() {
    let c = config(&text(
        "",
        "BTN r2.5, BB c / BB b1.8, BTN c",
        "Ks 7h 2d Ac",
        "preflop { force raise [a] } flop { force bet [a] } turn when cbet { add bet [1bb] }",
    ));
    assert_eq!(c.pot, Chips(9100));
    assert_eq!(c.effective_stack, Chips(95_700));
    assert_eq!(c.preflop_aggressor, Some(Player::P0));
    assert!(c.streets.flop.nlh_rules.as_ref().unwrap().rules.is_empty());
    let g = game(&c);
    assert_eq!(actions(&g, ""), ["x", "r1000"]);
    let estimate = memory_usage(&c);
    assert!(estimate.rule_hits.flop.is_empty());
    assert_eq!(estimate.rule_hits.turn, [true]);
    let checked = config(&text(
        "",
        "BTN r2.5, BB c / BB x, BTN x",
        "Ks 7h 2d Ac",
        "turn when cbet || donk { add bet [1bb] }",
    ));
    assert_eq!(checked.preflop_aggressor, None);
    assert_eq!(memory_usage(&checked).rule_hits.turn, [false]);
}

#[test]
fn effective_stack_allin_and_size_resolution() {
    let base = text(
        "[table.stacks_bb]\nBB = 20",
        "BTN r2.5, BB c / BB x, BTN x / BB x, BTN x",
        "Ks 7h 2d Ac 9s",
        "river { add bet [60%stack, 60%effective, a, 0.001bb, 1.234bb] }",
    );
    let c = config(&base);
    assert_eq!(c.effective_stack, Chips(17_500));
    assert_eq!(
        actions(&game(&c), ""),
        ["x", "r1000", "r1234", "r10500", "r17500"]
    );
    let threshold = base.replace("script =", "allin_threshold = 0.6\nscript =");
    assert_eq!(
        actions(&game(&config(&threshold)), ""),
        ["x", "r1000", "r1234", "r17500"]
    );
    let caps = base + "\n[tree.max_aggressive_actions]\nriver = 0";
    assert_eq!(actions(&game(&config(&caps)), ""), ["x"]);
    let huge = config(&text(
        "",
        "BTN r2.5, BB c / BB x, BTN x / BB x, BTN x",
        "Ks 7h 2d Ac 9s",
        "river { add bet [1bb] add raise [1000000000000] }",
    ));
    let g = game(&huge);
    assert_eq!(actions(&g, "r1000"), ["f", "c", "r97500"]);
    // The next raiser already has a street wager; adding a saturated size
    // must still clamp to the effective stack, rather than overflowing.
    let huge = config(&text(
        "",
        "BTN r2.5, BB c / BB x, BTN x / BB x, BTN x",
        "Ks 7h 2d Ac 9s",
        "river { add bet [1bb] } river when aggressions == 1 { add raise [2x] } river when aggressions == 2 { add raise [1000000000000] }",
    ));
    assert_eq!(actions(&game(&huge), "r1000r2000"), ["f", "c", "r97500"]);
}

#[test]
fn antes_bb_ante_and_straddles_are_in_the_starting_pot() {
    for (table, line, pot, stack) in [
        ("ante_bb = 0.1", "BTN r2.5, BB c", 6100, 97400),
        ("bb_ante_bb = 1", "BTN r2.5, BB c", 6500, 96500),
        ("straddles_bb = [2, 4]", "BTN r10, HJ c", 23500, 90000),
    ] {
        let raw = text(table, line, "Ks 7h 2d", "").replace(
            "BB = 'KK'",
            if line.contains("HJ c") {
                "HJ = 'KK'"
            } else {
                "BB = 'KK'"
            },
        );
        let c = config(&raw);
        assert_eq!(c.pot, Chips(pot));
        assert_eq!(c.effective_stack, Chips(stack));
        assert_eq!(c.min_bet, Chips(1000));
    }
}

#[test]
fn each_tree_variable_is_evaluated_at_p1_nodes() {
    let conditions = [
        "aggressions == 0",
        "raises == 0",
        "unopened",
        "players == 2",
        "position == \"BB\"",
        "!in_position",
        "spr > 17 && spr < 18",
        "pot == 5.5",
        "to_call == 0",
        "facing_pct == 0",
        "!cbet",
        "donk",
        "limpers == 0",
        "flats == 1",
        "!squeeze",
        "open_cold_calls == 0",
        "preflop_participant",
        "!in_position_to_last_aggressor",
        "last_preflop_aggressor_position == \"BTN\"",
        "board_cards == 3",
        "board_suits == 3",
        "board_ranks == 3",
        "straight_ranks == 1",
        "!paired",
        "!monotone",
        "!two_tone",
        "rainbow",
        "!flush_possible",
        "!straight_possible",
        "high_card == \"K\"",
        "low_card == \"2\"",
    ];
    assert_eq!(conditions.len(), TreeVar::ALL.len());
    for (&var, condition) in TreeVar::ALL.iter().zip(conditions) {
        assert!(condition.contains(var.name()));
        let script = format!("flop when {condition} {{ remove call }}");
        let estimate = memory_usage(&config(&standard(&script)));
        assert_eq!(estimate.rule_hits.flop, [true], "{var:?}: {condition}");
    }
}

#[test]
fn spr_uses_both_remaining_stacks_and_amount_variables_use_bb() {
    let c = config(&text(
        "",
        "BTN r2.5, BB c / BB x, BTN x / BB x, BTN x",
        "Ks 7h 2d Ac 9s",
        "river when unopened { add bet [2bb] } river when aggressions == 1 && position == \"BTN\" && spr < 12.74 && spr > 12.73 && pot == 7.5 && to_call == 2 && facing_pct > 26.66 && facing_pct < 26.67 { force call [] }",
    ));
    let g = game(&c);
    assert_eq!(actions(&g, "r2000"), ["c"]);
    let mut zero_pot = config(&standard("flop when spr > 1000000000 { remove call }"));
    zero_pot.pot = Chips::ZERO;
    assert_eq!(memory_usage(&zero_pot).rule_hits.flop, [true]);
}

#[test]
fn line_history_constants_and_limped_previous_aggressor() {
    let limped = config(&text(
        "",
        "BTN c, BB x",
        "Ks 7h 2d",
        "flop when limpers == 1 && flats == 0 && last_preflop_aggressor_position == \"\" && !cbet && !donk { remove call }",
    ));
    assert_eq!(limped.preflop_aggressor, None);
    assert_eq!(memory_usage(&limped).rule_hits.flop, [true]);
    let cold_call = config(&text(
        "",
        "BTN r2.5, SB c, BB r10, BTN c",
        "Ks 7h 2d",
        "flop when flats >= 1 && open_cold_calls == 1 && preflop_participant && last_preflop_aggressor_position == \"BB\" { remove call }",
    ));
    assert_eq!(memory_usage(&cold_call).rule_hits.flop, [true]);
}

#[test]
fn fold_check_call_rules_source_order_and_no_script() {
    let river = |script| {
        config(&text(
            "",
            "BTN r2.5, BB c / BB x, BTN x / BB x, BTN x",
            "Ks 7h 2d Ac 9s",
            script,
        ))
    };
    assert_eq!(memory_usage(&river("")).nodes, 3);
    let g = game(&river(
        "river when unopened { replace bet [1bb] remove check add check [] replace check [] force bet [1bb] } river when !unopened { remove fold replace call [] add fold [] force call [] }",
    ));
    assert_eq!(actions(&g, ""), ["r1000"]);
    assert_eq!(actions(&g, "r1000"), ["c"]);
    let g = game(&river(
        "river { add bet [1bb] remove bet replace check [] force check [] }",
    ));
    assert_eq!(actions(&g, ""), ["x"]);
    for script in [
        "river { remove check }",
        "river { force raise [1bb] }",
        "river { add bet [1bb] } river when !unopened { checkdown }",
        "river { add bet [1bb] } river when !unopened { force check [] }",
        "river { force bet [1bb] remove bet }",
    ] {
        let c = river(script);
        assert!(
            matches!(try_memory_usage(&c), Err(TreeBuildError::EmptyMenu { .. })),
            "{script}"
        );
        assert!(try_build_postflop_game(&c, pipeline()).is_err());
    }
    // An intermediate empty menu is legal when a later rule restores candidates.
    assert_eq!(
        actions(
            &game(&river("river { force raise [1bb] add check [] }")),
            ""
        ),
        ["x"]
    );
    let included = text(
        "",
        "BTN r2.5, BB c / BB x, BTN x / BB x, BTN x",
        "Ks 7h 2d Ac 9s",
        "river { remove raise }",
    )
    .replace("script =", "include_allin = true\nscript =");
    let g = game(&config(&included));
    assert_eq!(actions(&g, ""), ["x", "r97500"]);
    assert_eq!(actions(&g, "r97500"), ["f", "c"]);
}

#[test]
fn v1_and_scaled_legacy_trees_are_identical() {
    let c = config(&text(
        "",
        "BTN r2.5, BB c / BB x, BTN b1.8, BB c",
        "Ks 7h 2d Ac",
        "turn, river { replace bet [33, 75, 2bb] replace raise [3x] }",
    ));
    let script = Script::compile(
        "turn, river { replace bet [33, 75, 2000c] replace raise [3x] }",
        &Default::default(),
        &POSTFLOP,
    )
    .unwrap();
    let legacy = PostflopConfig {
        streets: hu_postflop::PerStreet {
            flop: StreetTree::default(),
            turn: StreetTree::from_script(Street::Turn, &script.rules, 3, false, None),
            river: StreetTree::from_script(Street::River, &script.rules, 3, false, None),
        },
        ..c.clone()
    };
    let new = game(&c);
    let old = build_postflop_game(&legacy, pipeline());
    assert_eq!(new.game.tree.nodes.len(), old.game.tree.nodes.len());
    assert_eq!(new.node_info.len(), old.node_info.len());
    for (a, b) in new.node_info.iter().zip(&old.node_info) {
        assert_eq!(a.history, b.history);
        assert_eq!(a.actions, b.actions);
        assert_eq!(a.contrib, b.contrib);
    }
    assert_eq!(memory_usage(&c).nodes, memory_usage(&legacy).nodes);
}

#[test]
fn lowering_rejects_overflow_and_wrong_product() {
    let doc = parse(&standard(""));
    let settings = Settings::parse(&doc.spot, &doc.solver, &doc.output).unwrap();
    let mut s = doc.spot;
    s.context.effective_stack = Some(nlh::MwChips(u32::MAX as u64));
    assert_eq!(lower(&s, &settings).err().unwrap().code, Code::NLH003);
    s.product = spot::Product::MultiwayPreflop;
    assert_eq!(lower(&s, &settings).err().unwrap().code, Code::NLH005);
    assert!(Settings::parse(&s, &doc.solver, &doc.output).is_err());
}
