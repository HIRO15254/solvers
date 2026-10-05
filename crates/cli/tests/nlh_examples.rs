//! M5 numerical acceptance on complete example trees. Run ignored cases in release.
use std::path::Path;

use cli::{config::GameSection, economics, postflop_setup};
use hu_engine::{F32Storage, NodeKind, ParConfig, Solver};
use hu_postflop::{
    build_postflop_game,
    game::{PayoffPipeline, UtilityModel},
    input,
};
use nlh::Player;

// Unit conversion before f32 storage avoids amplifying roundoff by solving
// the legacy game with payoffs 1000 times larger. The raw-scale acceptance
// measurement below records that distinction instead of hiding it.
struct BbChipEv;
impl UtilityModel for BbChipEv {
    fn utility(&self, stacks: &nlh::PerPlayer<f64>) -> nlh::PerPlayer<f64> {
        stacks.map(|s| s / 1000.0)
    }
    fn is_zero_sum_affine(&self) -> bool {
        true
    }
}

fn compare(name: &str, icm: bool, raw_scale: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let new_path = root.join(format!("examples/nlh/{name}.toml"));
    let raw = std::fs::read_to_string(&new_path).unwrap();
    let mut document = spot::Document::parse(&raw, &new_path).unwrap();
    // The cash Pio comparison intentionally removes rake: v1 refunds the
    // uncalled bet before rake, unlike legacy fold terminals (tested below).
    if name == "postflop_pio_tree" {
        let mut value: toml::Value = toml::from_str(&raw).unwrap();
        value["economics"].as_table_mut().unwrap().remove("rake");
        document = spot::Document::parse(&toml::to_string(&value).unwrap(), &new_path).unwrap();
    }
    let settings =
        input::Settings::parse(&document.spot, &document.solver, &document.output).unwrap();
    let new_config = input::lower(&document.spot, &settings).unwrap();
    let old_name = if icm { "postflop_pio_tree" } else { name };
    let old_path = root.join(format!("examples/{old_name}.toml"));
    let old =
        cli::config::parse_solve_config_at(&std::fs::read_to_string(&old_path).unwrap(), &old_path)
            .unwrap();
    let GameSection::Postflop {
        board,
        oop_range,
        ip_range,
        pot,
        effective_stack,
        iso_merging,
        min_bet,
        preflop_aggressor,
        tree,
    } = old.game
    else {
        panic!("postflop fixture")
    };
    // Scale the original fixture before lowering: 1 BB = 1000 legacy chips.
    let old_config = postflop_setup::build_postflop_config(
        &board,
        &oop_range,
        &ip_range,
        pot * 1000,
        effective_stack * 1000,
        iso_merging,
        min_bet * 1000,
        tree.lower().unwrap(),
        &preflop_aggressor,
    )
    .unwrap();
    assert_eq!(new_config.pot, old_config.pot);
    assert_eq!(new_config.effective_stack, old_config.effective_stack);
    assert_eq!(new_config.min_bet, old_config.min_bet);
    let payoff = input::NlhPayoff::new(&document.spot).unwrap();
    let old_utility = if icm {
        let value: toml::Value = raw.parse().unwrap();
        let economics = &value["economics"];
        // Folded table players belong in the legacy outside field too.
        let mut field = document
            .spot
            .context
            .seats
            .iter()
            .filter(|s| s.folded)
            .map(|s| (s.starting_stack - s.total_contribution).0 as f64)
            .collect::<Vec<_>>();
        field.extend(
            economics["outside_field_bb"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    s.as_float()
                        .unwrap_or_else(|| s.as_integer().unwrap() as f64)
                        * 1000.0
                }),
        );
        let utility = cli::config::UtilitySection::TournamentIcm {
            outside_field: field
                .into_iter()
                .enumerate()
                .map(|(i, stack_bb)| cli::config::OutsidePlayerSection {
                    name: i.to_string(),
                    stack_bb,
                })
                .collect(),
            payouts: economics["payouts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    s.as_float()
                        .unwrap_or_else(|| s.as_integer().unwrap() as f64)
                })
                .collect(),
            samples: 100_000,
            seed: 0,
        };
        economics::build_utility(&utility).unwrap()
    } else if raw_scale {
        economics::build_utility(&cli::config::UtilitySection::ChipEv).unwrap()
    } else {
        Box::new(BbChipEv) as Box<dyn UtilityModel>
    };
    let old_game = build_postflop_game(
        &old_config,
        PayoffPipeline {
            rake: &hu_postflop::game::NoRake,
            utility: old_utility.as_ref(),
        },
    );
    let new_game = build_postflop_game(&new_config, payoff.pipeline());
    assert_eq!(
        old_game.game.tree.nodes.len(),
        new_game.game.tree.nodes.len()
    );
    for (a, b) in old_game.node_info.iter().zip(&new_game.node_info) {
        assert_eq!(a.history, b.history);
        assert_eq!(a.actions, b.actions);
        assert_eq!(a.contrib, b.contrib);
    }
    let nodes = new_game.game.tree.nodes.len();
    let iterations = if name == "river_small" {
        200
    } else if name == "turn_small" {
        32
    } else {
        4
    };
    let mut a = Solver::<_, F32Storage>::new(
        old_game.game,
        postflop_setup::build_schedule(&old.algorithm),
        Some(iterations),
    );
    let mut b = Solver::<_, F32Storage>::new(
        new_game.game,
        postflop_setup::build_schedule(&old.algorithm),
        Some(iterations),
    );
    // Fixed reduction order; full example ranges, board fan-out and rules.
    let par = ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    };
    a.set_par(par);
    b.set_par(par);
    a.run(iterations);
    b.run(iterations);
    let mut strategy_max = 0.0f64;
    for (id, node) in a.game().tree.nodes.iter().enumerate() {
        if node.kind == NodeKind::Action {
            for (x, y) in a
                .average_strategy_at(id as u32)
                .iter()
                .zip(b.average_strategy_at(id as u32))
            {
                strategy_max = strategy_max.max((*x as f64 - y as f64).abs());
            }
        }
    }
    let scale = if raw_scale { 1000.0 } else { 1.0 };
    let ev_a = postflop_setup::subgame_ev(
        nlh::PerPlayer::new(a.expected_value(Player::P0), a.expected_value(Player::P1)),
        postflop_setup::subgame_ev_offset(&old_config, old_utility.as_ref()),
    )
    .map(|v| v / scale);
    let ev_b = postflop_setup::subgame_ev(
        nlh::PerPlayer::new(b.expected_value(Player::P0), b.expected_value(Player::P1)),
        payoff.ev_offset(),
    );
    let expl_a = a.exploitability();
    let expl_b = b.exploitability();
    let nash_a = (expl_a[Player::P0] + expl_a[Player::P1]) / scale;
    let nash_b = expl_b[Player::P0] + expl_b[Player::P1];
    let relative = |x: f64, y: f64| (x - y).abs() / x.abs().max(y.abs()).max(f64::MIN_POSITIVE);
    let ev_relative = Player::BOTH
        .into_iter()
        .map(|p| relative(ev_a[p], ev_b[p]))
        .fold(0.0, f64::max);
    let nash_relative = relative(nash_a, nash_b);
    println!(
        "{name}: raw_scale={raw_scale} iterations={iterations} nodes={nodes} strategy_max_abs={strategy_max:.12e} ev_relative={ev_relative:.12e} nash_relative={nash_relative:.12e} EV_old={ev_a:?} EV_v1={ev_b:?} NashConv_old={nash_a:.12e} NashConv_v1={nash_b:.12e}"
    );
    if raw_scale {
        assert!(
            ev_relative < 1e-4 && nash_relative < 0.1,
            "raw f32 scale drift exceeds the recorded envelope"
        );
        return;
    }
    assert!(strategy_max < 1e-4, "strategy float noise {strategy_max}");
    assert!(ev_relative < 1e-9, "EV relative {ev_relative}");
    assert!(nash_relative < 1e-9, "NashConv relative {nash_relative}");
}

#[test]
fn river_small_matches_scaled_legacy() {
    compare("river_small", false, false);
}
#[test]
fn raw_milli_bb_payoffs_record_f32_scale_drift() {
    compare("river_small", false, true);
}
#[test]
#[ignore = "full turn tree; release numerical acceptance"]
fn turn_small_matches_scaled_legacy() {
    compare("turn_small", false, false);
}
#[test]
#[ignore = "full flop tree and >1 GiB storage per solver; run release serially"]
fn three_bet_matches_scaled_legacy() {
    compare("3betpot_fast", false, false);
}
#[test]
#[ignore = "full flop tree and >1 GiB storage per solver; run release serially"]
fn srp_matches_scaled_legacy() {
    compare("postflop_srp20", false, false);
}
#[test]
#[ignore = "full Pio tree; release numerical acceptance"]
fn pio_no_rake_matches_scaled_legacy() {
    compare("postflop_pio_tree", false, false);
}
#[test]
#[ignore = "full Pio tree with exact ICM; release numerical acceptance"]
fn pio_icm_matches_scaled_legacy() {
    compare("postflop_pio_icm", true, false);
}

#[test]
fn pio_fold_rake_returns_uncalled_bet_first() {
    use hu_postflop::game::{RakeModel, TerminalDescriptor, TerminalKind};
    use nlh::{Chips, PerPlayer, Street};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples/nlh/postflop_pio_tree.toml");
    let doc = spot::Document::parse(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
    let new = input::NlhPayoff::new(&doc.spot).unwrap();
    let old = economics::GenericRake::compile(
        0.05,
        Some(6000.0),
        "flop_dealt",
        Default::default(),
        Default::default(),
        1.0,
    )
    .unwrap();
    let t = TerminalDescriptor {
        kind: TerminalKind::Fold { folder: Player::P0 },
        street: Street::Flop,
        pot: Chips(29400),
        contrib: PerPlayer::new(Chips(10500), Chips(18900)),
        stacks_before: PerPlayer::new(Chips(205500), Chips(205500)),
    };
    assert_eq!(old.rake(&t) / 1000.0, 1.47);
    assert_eq!(new.rake.rake(&t) / 1000.0, 1.05);
    println!(
        "Pio fold after bet 8.4 BB: legacy rake=1.47 BB; v1 rake=1.05 BB; delta=0.42 BB (uncalled wager refunded)"
    );
}
