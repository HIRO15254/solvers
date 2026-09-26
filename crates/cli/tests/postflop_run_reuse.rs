use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cards::{Chips, PerPlayer, Player};
use engine::{Dcfr, F32Storage, I16Storage, NodeKind, Solver, Storage};
use game::{ChipEv, NoRake, PayoffPipeline, PercentCapRake, RakeModel};
use holdem::{PerStreet, PostflopConfig, StreetTree, build_postflop_game};

const TREE: &str = "river {\nreplace bet [50, 100]\nreplace raise [50, 100]\n}";
const CONFIG: &str = r#"
schema = "solvers.postflop/v1"
[game]
board = "Ks Qs 7h 2d 3c"
oop_range = "AhAd,7c7d,JhJd"
ip_range = "AcAs,KhKd,JcJs"
pot = 5
effective_stack = 8
iso_merging = false
[game.tree]
kind = "script"
script = '''river {
replace bet [50, 100]
replace raise [50, 100]
}'''
[run]
iterations = 7
check_every = 3
threads = 1
"#;
const RAKE: &str = "\n[rake]\nkind = \"percent-cap\"\nrate = 0.125\ncap = 1.5\n";

fn ok(args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(args)
        .output()
        .expect("execute solvers");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn solve(config: &str, directory: &Path) -> PathBuf {
    let input = directory.join("input.toml");
    let run = directory.join("run");
    std::fs::write(&input, config).unwrap();
    ok(&[
        "solve",
        input.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);
    run
}

fn checkpoint_iterations(run: &Path) -> Vec<u64> {
    formats::read_events(&run.join(formats::RUN_EVENTS_FILE), 0)
        .unwrap()
        .0
        .into_iter()
        .filter_map(|event| match event.payload {
            formats::RunEventPayload::Checkpoint { sweeps } => Some(sweeps),
            _ => None,
        })
        .collect()
}

fn assert_matches_rebuilt_solver<S: Storage>(run: &Path, raked: bool) {
    let checkpoint = formats::read_checkpoint(&run.join("checkpoint.ckpt")).unwrap();
    let payload = formats::read_sol(&run.join("solution.sol")).unwrap();
    let rake = PercentCapRake {
        rate: 0.125,
        cap: 1.5,
        no_flop_no_drop: false,
    };
    let rake: &dyn RakeModel = if raked { &rake } else { &NoRake };
    // Rebuild with metadata enabled, independently of the CLI's lean solve
    // path. Restore must retain identical storage refs and all saved policies.
    let game = build_postflop_game(
        &PostflopConfig {
            board: "Ks Qs 7h 2d 3c"
                .split_whitespace()
                .map(|c| c.parse().unwrap())
                .collect(),
            ranges: PerPlayer::new(
                "AhAd,7c7d,JhJd".parse().unwrap(),
                "AcAs,KhKd,JcJs".parse().unwrap(),
            ),
            pot: Chips(5),
            effective_stack: Chips(8),
            streets: PerStreet {
                flop: StreetTree::pot_fractions(&[], &[], 2),
                turn: StreetTree::pot_fractions(&[], &[], 2),
                river: StreetTree::pot_fractions(&[0.5, 1.0], &[0.5, 1.0], 2),
            },
            iso_merging: false,
            ..Default::default()
        },
        PayoffPipeline {
            rake,
            utility: &ChipEv,
        },
    );
    assert!(game.node_info.len() > 1);
    let mut solver = Solver::<_, S>::new(game.game, Box::<Dcfr>::default(), Some(7));
    solver.restore_state(checkpoint.state).unwrap();
    assert_eq!(payload.meta.iterations, solver.iteration());
    let expl = solver.exploitability();
    for (player, offset) in [(Player::P0, 2.0), (Player::P1, 3.0)] {
        assert_eq!(payload.meta.expl[player.index()], expl[player]);
        assert_eq!(
            payload.meta.ev[player.index()],
            solver.expected_value(player) + offset
        );
    }
    for (id, node) in solver.game().tree.nodes.iter().enumerate() {
        if node.kind == NodeKind::Action {
            let saved = payload
                .blocks
                .iter()
                .find(|block| block.sref == node.aux)
                .unwrap();
            assert_eq!(
                saved.probs,
                formats::quantize_probs(&solver.average_strategy_at(id as u32))
            );
        }
    }
    let rows: Vec<formats::MetricsRow> = std::fs::read_to_string(run.join("progress.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let last = rows.last().unwrap();
    assert_eq!(last.iteration, payload.meta.iterations);
    assert_eq!([last.expl_p0, last.expl_p1], payload.meta.expl);
    assert_eq!(last.nash_conv, payload.meta.nash_conv);
}

#[test]
fn final_partial_chunk_reuses_checkpoint_and_preserves_results() {
    for storage in ["f32", "i16"] {
        for raked in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let config = CONFIG.replace("[run]", &format!("[run]\nstorage = \"{storage}\""));
            let config = format!("{config}{}", if raked { RAKE } else { "" });
            let run = solve(&config, directory.path());
            assert_eq!(checkpoint_iterations(&run), [3, 6, 7]);
            if storage == "f32" {
                assert_matches_rebuilt_solver::<F32Storage>(&run, raked);
            } else {
                assert_matches_rebuilt_solver::<I16Storage>(&run, raked);
            }
        }
    }
}

fn assert_compact_resume_matches_direct<S: Storage>(storage: &str) {
    let directory = tempfile::tempdir().unwrap();
    let config = CONFIG
        .replace("AcAs,KhKd,JcJs", "AcAs,KhKd")
        .replace("[run]", &format!("[run]\nstorage = \"{storage}\""));
    let direct = solve(&format!("{config}{RAKE}"), directory.path());
    let raw = std::fs::read_to_string(direct.join("run.toml")).unwrap();
    let parsed = cli::config::parse_solve_config(&raw).unwrap();
    assert_eq!(parsed.run.iterations, 7);
    assert!(parsed.run.target_nash_conv.is_none());
    assert!(parsed.run.max_time_secs.is_none());
    let cli::config::GameSection::Postflop {
        board,
        oop_range,
        ip_range,
        pot,
        effective_stack,
        iso_merging,
        min_bet,
        preflop_aggressor,
        tree,
    } = &parsed.game
    else {
        panic!("postflop fixture required");
    };
    let pf = cli::postflop_setup::build_postflop_config(
        board,
        oop_range,
        ip_range,
        *pot,
        *effective_stack,
        *iso_merging,
        *min_bet,
        tree.lower().unwrap(),
        preflop_aggressor,
    )
    .unwrap();
    let rake = cli::economics::build_rake(&parsed.rake).unwrap();
    let utility = cli::economics::build_utility(&parsed.utility).unwrap();
    let resumed = directory.path().join("resumed");
    std::fs::create_dir(&resumed).unwrap();
    std::fs::write(resumed.join("run.toml"), &raw).unwrap();

    // Produce a real intermediate state under the SAME planned total and
    // effective config as the independent direct run. This deterministic
    // checkpoint fixture exercises CLI resume, not OS signal delivery.
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            let game = build_postflop_game(
                &pf,
                PayoffPipeline {
                    rake: rake.as_ref(),
                    utility: utility.as_ref(),
                },
            );
            assert_eq!(game.game.tree.root_dims, PerPlayer::new(3, 2));
            assert!(!game.game.zero_sum);
            let mut partial = Solver::<_, S>::new(
                game.game,
                cli::postflop_setup::build_schedule(&parsed.algorithm),
                Some(parsed.run.iterations),
            );
            cli::postflop_setup::configure_solver(&mut partial, &parsed.run);
            partial.run(3);
            formats::write_checkpoint_ref(
                &resumed.join("checkpoint.ckpt"),
                formats::config_hash(raw.as_bytes()),
                &partial.state_ref(),
            )
            .unwrap();
        });
    let intermediate = formats::read_checkpoint(&resumed.join("checkpoint.ckpt")).unwrap();
    assert_eq!(intermediate.iteration, 3);
    let partial_bytes = std::fs::read(resumed.join("checkpoint.ckpt")).unwrap();
    assert_eq!(
        u16::from_le_bytes(partial_bytes[8..10].try_into().unwrap()),
        2
    );

    ok(&["resume", resumed.to_str().unwrap()]);
    assert_eq!(checkpoint_iterations(&resumed), [6, 7]);
    let direct_checkpoint = formats::read_checkpoint(&direct.join("checkpoint.ckpt")).unwrap();
    let resumed_checkpoint = formats::read_checkpoint(&resumed.join("checkpoint.ckpt")).unwrap();
    assert_eq!(direct_checkpoint.iteration, 7);
    assert_eq!(resumed_checkpoint, direct_checkpoint);
    // Raw equality also covers every float bit, including I16's per-node
    // regret/strategy scales, rather than only a normalized policy.
    assert_eq!(
        std::fs::read(resumed.join("checkpoint.ckpt")).unwrap(),
        std::fs::read(direct.join("checkpoint.ckpt")).unwrap()
    );

    let direct_sol = direct.join("solution.sol");
    let resumed_sol = resumed.join("solution.sol");
    for path in [&direct_sol, &resumed_sol] {
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(u16::from_le_bytes(bytes[8..10].try_into().unwrap()), 4);
    }
    let mut expected = formats::read_sol(&direct_sol).unwrap();
    let mut actual = formats::read_sol(&resumed_sol).unwrap();
    assert_eq!(actual.mode, formats::StreetsStored::Full);
    assert_eq!(actual.meta.storage, storage);
    assert_eq!(actual.meta.iterations, 7);
    assert_eq!(
        actual.meta.ev.map(f64::to_bits),
        expected.meta.ev.map(f64::to_bits)
    );
    assert_eq!(
        actual.meta.expl.map(f64::to_bits),
        expected.meta.expl.map(f64::to_bits)
    );
    assert_eq!(
        actual.meta.nash_conv.to_bits(),
        expected.meta.nash_conv.to_bits()
    );
    for (a, b) in actual.values.iter().zip(&expected.values) {
        assert_eq!(a.scale.to_bits(), b.scale.to_bits());
    }
    // Elapsed time is observational, unlike the stored strategy/value bytes.
    actual.meta.wall_secs = 0.0;
    expected.meta.wall_secs = 0.0;
    assert_eq!(actual, expected);

    // Recompute the quantized saved policy's quality independently of the
    // pre-save metadata and value blocks. Equality here is between the two
    // saved profiles, not a claim of zero quantization error versus live CFR.
    let direct_audit = cli::sol::audit_saved_full_profile(&direct_sol, 1).unwrap();
    let resumed_audit = cli::sol::audit_saved_full_profile(&resumed_sol, 1).unwrap();
    assert!(!direct_audit.zero_sum_terminal_utility);
    assert!(!resumed_audit.zero_sum_terminal_utility);
    assert_eq!(direct_audit.recomputed.profile, "stored_quantized");
    assert_eq!(resumed_audit.recomputed.profile, "stored_quantized");
    let a = direct_audit.recomputed;
    let b = resumed_audit.recomputed;
    for (a, b) in [(a.ev, b.ev), (a.br, b.br), (a.gains, b.gains)] {
        assert_eq!(a.map(f64::to_bits), b.map(f64::to_bits));
    }
    assert_eq!(a.nash_conv.to_bits(), b.nash_conv.to_bits());
}

#[test]
fn asymmetric_compact_f32_checkpoint_resume_matches_direct_and_saved_profile() {
    assert_compact_resume_matches_direct::<F32Storage>("f32");
}

#[test]
fn asymmetric_compact_i16_checkpoint_resume_matches_direct_and_saved_profile() {
    assert_compact_resume_matches_direct::<I16Storage>("i16");
}

#[test]
fn resumed_early_stop_evaluates_the_new_iteration() {
    let directory = tempfile::tempdir().unwrap();
    let config = CONFIG.replace("[run]", "[run]\ntarget_nash_conv = 999.0");
    let run = solve(&format!("{config}{RAKE}"), directory.path());
    assert_eq!(checkpoint_iterations(&run), [3]);
    let before = formats::read_sol(&run.join("solution.sol")).unwrap();
    assert_eq!(before.meta.iterations, 3);
    ok(&["resume", run.to_str().unwrap()]);
    assert_eq!(checkpoint_iterations(&run), [3, 6]);
    assert_matches_rebuilt_solver::<F32Storage>(&run, true);
    assert_eq!(
        formats::read_sol(&run.join("solution.sol"))
            .unwrap()
            .meta
            .iterations,
        6
    );
}

#[test]
fn exhausted_time_resume_keeps_the_same_profile_and_saves_once() {
    let directory = tempfile::tempdir().unwrap();
    let config = CONFIG.replace(
        "[run]",
        "[run]\ntarget_nash_conv = 999.0\nmax_time = \"1s\"",
    );
    let run = solve(&format!("{config}{RAKE}"), directory.path());
    let before = formats::read_sol(&run.join("solution.sol")).unwrap();
    let progress_path = run.join("progress.jsonl");
    let progress = std::fs::read_to_string(&progress_path).unwrap();
    let mut rows: Vec<formats::MetricsRow> = progress
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    rows.last_mut().unwrap().elapsed_secs = 1.0;
    let recorded = rows
        .iter()
        .map(|row| format!("{}\n", serde_json::to_string(row).unwrap()))
        .collect::<String>();
    std::fs::write(&progress_path, &recorded).unwrap();
    let fork = directory.path().join("fork");
    let output = ok(&[
        "resume",
        run.to_str().unwrap(),
        "--out",
        fork.to_str().unwrap(),
    ]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("max_time already reached"));
    assert_eq!(checkpoint_iterations(&fork), [before.meta.iterations]);
    assert_eq!(
        std::fs::read_to_string(fork.join("progress.jsonl")).unwrap(),
        recorded
    );
    let after = formats::read_sol(&fork.join("solution.sol")).unwrap();
    assert_eq!(after.meta.iterations, before.meta.iterations);
    assert_eq!(after.meta.ev, before.meta.ev);
    assert_eq!(after.meta.expl, before.meta.expl);
    assert_eq!(after.blocks, before.blocks);
    assert_eq!(after.values, before.values);
    assert_matches_rebuilt_solver::<F32Storage>(&fork, true);
}

#[test]
fn external_tree_is_inlined_before_hashing_and_survives_source_removal() {
    let directory = tempfile::tempdir().unwrap();
    let tree_path = directory.path().join("river.tree");
    std::fs::write(&tree_path, TREE).unwrap();
    let external = CONFIG.replace(&format!("script = '''{TREE}'''"), "source = \"river.tree\"");
    let input = directory.path().join("input.toml");
    std::fs::write(&input, &external).unwrap();
    let effective = directory.path().join("effective.toml");
    ok(&[
        "validate",
        input.to_str().unwrap(),
        "--write-effective",
        effective.to_str().unwrap(),
    ]);
    let run = solve(&external, directory.path());
    let saved = std::fs::read_to_string(run.join("run.toml")).unwrap();
    assert_eq!(saved, std::fs::read_to_string(&effective).unwrap());
    std::fs::write(&input, CONFIG).unwrap();
    ok(&[
        "validate",
        input.to_str().unwrap(),
        "--write-effective",
        effective.to_str().unwrap(),
    ]);
    assert_eq!(saved, std::fs::read_to_string(&effective).unwrap());
    let document: toml::Value = toml::from_str(&saved).unwrap();
    assert!(document["game"]["tree"].get("source").is_none());
    assert_eq!(document["game"]["tree"]["script"].as_str().unwrap(), TREE);
    let hash = formats::config_hash(saved.as_bytes());
    assert_ne!(hash, formats::config_hash(external.as_bytes()));
    let checkpoint = formats::read_checkpoint(&run.join("checkpoint.ckpt")).unwrap();
    assert_eq!(checkpoint.config_hash, hash);
    let before = formats::read_sol(&run.join("solution.sol")).unwrap();
    assert_eq!(before.config_toml, saved);
    let manifest = formats::RunManifest::read(&run).unwrap();
    assert_eq!(manifest.config_hash, formats::config_hash_hex(&hash));

    std::fs::remove_file(&tree_path).unwrap();
    std::fs::remove_file(&input).unwrap();
    let output = ok(&[
        "export",
        run.join("solution.sol").to_str().unwrap(),
        "strategy",
        "--node",
        "all",
    ]);
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!rows.as_array().unwrap().is_empty());
    let fork = directory.path().join("fork");
    ok(&[
        "resume",
        run.to_str().unwrap(),
        "--out",
        fork.to_str().unwrap(),
    ]);
    assert_eq!(checkpoint_iterations(&fork), [7]);
    let after = formats::read_sol(&fork.join("solution.sol")).unwrap();
    assert_eq!(after.meta.ev, before.meta.ev);
    assert_eq!(after.meta.expl, before.meta.expl);
    assert_eq!(after.blocks, before.blocks);
    assert_eq!(after.values, before.values);
    assert_matches_rebuilt_solver::<F32Storage>(&fork, false);
}
