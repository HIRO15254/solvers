//! Legacy game snapshots were captured with cli::multiway_v1::parse_and_lower_at
//! at 0befef85417fd1e015d662a2de00bfb145174b04 (M5). Source hashes prevent stale
//! snapshots from silently testing different examples. No CLI dependency is needed.
use mw_preflop::betting::BettingMenu;
use mw_preflop::config::{BettingConfig, MultiwayConfig, RuleStreet};
use mw_preflop::input::{P2Sections, Settings, lower};
use mw_preflop::{BettingState, Street};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

fn expected(path: &str) -> (MultiwayConfig, toml::Table) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let raw = std::fs::read_to_string(root.join(path)).unwrap();
    let snapshots: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/legacy_example_games.json")).unwrap();
    let snapshot = &snapshots[path];
    assert_eq!(
        snapshot["source_blake3"].as_str().unwrap(),
        blake3::hash(raw.as_bytes()).to_hex().as_str(),
        "update the M5 oracle snapshot deliberately when changing {path}"
    );
    let game: MultiwayConfig = serde_json::from_value(snapshot["game"].clone()).unwrap();
    // Empty new fields must not change the old-family JSON game identity.
    assert_eq!(serde_json::to_value(&game).unwrap(), snapshot["game"]);
    (game, toml::from_str(&raw).unwrap())
}

/// Materialize old menus explicitly; common input has no implicit standard tree.
/// Rules follow (priority, source_order), and old Postflop expands to three streets.
fn v1(game: &MultiwayConfig, old: &toml::Table) -> String {
    use nlh::script::{ActionKind, Effect};
    let mut script = String::new();
    for street in Street::ALL {
        let menu = game.betting.for_street(street);
        let keyword = match street {
            Street::Preflop => "preflop",
            Street::Flop => "flop",
            Street::Turn => "turn",
            Street::River => "river",
        };
        let opening = if street == Street::Preflop {
            "raise"
        } else {
            "bet"
        };
        for (condition, action, sizes) in [
            ("unopened", opening, menu.bet_sizes.as_slice()),
            ("!unopened", "raise", menu.raise_sizes.as_slice()),
        ] {
            let sizes = sizes
                .iter()
                .map(|s| s.render(nlh::SizeUnit::Bb))
                .collect::<Vec<_>>()
                .join(", ");
            script.push_str(&format!(
                "{keyword} when {condition} {{ replace {action} [{sizes}] }}\n"
            ));
        }
    }
    if !game.betting.allow_limp {
        script.push_str("preflop when unopened { remove call }\n");
    }
    let mut rules = game.betting.rules.iter().collect::<Vec<_>>();
    rules.sort_by_key(|r| (r.priority, r.source_order));
    for rule in rules {
        let streets = match rule.street {
            RuleStreet::Preflop => "preflop",
            RuleStreet::Flop => "flop",
            RuleStreet::Turn => "turn",
            RuleStreet::River => "river",
            RuleStreet::Postflop => "flop, turn, river",
        };
        let effect = match rule.effect {
            Effect::Add => "add",
            Effect::Remove => "remove",
            Effect::Replace => "replace",
            Effect::Force => "force",
            Effect::Checkdown => "checkdown",
        };
        let action = match rule.action {
            None => "",
            Some(ActionKind::Fold) => "fold",
            Some(ActionKind::Check) => "check",
            Some(ActionKind::Call) => "call",
            Some(ActionKind::Bet) => "bet",
            Some(ActionKind::Raise) => "raise",
        };
        let sizes = if rule.sizes.is_empty() {
            String::new()
        } else {
            format!(
                " [{}]",
                rule.sizes
                    .iter()
                    .map(|s| s.render(nlh::SizeUnit::Bb))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let when = if rule.condition == "always" {
            String::new()
        } else {
            format!(" when {}", rule.condition)
        };
        script.push_str(&format!("{streets}{when} {{ {effect} {action}{sizes} }}\n"));
    }
    // include_allin acts before script effects; replace rules above must preserve
    // the implicit old all-in in their menus too, unless later rules remove it.
    let mut lines = script.lines().map(str::to_owned).collect::<Vec<_>>();
    for (i, street) in Street::ALL.into_iter().enumerate() {
        if game.betting.for_street(street).include_allin {
            for line in &mut lines[i * 2..i * 2 + 2] {
                *line = line.replace("]", ", a]");
            }
        }
    }
    let script = lines.join("\n");
    let mut table = toml::Table::new();
    table.insert("players".into(), (game.seats.len() as i64).into());
    let mut stacks = toml::Table::new();
    let mut ranges = toml::Table::new();
    for (i, seat) in game.seats.iter().enumerate() {
        let position = nlh::position_name(
            nlh::SeatId::new_unchecked(i as u8),
            game.button,
            game.seats.len(),
        );
        stacks.insert(position.into(), seat.stack_bb.into());
        ranges.insert(
            position.into(),
            if seat.range.is_empty() {
                "random".into()
            } else {
                seat.range.clone().into()
            },
        );
    }
    table.insert("stacks_bb".into(), stacks.into());
    let caps = &game.betting;
    let mut tree = toml::Table::new();
    tree.insert("script".into(), script.into());
    tree.insert("include_allin".into(), true.into());
    tree.insert(
        "max_aggressive_actions".into(),
        toml::Value::try_from(
            [
                ("preflop", caps.preflop.max_aggressive_actions),
                ("flop", caps.flop.max_aggressive_actions),
                ("turn", caps.turn.max_aggressive_actions),
                ("river", caps.river.max_aggressive_actions),
            ]
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>(),
        )
        .unwrap(),
    );
    if let Some(ratio) = caps.preflop.reraise_jam_above_actor_starting_stack {
        tree.insert(
            "preflop_reraise_jam_above_stack".into(),
            toml::Value::try_from(ratio).unwrap(),
        );
    }
    let old_game = old["game"].as_table().unwrap();
    let mut solver = old
        .get("solver")
        .and_then(toml::Value::as_table)
        .cloned()
        .unwrap_or_default();
    if let Some(abstraction) = old_game.get("abstraction") {
        solver.insert("abstraction".into(), abstraction.clone());
    }
    let mut run = toml::Table::new();
    if let Some(old_run) = old.get("run").and_then(toml::Value::as_table) {
        let mut stop = old_run
            .get("stop")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default();
        if let Some(sweeps) = old_run.get("max_sweeps") {
            stop.insert("max_sweeps".into(), sweeps.clone());
        }
        solver.insert("stop".into(), stop.into());
        if let Some(resources) = old_run.get("resources").and_then(toml::Value::as_table) {
            run.extend(resources.clone());
        }
        if let Some(time) = old_run.get("max_time") {
            run.insert("max_time".into(), time.clone());
        }
        if let Some(interval) = old_run.get("checkpoint").and_then(|c| c.get("interval")) {
            run.insert("checkpoint_interval".into(), interval.clone());
        }
    }
    let mut root = toml::Table::new();
    root.insert("schema".into(), "solvers.nlh/v1".into());
    root.insert("table".into(), table.into());
    root.insert("ranges".into(), ranges.into());
    root.insert("tree".into(), tree.into());
    root.insert("solver".into(), solver.into());
    root.insert("run".into(), run.into());
    if let Some(output) = old.get("output") {
        root.insert("output".into(), output.clone());
    }
    if let Some(economics) = old.get("economics") {
        root.insert("economics".into(), economics.clone());
    }
    toml::to_string(&root).unwrap()
}

/// Bisimulation of the entire reachable public tree. Exact state bytes cache
/// previously proven identical suffixes (no board/private-card or depth sampling).
/// The count still includes every history node represented by a reused suffix.
struct Proof {
    suffixes: Vec<Option<(Vec<u8>, u64)>>,
    visits: u64,
    path: String,
}

fn walk(
    a: BettingState,
    b: BettingState,
    old: &BettingConfig,
    new: &BettingConfig,
    proven: &mut Proof,
) -> u64 {
    assert_eq!(a, b, "root or transition differs");
    proven.visits += 1;
    if proven.visits.is_multiple_of(1_000_000) {
        println!("{}: {} visited states", proven.path, proven.visits);
    }
    // Terminal states are compared too, but have no suffix worth caching.
    // Keeping them out of the cache leaves its budget for decision states.
    if a.phase.is_terminal() {
        assert!(a.legal_actions(old).unwrap().is_empty());
        assert!(b.legal_actions(new).unwrap().is_empty());
        return 1;
    }
    let key = postcard::to_stdvec(&a).unwrap();
    let mut hash = DefaultHasher::new();
    key.hash(&mut hash);
    let slot = hash.finish() as usize & (proven.suffixes.len() - 1);
    if let Some((cached_key, count)) = &proven.suffixes[slot]
        && cached_key == &key
    {
        return *count;
    }
    let old_actions = a.legal_actions(old).unwrap();
    let new_actions = b.legal_actions(new).unwrap();
    assert_eq!(old_actions, new_actions, "menu differs at {a:?}");
    let mut count = 1;
    for action in &old_actions {
        let mut next_a = a.clone();
        let mut next_b = b.clone();
        next_a
            .apply_from_actions(action.clone(), &old_actions, old)
            .unwrap();
        next_b
            .apply_from_actions(action.clone(), &new_actions, new)
            .unwrap();
        count += walk(next_a, next_b, old, new, proven);
    }
    // Bound memory while continuing to cache later branches. Hash collisions
    // only evict work: a cache hit always compares the entire serialized state.
    proven.suffixes[slot] = Some((key, count));
    count
}

fn check(path: &str) {
    let (old, source) = expected(path);
    let raw = v1(&old, &source);
    let doc = spot::Document::parse(&raw, Path::new("v1.toml")).unwrap();
    let effective = doc.normalize(&P2Sections).unwrap();
    assert_eq!(
        spot::Document::parse(&effective, Path::new("v1.toml"))
            .unwrap()
            .normalize(&P2Sections)
            .unwrap(),
        effective
    );
    let settings = Settings::parse(&doc.spot, &doc.solver, &doc.output).unwrap();
    let new = lower(&doc.spot, &settings)
        .unwrap()
        .game
        .validated()
        .unwrap();
    let old = old.validated().unwrap();
    let mut proven = Proof {
        suffixes: vec![None; 1 << 19],
        visits: 0,
        path: path.into(),
    };
    let count = walk(
        BettingState::from_config(&old).unwrap(),
        BettingState::from_config(&new).unwrap(),
        &old.betting,
        &new.betting,
        &mut proven,
    );
    println!(
        "{path}: {count} history nodes, {} cached distinct suffixes",
        proven
            .suffixes
            .iter()
            .filter(|entry| entry.is_some())
            .count()
    );
}

macro_rules! identity {
    ($name:ident, $path:literal) => {
        #[test]
        fn $name() {
            check($path);
        }
    };
}
macro_rules! expensive_identity {
    ($name:ident, $path:literal) => {
        #[test]
        #[ignore = "exhaustive multi-million-node tree; run with --release --ignored"]
        fn $name() {
            check($path);
        }
    };
}
identity!(smoke_3max, "examples/preflop_multiway_v1_3max_smoke.toml");
identity!(default_6max, "examples/preflop_multiway_v1_default.toml");
identity!(
    production_smoke,
    "examples/preflop_multiway_v1_production_smoke.toml"
);
identity!(bench_3max_2bb, "examples/bench_multiway/3max_2bb.toml");
identity!(bench_6max_2bb, "examples/bench_multiway/6max_2bb.toml");
identity!(
    bench_checkdown,
    "examples/bench_multiway/6max_20bb_checkdown.toml"
);
expensive_identity!(
    bench_selector,
    "examples/bench_multiway/6max_position_selector.toml"
);
expensive_identity!(
    reference,
    "examples/bench_multiway/6max_100bb_nl50_partial_reference.toml"
);
expensive_identity!(
    reference_limp,
    "examples/bench_multiway/6max_100bb_nl50_partial_reference_limp.toml"
);
expensive_identity!(
    reference_simple,
    "examples/bench_multiway/6max_100bb_nl50_partial_simple_reference.toml"
);

#[test]
fn full_surface_has_an_explicit_unrepresentable_first_actor_boundary() {
    let (old, source) = expected("examples/preflop_multiway_v1_full_surface.toml");
    let raw = v1(&old, &source);
    let doc = spot::Document::parse(&raw, Path::new("v1.toml")).unwrap();
    let settings = Settings::parse(&doc.spot, &doc.solver, &doc.output).unwrap();
    let new = lower(&doc.spot, &settings).unwrap();
    assert_eq!(old.forced_bets.as_ref().unwrap().first_to_act.index(), 4);
    assert_eq!(
        new.game.forced_bets.as_ref().unwrap().first_to_act.index(),
        3
    );
    let attempted_override = raw.replace("[table]", "[table]\npreflop_first_to_act = 4");
    assert!(
        spot::Document::parse(&attempted_override, Path::new("v1.toml"))
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
