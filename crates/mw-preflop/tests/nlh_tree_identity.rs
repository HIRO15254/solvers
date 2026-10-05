//! V1 public-tree regressions. History-node counts retain the values proven
//! equal to the former implementations by the M5/M6 exhaustive comparisons.
use mw_preflop::BettingState;
use mw_preflop::betting::BettingMenu;
use mw_preflop::config::BettingConfig;
use mw_preflop::input::{P2Sections, Settings, lower};
use std::collections::HashMap;
use std::path::Path;

fn lowered(raw: &str) -> mw_preflop::input::Lowered {
    let doc = spot::Document::parse(raw, Path::new("fixture.toml")).unwrap();
    let settings = Settings::parse(&doc.spot, &doc.solver, &doc.output).unwrap();
    lower(&doc.spot, &settings).unwrap()
}

/// Hash integer public states and their ordered action menus; no private-card
/// sampling or floating-point solver output is included. Cache keys compare
/// complete states, so a hash collision cannot skip a differing suffix.
fn walk(
    state: BettingState,
    betting: &BettingConfig,
    cache: &mut HashMap<Vec<u8>, (u64, [u8; 32])>,
) -> (u64, [u8; 32]) {
    let key = postcard::to_stdvec(&state).unwrap();
    if let Some(identity) = cache.get(&key) {
        return *identity;
    }
    let actions = state.legal_actions(betting).unwrap();
    let mut hash = blake3::Hasher::new();
    hash.update(&key);
    hash.update(&postcard::to_stdvec(&actions).unwrap());
    let mut count = 1;
    for action in &actions {
        let mut next = state.clone();
        next.apply_from_actions(action.clone(), &actions, betting)
            .unwrap();
        let (nodes, suffix) = walk(next, betting, cache);
        count += nodes;
        hash.update(&suffix);
    }
    let identity = (count, *hash.finalize().as_bytes());
    if !actions.is_empty() {
        cache.insert(key, identity);
    }
    identity
}

fn identity(raw: &str) -> (u64, String) {
    let game = lowered(raw).game.validated().unwrap();
    let root = BettingState::from_config(&game).unwrap();
    let (nodes, hash) = walk(root, &game.betting, &mut HashMap::new());
    (nodes, blake3::Hash::from(hash).to_hex().to_string())
}

fn check(raw: &str, expected_nodes: u64, expected_hash: &str) {
    let doc = spot::Document::parse(raw, Path::new("fixture.toml")).unwrap();
    let effective = doc.normalize(&P2Sections).unwrap();
    assert_eq!(
        spot::Document::parse(&effective, Path::new("fixture.toml"))
            .unwrap()
            .normalize(&P2Sections)
            .unwrap(),
        effective
    );
    let (nodes, hash) = identity(raw);
    assert_eq!(nodes, expected_nodes);
    println!("nodes={nodes} hash={hash}");
    assert_eq!(hash, expected_hash);
}

macro_rules! regression {
    ($name:ident, $file:literal, $count:literal, $hash:literal) => {
        #[test]
        fn $name() {
            check(include_str!($file), $count, $hash);
        }
    };
}
regression!(
    smoke_3max,
    "fixtures/preflop_multiway_v1_3max_smoke.toml",
    160,
    "23f789ee7a21c9cecf4f957c215a2a19034f48679e7c82820247dfb1bbaefdb9"
);
regression!(
    default_6max,
    "fixtures/preflop_multiway_v1_default.toml",
    145590,
    "76253a4ad5dc896c8e2edde376c40de38ac892f13914566b87c6cb08ba53205e"
);
regression!(
    production_smoke,
    "fixtures/preflop_multiway_v1_production_smoke.toml",
    13,
    "26ce1eb0decc1cdd440009fa1b01fe584cd4781fe5b6a7addea2949e8dd5b961"
);
regression!(
    bench_3max_2bb,
    "../../../examples/bench/3max_2bb.toml",
    13,
    "26ce1eb0decc1cdd440009fa1b01fe584cd4781fe5b6a7addea2949e8dd5b961"
);
regression!(
    bench_6max_2bb,
    "../../../examples/bench/6max_2bb.toml",
    125,
    "0b29b4468b0d0ea1a2682485c0942e7f53034cbd30d9b3269b7f4be75ff84b1b"
);
regression!(
    bench_checkdown,
    "../../../examples/bench/6max_20bb_checkdown.toml",
    11597,
    "67e2aac06766d3e8515d27f78f5581a0df7116a6e165bcaf8f4d8090eda84535"
);

#[test]
#[ignore = "exhaustive multi-million-node tree; run with --release --ignored"]
fn bench_selector() {
    check(
        include_str!("../../../examples/bench/6max_position_selector.toml"),
        2474538,
        "91c7c88a8e36fb628a4167c4e64722d1ec6a5034b20ccdacd27868b91ae6007e",
    );
}
#[test]
#[ignore = "exhaustive multi-million-node tree; run with --release --ignored"]
fn reference_simple() {
    check(
        include_str!("../../../examples/bench/6max_100bb_nl50_partial_simple_reference.toml"),
        1962894,
        "37a9949cc832cd909d339cc51a6b24c22a582a0bfc530eda0bc71d75adac2236",
    );
}

macro_rules! expensive_consistency {
    ($name:ident, $file:literal, $count:literal, $hash:literal) => {
        #[test]
        #[ignore = "exhaustive multi-million-node tree; run with --release --ignored"]
        fn $name() {
            let raw = include_str!($file);
            let doc = spot::Document::parse(raw, Path::new("fixture.toml")).unwrap();
            let effective = doc.normalize(&P2Sections).unwrap();
            let observed = identity(raw);
            println!(
                "{}: nodes={} hash={}",
                stringify!($name),
                observed.0,
                observed.1
            );
            assert_eq!(observed.0, $count);
            assert_eq!(observed.1, $hash);
            assert_eq!(observed, identity(&effective));
        }
    };
}
expensive_consistency!(
    reference,
    "../../../examples/bench/6max_100bb_nl50_partial_reference.toml",
    37191758,
    "85cd8a03760f02de8b4c288da5df39858675f4884e7a82156a957ec0057779d3"
);
expensive_consistency!(
    reference_limp,
    "../../../examples/bench/6max_100bb_nl50_partial_reference_limp.toml",
    37193418,
    "957d4f9ef26c51a779138040983e994b0167052668a7de1fb2880f00fbd88598"
);

#[test]
fn full_surface_has_an_explicit_unrepresentable_first_actor_boundary() {
    let raw = include_str!("fixtures/preflop_multiway_v1_full_surface.toml");
    let new = lowered(raw);
    assert_eq!(
        new.game.forced_bets.as_ref().unwrap().first_to_act.index(),
        3
    );
    let attempted_override = raw.replace("[table]", "[table]\npreflop_first_to_act = 4");
    assert!(
        spot::Document::parse(&attempted_override, Path::new("fixture.toml"))
            .err()
            .unwrap()
            .to_string()
            .contains("NLH002")
    );
    assert_eq!(new.solver.seed, 19);
    assert_eq!(new.solver.sweep_batch, 3);
    assert_eq!(
        new.output.probability_encoding,
        mw_preflop::input::ProbabilityEncoding::F32
    );
}

#[test]
fn fixtures_keep_the_previously_proven_full_abstraction_config() {
    let snapshots: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/tree_abstractions.json")).unwrap();
    let crate_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (filename, expected) in snapshots.as_object().unwrap() {
        let path = if filename.starts_with("preflop_multiway_v1_") {
            crate_directory.join("tests/fixtures").join(filename)
        } else {
            crate_directory.join("../../examples/bench").join(filename)
        };
        let raw = std::fs::read_to_string(&path).unwrap();
        let expected: mw_preflop::config::AbstractionConfig =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(lowered(&raw).game.abstraction, expected, "{filename}");
        let document = spot::Document::parse(&raw, &path).unwrap();
        let effective = document.normalize(&P2Sections).unwrap();
        assert_eq!(
            spot::Document::parse(&effective, Path::new("effective.toml"))
                .unwrap()
                .normalize(&P2Sections)
                .unwrap(),
            effective
        );
    }
}
