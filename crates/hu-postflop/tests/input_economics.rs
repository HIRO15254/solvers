use hu_engine::{Dcfr, F32Storage, Solver};
use hu_postflop::game::{RakeModel, TerminalDescriptor, TerminalKind, UtilityModel};
use hu_postflop::input::{
    NlhPayoff, P1Sections, Settings, Storage, check_memory_limit, lower, physical_memory_bytes,
    resolve_memory_limit, resolve_target,
};
use hu_postflop::{MemoryEstimate, try_build_postflop_game, try_memory_usage};
use nlh::{Chips, PerPlayer, Player, Street};
use spot::{Code, Document};
use std::path::Path;

fn source(economics: &str, tree: &str) -> String {
    format!(
        "schema = 'solvers.nlh/v1'\n[table]\nplayers = 6\n[table.stacks_bb]\nBTN = 100\nSB = 80\nBB = 60\nUTG = 100\nHJ = 100\nCO = 100\n{economics}\n[spot]\nline = 'BTN r2.5, BB c / BB x, BTN x / BB x, BTN x'\nboard = 'Ks 7h 2d 3c 4s'\n[ranges]\nBTN = 'AA'\nBB = 'QQ'\n[tree]\nscript = '''{tree}'''\n"
    )
}
fn document(text: &str) -> Document {
    Document::parse(text, Path::new("test.toml")).unwrap()
}
fn settings(doc: &Document) -> Settings {
    Settings::parse(&doc.spot, &doc.solver, &doc.output).unwrap()
}
fn terminal(doc: &Document, bets: (u32, u32), kind: TerminalKind) -> TerminalDescriptor {
    let config = lower(&doc.spot, &settings(doc)).unwrap();
    let shares = PerPlayer::new(
        config.starting_share(Player::P0),
        config.starting_share(Player::P1),
    );
    TerminalDescriptor {
        kind,
        street: Street::River,
        pot: config.pot + Chips(bets.0 + bets.1),
        contrib: PerPlayer::new(
            shares[Player::P0] + Chips(bets.0),
            shares[Player::P1] + Chips(bets.1),
        ),
        stacks_before: shares.map(|s| config.effective_stack + s),
    }
}
fn rake_source(when: &str, rounding: &str) -> String {
    format!("[economics.rake]\nrate = 0.05\nwhen = '{when}'\nrounding = '{rounding}'")
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-10, "{a} != {b}");
}

#[test]
fn section_type_errors_have_full_paths_and_nlh002() {
    for (extra, key) in [
        ("[solver]\niso_merging = 'true'", "solver.iso_merging"),
        ("[solver]\nstorage = 1", "solver.storage"),
        ("[solver]\nstop = []", "solver.stop"),
        ("[solver]\nalgorithm = false", "solver.algorithm"),
        ("[solver]\nparallel = 1", "solver.parallel"),
        (
            "[solver.algorithm]\nschedule = 1",
            "solver.algorithm.schedule",
        ),
        (
            "[solver.algorithm]\nalpha = '1.5'",
            "solver.algorithm.alpha",
        ),
        ("[solver.algorithm]\nbeta = []", "solver.algorithm.beta"),
        (
            "[solver.algorithm]\ngamma = false",
            "solver.algorithm.gamma",
        ),
        (
            "[solver.algorithm]\npow4_reset = 1",
            "solver.algorithm.pow4_reset",
        ),
        (
            "[solver.algorithm]\nschedule = 'hs-dcfr'\ngamma0 = '30'",
            "solver.algorithm.gamma0",
        ),
        ("[solver.stop]\ntarget = 1", "solver.stop.target"),
        (
            "[solver.stop]\nmax_iterations = 1.5",
            "solver.stop.max_iterations",
        ),
        (
            "[solver.stop]\ncheck_every = '25'",
            "solver.stop.check_every",
        ),
        (
            "[solver.parallel]\nchance_depth = '2'",
            "solver.parallel.chance_depth",
        ),
        (
            "[solver.parallel]\nmin_children = false",
            "solver.parallel.min_children",
        ),
        ("[output]\nsolution_streets = []", "output.solution_streets"),
    ] {
        let doc = document(&format!("{}\n{extra}", source("", "")));
        let err = doc.normalize(&P1Sections).unwrap_err();
        assert_eq!(err.code, Code::NLH002, "{extra}: {err}");
        assert_eq!(err.key.as_deref(), Some(key), "{extra}");
    }
}

#[test]
fn section_value_errors_have_full_paths_and_nlh003() {
    for (extra, key) in [
        ("[solver]\nstorage = 'u16'", "solver.storage"),
        ("[solver.stop]\ncheck_every = 0", "solver.stop.check_every"),
        (
            "[solver.stop]\nmax_iterations = -1",
            "solver.stop.max_iterations",
        ),
        ("[solver.algorithm]\nalpha = inf", "solver.algorithm.alpha"),
        ("[solver.algorithm]\nbeta = nan", "solver.algorithm.beta"),
        ("[solver.algorithm]\ngamma = -inf", "solver.algorithm.gamma"),
        (
            "[solver.algorithm]\nschedule = 'hs-dcfr'\ngamma0 = inf",
            "solver.algorithm.gamma0",
        ),
        (
            "[solver.parallel]\nchance_depth = -1",
            "solver.parallel.chance_depth",
        ),
        (
            "[solver.parallel]\nchance_depth = 4294967296",
            "solver.parallel.chance_depth",
        ),
        (
            "[solver.parallel]\nmin_children = 0",
            "solver.parallel.min_children",
        ),
        (
            "[output]\nsolution_streets = 'river'",
            "output.solution_streets",
        ),
    ] {
        let doc = document(&format!("{}\n{extra}", source("", "")));
        let err = doc.normalize(&P1Sections).unwrap_err();
        assert_eq!(err.code, Code::NLH003, "{extra}: {err}");
        assert_eq!(err.key.as_deref(), Some(key), "{extra}");
    }
}

#[test]
fn targets_resolve_in_bb_or_prizes_and_no_default() {
    let cash = document(&source("", ""));
    assert_eq!(settings(&cash).solver.stop.target, None);
    close(resolve_target(&cash.spot, "0.3%pot").unwrap(), 0.0165);
    close(resolve_target(&cash.spot, "0.05bb").unwrap(), 0.05);
    let icm = document(&source(
        "[economics]\nkind = 'tournament'\npayouts = [100,60,40,20,10,0]",
        "",
    ));
    close(resolve_target(&icm.spot, "0.01%prizes").unwrap(), 0.023);
    for (doc, target) in [(&cash, "0.01%prizes"), (&icm, "1bb"), (&icm, "1%pot")] {
        assert_eq!(
            resolve_target(&doc.spot, target).unwrap_err().code,
            Code::NLH003
        );
    }
    for target in [
        "0bb", "0.0bb", "0%pot", "-1bb", "+1bb", "1e-3bb", ".1bb", "1.bb", "1..2bb", " 1bb",
        "1 bb", "NaNbb", "infbb",
    ] {
        let doc = document(&format!(
            "{}\n[solver.stop]\ntarget = '{target}'",
            source("", "")
        ));
        let err = doc.normalize(&P1Sections).unwrap_err();
        assert_eq!(err.code, Code::NLH003, "{target}");
        assert_eq!(err.key.as_deref(), Some("solver.stop.target"));
    }
    let doc = document(&format!(
        "{}\n[solver.stop]\ntarget = '1.25bb'",
        source("", "")
    ));
    assert_eq!(settings(&doc).solver.stop.target.as_deref(), Some("1.25bb"));
}

#[test]
fn rake_excludes_uncalled_wager_and_uses_table_hand_facts() {
    let doc = document(&source(
        &rake_source(
            "flop_dealt && players_dealt == 6 && players_saw_flop == 2",
            "down",
        ),
        "",
    ));
    let models = NlhPayoff::new(&doc.spot).unwrap();
    close(
        models
            .rake
            .rake(&terminal(&doc, (1000, 1000), TerminalKind::Showdown)),
        375.0,
    );
    let fold = terminal(&doc, (1000, 0), TerminalKind::Fold { folder: Player::P1 });
    close(models.rake.rake(&fold), 275.0); // 5.5 BB, not the old family's 6.5 BB.
    let reported = models.pipeline().bake(&fold).win_p0;
    let offset = models.ev_offset();
    close(reported[Player::P0] + offset[Player::P0], 5.225);
    close(reported[Player::P1] + offset[Player::P1], 0.0);
    for (when, kind, expected) in [
        ("showdown", TerminalKind::Showdown, 275.0),
        ("showdown", TerminalKind::Fold { folder: Player::P0 }, 0.0),
        ("won_without_showdown", TerminalKind::Showdown, 0.0),
        (
            "won_without_showdown",
            TerminalKind::Fold { folder: Player::P0 },
            275.0,
        ),
        ("players_dealt == 2", TerminalKind::Showdown, 0.0),
        ("!flop_dealt", TerminalKind::Showdown, 0.0),
    ] {
        let doc = document(&source(&rake_source(when, "down"), ""));
        close(
            NlhPayoff::new(&doc.spot)
                .unwrap()
                .rake
                .rake(&terminal(&doc, (0, 0), kind)),
            expected,
        );
    }
}

#[test]
fn rake_rounds_before_capping_on_milli_bb_grid() {
    for (rounding, expected) in [("down", 55.0), ("nearest", 56.0), ("up", 56.0)] {
        let doc = document(&source(
            &format!("[economics.rake]\nrate = 0.0101\nrounding = '{rounding}'"),
            "",
        ));
        close(
            NlhPayoff::new(&doc.spot).unwrap().rake.rake(&terminal(
                &doc,
                (0, 0),
                TerminalKind::Showdown,
            )),
            expected,
        );
    }
    let doc = document(&source("[economics.rake]\nrate = 0.05\ncap_bb = 0.2", ""));
    close(
        NlhPayoff::new(&doc.spot).unwrap().rake.rake(&terminal(
            &doc,
            (1000, 1000),
            TerminalKind::Showdown,
        )),
        200.0,
    );
}

#[test]
fn actual_contributions_and_dead_money_cancel_from_reported_chip_ev() {
    let doc = document(&source("", ""));
    let models = NlhPayoff::new(&doc.spot).unwrap();
    let t = terminal(&doc, (1000, 1000), TerminalKind::Showdown);
    let baked = models.pipeline().bake(&t);
    let offset = models.ev_offset();
    close(offset[Player::P0], 2.5);
    close(offset[Player::P1], 2.5);
    assert!(!models.pipeline().is_zero_sum()); // SB's 0.5 BB is dead money.
    for u in [baked.win_p0, baked.tie, baked.win_p1] {
        close(
            u[Player::P0] + u[Player::P1] + offset[Player::P0] + offset[Player::P1],
            5.5,
        );
    }
    // The builder has equal effective stacks; utility must retain the actual
    // 40 BB difference in behind stacks.
    let config = lower(&doc.spot, &settings(&doc)).unwrap();
    let behind = config.effective_stack.as_f64();
    assert_eq!(
        models.utility.utility(&PerPlayer::new(behind, behind)),
        PerPlayer::new(57.5, 97.5)
    );
}

#[test]
fn unequal_actual_contributions_and_ante_dead_money_set_the_ev_shift() {
    for (extra, oop_contribution, ip_contribution, pot) in [
        ("bb_ante_bb = 1", 3.5, 2.5, 6.5),
        ("ante_bb = 0.1", 2.6, 2.6, 6.1),
    ] {
        let text = source("", "").replace("players = 6", &format!("players = 6\n{extra}"));
        let doc = document(&text);
        let models = NlhPayoff::new(&doc.spot).unwrap();
        let offset = models.ev_offset();
        close(offset[Player::P0], oop_contribution);
        close(offset[Player::P1], ip_contribution);
        let baked = models
            .pipeline()
            .bake(&terminal(&doc, (1000, 1000), TerminalKind::Showdown));
        for u in [baked.win_p0, baked.tie, baked.win_p1] {
            close(
                u[Player::P0] + u[Player::P1] + offset[Player::P0] + offset[Player::P1],
                pot,
            );
        }
        assert!(!models.pipeline().is_zero_sum());
    }
}

#[test]
fn two_player_free_chip_ev_retains_the_zero_sum_shortcut() {
    let doc = document(
        "schema = 'solvers.nlh/v1'\n[table]\nplayers = 2\nstack_bb = 100\n[spot]\nline = 'BTN r2.5, BB c'\nboard = 'Ks 7h 2d'\n[ranges]\nBTN = 'AA'\nBB = 'QQ'",
    );
    let models = NlhPayoff::new(&doc.spot).unwrap();
    assert!(models.pipeline().is_zero_sum());
    let baked = models
        .pipeline()
        .bake(&terminal(&doc, (1000, 1000), TerminalKind::Showdown));
    for u in [baked.win_p0, baked.tie, baked.win_p1] {
        close(u[Player::P0] + u[Player::P1], 0.0);
    }
}

#[test]
fn chip_ev_identity_on_small_solved_spots() {
    for (script, expected_rake) in [
        ("", 0.275),
        (
            "river when unopened { force bet [1bb] } river when !unopened { force call [] }",
            0.375,
        ),
        (
            "river when unopened { force bet [1bb] } river when !unopened { force fold [] }",
            0.275,
        ),
    ] {
        for rake in [false, true] {
            let doc = document(&source(
                if rake {
                    "[economics.rake]\nrate = 0.05"
                } else {
                    ""
                },
                script,
            ));
            let config = lower(&doc.spot, &settings(&doc)).unwrap();
            let models = NlhPayoff::new(&doc.spot).unwrap();
            let game = try_build_postflop_game(&config, models.pipeline()).unwrap();
            let mut solver =
                Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(4));
            solver.run(4);
            let offset = models.ev_offset();
            let sum = solver.expected_value(Player::P0)
                + solver.expected_value(Player::P1)
                + offset[Player::P0]
                + offset[Player::P1];
            let expected = 5.5 - if rake { expected_rake } else { 0.0 };
            assert!(
                (sum - expected).abs() < 1e-6,
                "f32 evaluator: {sum} != {expected}"
            );
        }
    }
}

#[test]
fn memory_auto_explicit_boundary_and_overflow() {
    assert_eq!(resolve_memory_limit(None, 100), 80);
    assert_eq!(resolve_memory_limit(None, 9), 7);
    assert_eq!(resolve_memory_limit(None, 0), 0);
    assert_eq!(
        resolve_memory_limit(None, u64::MAX),
        ((u64::MAX as u128) * 4 / 5) as u64
    );
    assert_eq!(resolve_memory_limit(Some(123), 0), 123);
    let estimate = MemoryEstimate {
        f32_bytes: 100,
        i16_bytes: 60,
        ..Default::default()
    };
    check_memory_limit(&estimate, Storage::F32, 100).unwrap();
    check_memory_limit(&estimate, Storage::I16, 60).unwrap();
    let error = check_memory_limit(&estimate, Storage::F32, 99).unwrap_err();
    assert_eq!((error.required, error.limit), (100, 99));
    assert!(error.to_string().contains("resource limit"));
    let doc = document(&source("", ""));
    let config = lower(&doc.spot, &settings(&doc)).unwrap();
    let estimate = try_memory_usage(&config).unwrap();
    assert!(estimate.save_bytes > 0);
    assert!(estimate.compression_bytes > 0);
    let total = estimate.f32_bytes + estimate.save_bytes + estimate.compression_bytes;
    check_memory_limit(&estimate, Storage::F32, total).unwrap();
    assert!(check_memory_limit(&estimate, Storage::F32, estimate.f32_bytes).is_err());
    assert!(check_memory_limit(&estimate, Storage::F32, 1).is_err());
}

#[test]
fn physical_ram_query_returns_positive_bytes() {
    assert!(physical_memory_bytes().unwrap() > 0);
}
