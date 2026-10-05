//! User examples remain valid common inputs; small examples exercise both products.
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_user_example_validates_and_normalizes_idempotently() {
    for product in ["hu-postflop", "mw-preflop"] {
        for entry in std::fs::read_dir(root().join("examples").join(product)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let output = Command::new(env!("CARGO_BIN_EXE_solvers"))
                .args(["validate", path.to_str().unwrap(), "--format", "json"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
            let raw = std::fs::read_to_string(&path).unwrap();
            let doc = spot::Document::parse(&raw, &path).unwrap();
            let normalize = |doc: &spot::Document| {
                if product == "hu-postflop" {
                    doc.normalize(&hu_postflop::input::P1Sections).unwrap()
                } else {
                    doc.normalize(&mw_preflop::input::P2Sections).unwrap()
                }
            };
            let effective = normalize(&doc);
            assert_eq!(
                effective,
                normalize(&spot::Document::parse(&effective, &path).unwrap()),
                "{}",
                path.display()
            );
        }
    }
}

#[test]
fn small_p1_user_examples_solve() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "river_small",
        "turn_small",
        "tournament_icm",
        "river_script",
    ] {
        let config = root().join(format!("examples/hu-postflop/{name}.toml"));
        let run = dir.path().join(name);
        let output = Command::new(env!("CARGO_BIN_EXE_solvers"))
            .args([
                "solve",
                config.to_str().unwrap(),
                "--out",
                run.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(run.join("solution.sol").is_file());
    }
}

#[test]
fn small_p2_user_examples_solve_with_one_bucket_per_street() {
    use mw_preflop::abstraction::{FeatureHashAbstraction, FeatureHashParams};
    // With k=1 every live hand maps to bucket 0, exactly as production EHS².
    // Supply that mapping directly so this unit test needs no global cache build.
    for name in ["3max_smoke", "tournament_icm", "selectors_checkdown"] {
        let path = root().join(format!("examples/mw-preflop/{name}.toml"));
        let doc = spot::Document::parse(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
        let settings =
            mw_preflop::input::Settings::parse(&doc.spot, &doc.solver, &doc.output).unwrap();
        let input = mw_preflop::input::lower(&doc.spot, &settings).unwrap();
        let abstraction = FeatureHashAbstraction::new(FeatureHashParams {
            flop_buckets: 1,
            turn_buckets: 1,
            river_buckets: 1,
        })
        .unwrap();
        let effective = doc.normalize(&mw_preflop::input::P2Sections).unwrap();
        let mut session =
            mw_preflop::input::build_session(input, abstraction, effective, None).unwrap();
        session.solver.run_sweeps(2).unwrap();
        assert_eq!(session.solver.metrics().sweeps, 2, "{name}");
    }
}

#[test]
fn fold_rake_refunds_the_uncalled_bet_before_charging() {
    use hu_postflop::game::{RakeModel, TerminalDescriptor, TerminalKind};
    use nlh::{Chips, PerPlayer, Player, Street};
    let path = root().join("crates/cli/tests/fixtures/postflop_pio_tree.toml");
    let doc = spot::Document::parse(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
    let payoff = hu_postflop::input::NlhPayoff::new(&doc.spot).unwrap();
    let terminal = TerminalDescriptor {
        kind: TerminalKind::Fold { folder: Player::P0 },
        street: Street::Flop,
        pot: Chips(29400),
        contrib: PerPlayer::new(Chips(10500), Chips(18900)),
        stacks_before: PerPlayer::new(Chips(205500), Chips(205500)),
    };
    assert_eq!(payoff.rake.rake(&terminal) / 1000.0, 1.05);
}
