//! Experimental saved-profile action EVs at a preflop decision, with fold = 0.
//! Reuses the trunk terminal evaluator/backward pass and L1 leaf diagnostic.
//! L1 board estimates are normalized by their sampled compatible-combo mass,
//! then scaled by exact trunk opponent mass, matching l1_leaf_diag/call_ev.py.
use anyhow::{Context, Result, bail, ensure};
use mw_preflop::{
    Street,
    card_abstraction::{Ehs2Abstraction, Ehs2Params},
    trunk::{
        classes::Classes,
        l0::{self, EvaluationOptions, Model, Profile, Reach, Tree},
        l1,
        tables::{HuShowdownTable, ThreeWayTable},
    },
};
use std::path::PathBuf;

const USAGE: &str = "node_ev --config TOML --ehs2-cache PATH --profile JSON --postflop BIN \
    --line label/label/... [--boards 2048] [--seed 0] [--tables-dir PATH] \
    [--l1-relative BUCKETS[xLEVELS]:STRENGTH_TABLE_128] \
    [--l1-relative-prior COMBOS] [--l1-flop-groups none|recommended|FILE[:flop]] \
    [--l1-hand none|PART[,PART...] (made:N|draws|potential:N|flop-draws|pairs|backdoor|gutshot, or category[:N] alone)] [--l1-low-spr MAX_SPR:BUCKETS[xLEVELS]:GROUPS] \
    [--l1-refine texture:0|1|2,recall:N]";
const RECALL_USAGE: &str = "--l1-refine texture:0|1|2,recall:N (relative mode keeps the original flop strength); --l1-hand draws,flop-draws keeps a binary flop draw class";

fn main() -> Result<()> {
    let mut config = None;
    let mut ehs2 = None;
    let mut profile_path = None;
    let mut postflop_path = None;
    let mut line = String::new();
    let mut boards = 2048_u32;
    let mut seed = 0_u64;
    let mut flop_groups: Option<String> = None;
    let mut low_spr: Option<String> = None;
    let mut hand = None;
    let mut tables_dir = PathBuf::from(".cache/p2-trunk");
    let mut relative: Option<(usize, usize, PathBuf)> = None;
    let mut relative_prior: Option<f64> = None;
    let mut refinement = l1::Refinement::NONE;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args
            .next()
            .with_context(|| format!("missing value for {arg}; {USAGE}"))?;
        match arg.as_str() {
            "--config" => config = Some(PathBuf::from(value)),
            "--ehs2-cache" => ehs2 = Some(PathBuf::from(value)),
            "--profile" => profile_path = Some(PathBuf::from(value)),
            "--postflop" => postflop_path = Some(PathBuf::from(value)),
            "--line" => line = value,
            "--boards" => boards = value.parse()?,
            "--seed" => seed = value.parse()?,
            "--l1-flop-groups" => flop_groups = Some(value),
            "--l1-low-spr" => low_spr = Some(value),
            "--l1-hand" => hand = Some(l1::HandSplit::parse(&value)?),
            "--l1-refine" => {
                refinement = l1::Refinement::parse(&value).context(RECALL_USAGE)?;
            }
            "--tables-dir" => tables_dir = value.into(),
            "--l1-relative" => {
                let (spec, path) = value
                    .split_once(':')
                    .context("--l1-relative is BUCKETS[xLEVELS]:STRENGTH_TABLE_128")?;
                let (n, levels) = spec.split_once('x').unwrap_or((spec, "1"));
                relative = Some((n.parse()?, levels.parse()?, PathBuf::from(path)));
            }
            "--l1-relative-prior" => relative_prior = Some(value.parse()?),
            _ => bail!("{USAGE}"),
        }
    }
    ensure!(
        low_spr.is_none() || relative.is_some(),
        "--l1-low-spr requires --l1-relative"
    );
    ensure!(
        boards >= 2 && boards.is_multiple_of(2),
        "--boards must be even and >= 2"
    );
    ensure!(
        relative.is_some() || (relative_prior.is_none() && flop_groups.is_none() && hand.is_none()),
        "--l1-relative-prior, --l1-flop-groups and --l1-hand require --l1-relative"
    );
    let relative_prior = relative_prior.unwrap_or(0.0);
    ensure!(
        relative_prior.is_finite() && relative_prior >= 0.0,
        "--l1-relative-prior must be finite and nonnegative"
    );
    let config = config.context("--config")?;
    let game = l0::game_from_config(&std::fs::read_to_string(&config)?, &config)?;
    let tree = Tree::build_with(&game, l0::FlopLeaves::L1)?;
    let t2 = HuShowdownTable::load_or_build(&tables_dir)?;
    let t3 = ThreeWayTable::load_or_build(&tables_dir, 4096, 0)?;
    let tables = (&t2, &t3);
    let model = Model::new(&game, &tables, EvaluationOptions::default())?;
    let counts = &model.game().config().abstraction;
    let abstraction = Ehs2Abstraction::load_or_build(
        Ehs2Params {
            flop_buckets: u32::from(counts.flop_buckets),
            turn_buckets: u32::from(counts.turn_buckets),
            river_buckets: u32::from(counts.river_buckets),
        },
        &[Street::Flop, Street::Turn, Street::River],
        Some(ehs2.as_deref().context("--ehs2-cache")?),
    );
    let document: l0::ClassProfileDocument =
        serde_json::from_slice(&std::fs::read(profile_path.context("--profile")?)?)?;
    let profile = Profile::from_json(&tree, &document)?;
    drop(document);
    let refined = l1::Refined {
        base: &abstraction,
        refinement,
    };
    let strength = relative.as_ref().map(|(_, _, path)| {
        Ehs2Abstraction::load_or_build(
            Ehs2Params {
                flop_buckets: 128,
                turn_buckets: 128,
                river_buckets: 128,
            },
            &[Street::Flop, Street::Turn, Street::River],
            Some(path),
        )
    });
    let flop_groups = flop_groups
        .as_deref()
        .map(l1::FlopClusters::from_spec)
        .transpose()?
        .flatten();
    let relative_source =
        relative
            .as_ref()
            .zip(strength.as_ref())
            .map(|((n, levels, _), strength)| l1::Relative {
                strength,
                counts: [*n; 3],
                levels: *levels,
                prior: relative_prior,
                prior_fraction: 0.0,
                reach: l1::RelativeReach::Current,
                refinement,
                flops: flop_groups.as_ref(),
                hand: hand.unwrap_or_default(),
                runout: l1::Runout::default(),
                opponent_levels: [false; 3],
            });
    let low_spr_spec = low_spr.as_deref().map(l1::LowSprSpec::parse).transpose()?;
    let low_spr_source = relative_source
        .as_ref()
        .zip(low_spr_spec.as_ref())
        .map(|(base, spec)| l1::LowSprCards::new(*base, spec))
        .transpose()?;
    let buckets: &dyn l1::BucketSource = match (&low_spr_source, &relative_source) {
        (Some(source), _) => source,
        (_, Some(source)) => source,
        _ => &refined,
    };
    let mut strategies = l1::Strategies::new(&tree, buckets);
    strategies.read_average(
        &tree,
        &mut std::fs::File::open(postflop_path.context("--postflop")?)?,
    )?;
    let mut root = 0;
    for label in line.split('/').filter(|label| !label.is_empty()) {
        let node = &tree.nodes[root];
        let action = node
            .labels
            .iter()
            .position(|l| l == label)
            .with_context(|| format!("no action {label} at node {root}: {:?}", node.labels))?;
        root = node.children[action];
    }
    let node = &tree.nodes[root];
    let hero = node
        .actor
        .context("line must end at a trunk decision node")?;
    let fold = node
        .labels
        .iter()
        .position(|l| l == "fold")
        .context("decision has no fold action")?;
    let reach = l0::reaches(&tree, &profile, &model);
    let boards = l1::evaluation_boards(
        buckets,
        &l1::Options {
            l1_eval_boards: boards,
            l1_eval_seed: seed,
            ..Default::default()
        },
    );
    let mut descendants = vec![false; tree.nodes.len()];
    descendants[root] = true;
    let mut leaf_values = Vec::new();
    for (z, node) in tree.nodes.iter().enumerate().skip(root + 1) {
        descendants[z] = node.parent.is_some_and(|(parent, _)| descendants[parent]);
        if !descendants[z]
            || !node
                .terminal
                .as_ref()
                .is_some_and(|t| t.l1.is_some() && t.active.contains(&hero))
        {
            continue;
        }
        let diagnostic =
            l1::leaf_diagnostic(&tree, &profile, &model, &strategies, &boards, z, hero)?;
        let values = std::array::from_fn(|c| {
            if diagnostic.mass[c] > 0.0 {
                diagnostic.profile[c] / diagnostic.mass[c]
                    * opponent_mass(&reach[z], tree.seats, hero, c)
            } else {
                0.0
            }
        });
        leaf_values.push((z, values));
    }
    let values = l0::profile_values(&tree, &profile, &model, &reach, hero, &leaf_values)?;
    println!("line {line}");
    println!(
        "node {root}, actor {hero}, boards {}, seed {seed}",
        boards.len()
    );
    println!(
        "EV in bb, action minus fold; L1 compatible-board mass normalized as l1_leaf_diag/call_ev.py"
    );
    println!("range shares use combo count * starting class weight * own trunk reach");
    println!(
        "model: hero-conditioned classes; ignores opponent/opponent overlap and folded cards; T3/K4 sampled as solver"
    );
    let catalog = Classes::get();
    let own_mass: [f64; 169] = std::array::from_fn(|c| {
        catalog.n(c) as f64 * model.weights()[hero][c] * reach[root].pi(hero, c)
    });
    let total: f64 = own_mass.iter().sum();
    ensure!(total > 0.0, "actor's range has zero reach");
    print!("class,range_share");
    for label in &node.labels {
        print!(",strategy:{label},ev_bb:{label}");
    }
    println!();
    for (c, &mass) in own_mass.iter().enumerate() {
        if mass <= 0.0 {
            continue;
        }
        let opponent = opponent_mass(&reach[root], tree.seats, hero, c);
        ensure!(opponent > 0.0, "no opponent reach for {}", catalog.name(c));
        print!("{},{:.8}", catalog.name(c), mass / total);
        let baseline = values[node.children[fold]][c];
        for (a, &child) in node.children.iter().enumerate() {
            print!(
                ",{:.8},{:.6}",
                profile.row(&tree, root, c)[a],
                (values[child][c] - baseline) / opponent
            );
        }
        println!();
    }
    for (a, &child) in node.children.iter().enumerate() {
        if node.labels[a].starts_with("raise-to:") {
            print_replies(
                &tree,
                &profile,
                &model,
                &reach,
                child,
                hero,
                &node.labels[a],
            );
        }
    }
    Ok(())
}

fn opponent_mass(reach: &Reach, seats: usize, hero: usize, class: usize) -> f64 {
    (0..seats)
        .filter(|&seat| seat != hero)
        .map(|seat| reach.mass(seat, class))
        .product()
}

fn joint_mass(tree: &Tree, model: &Model<'_>, reach: &Reach, hero: usize) -> f64 {
    (0..169)
        .map(|c| {
            Classes::get().n(c) as f64
                * model.weights()[hero][c]
                * reach.pi(hero, c)
                * opponent_mass(reach, tree.seats, hero, c)
        })
        .sum()
}

fn print_replies(
    tree: &Tree,
    profile: &Profile,
    model: &Model<'_>,
    reach: &[Reach],
    root: usize,
    hero: usize,
    label: &str,
) {
    println!("raise {label}");
    let mut z = root;
    while let Some(actor) = tree.nodes[z].actor {
        let node = &tree.nodes[z];
        let weights: [f64; 169] = std::array::from_fn(|c| {
            Classes::get().n(c) as f64 * model.weights()[actor][c] * reach[z].pi(actor, c)
        });
        let mass: f64 = weights.iter().sum();
        print!("reply node {z},actor {actor}");
        for (a, action) in node.labels.iter().enumerate() {
            let probability = if mass > 0.0 {
                weights
                    .iter()
                    .enumerate()
                    .map(|(c, w)| w * profile.row(tree, z, c)[a])
                    .sum::<f64>()
                    / mass
            } else {
                f64::NAN
            };
            print!(",{action}:{probability:.8}");
        }
        println!();
        let Some(fold) = node.labels.iter().position(|l| l == "fold") else {
            break;
        };
        z = node.children[fold];
    }
    let mass = joint_mass(tree, model, &reach[root], hero);
    if mass <= 0.0 {
        println!("fold_to_probability unreachable (raise has zero profile reach)");
    } else {
        let folded = tree.nodes[z]
            .terminal
            .as_ref()
            .is_some_and(|t| t.active == [hero]);
        let probability = if folded {
            joint_mass(tree, model, &reach[z], hero) / mass
        } else {
            0.0
        };
        println!("fold_to_probability {probability:.8}");
    }
}
