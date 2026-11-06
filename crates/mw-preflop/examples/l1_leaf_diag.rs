//! Per-class values of one saved L1 leaf: the Postflop average, the hero's
//! real-card best response to it, and the checkdown line, per combo in BB.
//! With `--flops FILE` (one flop per line, `Ks 7h 2d`) the values are printed
//! per flop (a `flop` line before its tables) over 49 turns, each with
//! `--rivers R` rivers (default 1, range 1..=48). `--dump strategy` prints
//! instead each hero decision node's class frequencies of the saved average
//! and the share of the class reaching the node. `--dump informed` (needs
//! `--flops`) appends `ev_informed`: the class value when each combo
//! best-responds seeing only the cards dealt so far (`ev_best` sees the whole
//! runout). `--rebind-postflop OUT`
//! copies `--postflop` to OUT under this config's game fingerprint and exits
//! (no `--profile` or `--line`): for an average saved before a fingerprint
//! scheme change, once the old and new configs are shown to give equal games.
use anyhow::{Context, Result, bail, ensure};
use mw_preflop::{
    Street,
    card_abstraction::{Ehs2Abstraction, Ehs2Params},
    trunk::{
        classes::Classes,
        l0::{self, EvaluationOptions, Model, Tree},
        l1,
        tables::{HuShowdownTable, ThreeWayTable},
    },
};
use nlh::Card;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut config = None;
    let mut ehs2 = None;
    let mut profile_path = None;
    let mut postflop_path = None;
    let mut line = String::new();
    let mut boards = 4096_u32;
    let mut seed = 7_u64;
    let mut flops: Option<PathBuf> = None;
    let mut rivers = 1_usize;
    let mut flop_groups: Option<String> = None;
    let mut hand = None;
    let mut runout = None;
    let mut cards: Option<PathBuf> = None;
    let mut low_spr: Option<String> = None;
    let mut leaf_flop_groups: Option<l1::LeafFlopGroups> = None;
    let mut runout_max_slots: Option<usize> = None;
    let mut low_spr_levels: Option<Option<l1::LowSprLevels>> = None;
    let mut tables_dir = PathBuf::from(".cache/p2-trunk");
    let mut refinement = l1::Refinement::NONE;
    let mut relative: Option<(usize, usize, PathBuf)> = None;
    let mut relative_prior: Option<f64> = None;
    let mut relative_prior_fraction: Option<f64> = None;
    let mut opponent_levels: Option<[bool; 3]> = None;
    let mut strength_buckets = 128_u32;
    let mut strategy = false;
    let mut informed = false;
    let mut rebind: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().context("missing value")?;
        match arg.as_str() {
            "--config" => config = Some(PathBuf::from(value)),
            "--ehs2-cache" => ehs2 = Some(PathBuf::from(value)),
            "--profile" => profile_path = Some(PathBuf::from(value)),
            "--postflop" => postflop_path = Some(PathBuf::from(value)),
            "--line" => line = value,
            "--boards" => boards = value.parse()?,
            "--seed" => seed = value.parse()?,
            "--flops" => flops = Some(PathBuf::from(value)),
            "--rivers" => rivers = value.parse()?,
            "--l1-flop-groups" => flop_groups = Some(value),
            "--l1-low-spr" => low_spr = Some(value),
            "--l1-leaf-flop-groups" => {
                leaf_flop_groups = Some(l1::LeafFlopGroups::parse(&value)?);
            }
            "--l1-runout-max-slots" => runout_max_slots = Some(value.parse()?),
            "--l1-relative-low-spr-levels" => {
                low_spr_levels = Some(l1::parse_low_spr_levels(&value)?);
            }
            "--l1-hand" => hand = Some(l1::HandSplit::parse(&value)?),
            "--l1-runout" => runout = Some(l1::Runout::parse(&value)?),
            "--l1-cards" => {
                let path = value
                    .strip_prefix("recommended:")
                    .context("--l1-cards is recommended:STRENGTH_TABLE_128")?;
                cards = Some(PathBuf::from(path));
            }
            "--tables-dir" => tables_dir = value.into(),
            "--l1-refine" => refinement = l1::Refinement::parse(&value).context("--l1-refine")?,
            "--l1-relative" => {
                let (spec, path) = value
                    .split_once(':')
                    .context("--l1-relative is BUCKETS[xLEVELS]:STRENGTH_TABLE_128")?;
                let (n, levels) = spec.split_once('x').unwrap_or((spec, "1"));
                relative = Some((n.parse()?, levels.parse()?, PathBuf::from(path)));
            }
            "--l1-relative-prior" => relative_prior = Some(value.parse()?),
            "--l1-relative-prior-fraction" => relative_prior_fraction = Some(value.parse()?),
            "--l1-relative-opponent-levels" => {
                opponent_levels = Some(l1::parse_streets(&value)?);
            }
            "--l1-strength-buckets" => strength_buckets = value.parse()?,
            "--dump" => match value.as_str() {
                "strategy" => strategy = true,
                "informed" => informed = true,
                _ => bail!("--dump takes `strategy` or `informed`"),
            },
            "--rebind-postflop" => rebind = Some(PathBuf::from(value)),
            _ => bail!(
                "l1_leaf_diag --config TOML --ehs2-cache PATH --profile JSON --postflop BIN \
                 --line label/label/... [--boards 4096] [--seed 7] [--flops FILE] [--rivers 1] [--tables-dir PATH] [--dump strategy|informed] \
                 [--rebind-postflop OUT (copy --postflop under this config's fingerprint and exit)] \
                 [--l1-refine SPEC] [--l1-relative BUCKETS[xLEVELS]:STRENGTH_TABLE_128] \
                 [--l1-cards recommended:STRENGTH_TABLE_128 (the recommended card abstraction for every part not given; needs --l1-leaf-flop-groups own:PROFILE or none)] \
                 [--l1-relative-prior COMBOS (default 0; finite, >=0; requires --l1-relative)] \
                 [--l1-relative-prior-fraction F (adds F times the actor's reach total, default 0; requires --l1-relative)] \
                 [--l1-relative-opponent-levels none|STREET[,STREET...] (flop|turn|river; levels by showdown equity against the opponent's reach at the node; --l1-cards recommended: flop,turn,river; requires --l1-relative with LEVELS > 1; not with --dump informed or --l1-low-spr)] \
                 [--l1-strength-buckets K (strength table buckets per street, default 128)] \
                 [--l1-hand none|PART[,PART...] (made:N|draws|potential:N|flop-draws|pairs|backdoor|gutshot, or category[:N] alone; requires --l1-relative)] \
                 [--l1-runout none|KIND[,KIND...] (flush|pair|straight|over; requires --l1-relative)] \
                 [--l1-flop-groups none|recommended|FILE[:flop] (`:flop` splits only the flop; requires --l1-relative)] \
                 [--l1-low-spr MAX_SPR:BUCKETS[xLEVELS]:GROUPS (refine only low-SPR leaves; requires --l1-relative)] \
                 [--l1-leaf-flop-groups none|own[:K]:PROFILE|FILE (none: no leaf; own: every L1 leaf that a seat reaches under the class profile PROFILE gets K (default 49) flop groups from its two ranges, as trunk_solve built them; FILE: `LINE GROUPS` rows, these leaves' own flop groups; own and FILE require --l1-relative or --l1-cards and exclude --l1-low-spr)] \
                 [--l1-runout-max-slots N (the --l1-runout split only at leaves whose strategy with it has at most N slots; same requirements)] \
                 [--l1-relative-low-spr-levels MAX_SPR:[BUCKETSx]LEVELS|none (leaves whose flop SPR is at most MAX_SPR have BUCKETS percentile buckets (default those of --l1-relative) and LEVELS strength levels, a divisor of the --l1-relative LEVELS; --l1-cards recommended: 2.5:2; needs --l1-relative-opponent-levels flop,turn,river; same requirements)]"
            ),
        }
    }
    ensure!((1..=48).contains(&rivers), "--rivers must be in 1..=48");
    if let Some(path) = cards {
        let recommended = l1::CardSpec::recommended();
        relative.get_or_insert((recommended.buckets[0], recommended.levels, path));
        relative_prior.get_or_insert(recommended.prior);
        if recommended.flops.is_some() {
            flop_groups.get_or_insert_with(|| "recommended".into());
        }
        hand.get_or_insert(recommended.hand);
        runout.get_or_insert(recommended.runout);
        opponent_levels.get_or_insert(recommended.opponent_levels);
        low_spr_levels.get_or_insert(recommended.low_spr_levels);
        ensure!(
            recommended.leaf_flops.is_none() || leaf_flop_groups.is_some(),
            "--l1-cards recommended gives every L1 leaf its own flop groups from the leaf's ranges: \
             add --l1-leaf-flop-groups own:PROFILE (the profile that built the solve's groups) or none"
        );
    }
    let low_spr_levels = low_spr_levels.flatten();
    let per_leaf = leaf_flop_groups
        .as_ref()
        .is_some_and(|g| *g != l1::LeafFlopGroups::None)
        || runout_max_slots.is_some()
        || low_spr_levels.is_some();
    ensure!(
        low_spr.is_none() || relative.is_some(),
        "--l1-low-spr requires --l1-relative or --l1-cards"
    );
    ensure!(
        !per_leaf || (relative.is_some() && low_spr.is_none()),
        "--l1-leaf-flop-groups, --l1-runout-max-slots and --l1-relative-low-spr-levels require \
         --l1-relative or --l1-cards, and exclude --l1-low-spr"
    );
    ensure!(
        runout_max_slots.is_none() || runout.is_some_and(|r| r != l1::Runout::default()),
        "--l1-runout-max-slots requires --l1-runout KINDS"
    );
    ensure!(
        relative.is_some()
            || (relative_prior.is_none()
                && relative_prior_fraction.is_none()
                && opponent_levels.is_none()
                && flop_groups.is_none()
                && hand.is_none()
                && runout.is_none()),
        "--l1-relative-prior, --l1-relative-prior-fraction, --l1-relative-opponent-levels, --l1-flop-groups, --l1-hand and --l1-runout require --l1-relative or --l1-cards"
    );
    let opponent_levels = opponent_levels.unwrap_or_default();
    ensure!(
        opponent_levels == [false; 3]
            || relative.as_ref().is_some_and(|(_, levels, _)| *levels > 1),
        "--l1-relative-opponent-levels requires more than one strength level"
    );
    ensure!(
        low_spr_levels.is_none() || opponent_levels == [true; 3],
        "--l1-relative-low-spr-levels needs --l1-relative-opponent-levels flop,turn,river \
         (--l1-cards recommended has both; --l1-relative-low-spr-levels none turns the rule off)"
    );
    let hand = hand.unwrap_or_default();
    let runout = runout.unwrap_or_default();
    let relative_prior = relative_prior.unwrap_or(0.0);
    ensure!(
        relative_prior.is_finite() && relative_prior >= 0.0,
        "--l1-relative-prior must be finite and nonnegative"
    );
    let relative_prior_fraction = relative_prior_fraction.unwrap_or(0.0);
    ensure!(
        relative_prior_fraction.is_finite() && relative_prior_fraction >= 0.0,
        "--l1-relative-prior-fraction must be finite and nonnegative"
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
    let refined = l1::Refined {
        base: &abstraction,
        refinement,
    };
    let strength = relative.as_ref().map(|(_, _, path)| {
        Ehs2Abstraction::load_or_build(
            Ehs2Params {
                flop_buckets: strength_buckets,
                turn_buckets: strength_buckets,
                river_buckets: strength_buckets,
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
    let relative_source = relative
        .as_ref()
        .zip(strength.as_ref())
        .map(|((n, levels, _), s)| l1::Relative {
            strength: s,
            counts: [*n; 3],
            levels: *levels,
            prior: relative_prior,
            prior_fraction: relative_prior_fraction,
            reach: l1::RelativeReach::Current,
            refinement,
            flops: flop_groups.as_ref(),
            hand,
            runout,
            opponent_levels,
        });
    let low_spr_spec = low_spr.as_deref().map(l1::LowSprSpec::parse).transpose()?;
    let low_spr_source = relative_source
        .as_ref()
        .zip(low_spr_spec.as_ref())
        .map(|(base, spec)| l1::LowSprCards::new(*base, spec))
        .transpose()?;
    let leaf_groups = match (&leaf_flop_groups, strength.as_ref()) {
        (Some(spec), Some(strength)) => spec.resolve(&tree, &model, strength)?,
        _ => None,
    };
    let leaf_source = relative_source
        .as_ref()
        .filter(|_| leaf_groups.is_some() || runout_max_slots.is_some() || low_spr_levels.is_some())
        .map(|base| {
            let spec = l1::LeafCardSpec {
                flops: leaf_groups
                    .iter()
                    .flatten()
                    .map(|(z, groups)| (*z, groups.as_ref()))
                    .collect(),
                runout: runout_max_slots.map(|max| (runout, max)),
                low_spr_levels,
            };
            let base = l1::Relative {
                runout: if runout_max_slots.is_some() {
                    l1::Runout::default()
                } else {
                    runout
                },
                ..*base
            };
            l1::LeafCards::new(&tree, base, &spec)
        })
        .transpose()?;
    let buckets: &dyn l1::BucketSource = match (&leaf_source, &low_spr_source, &relative_source) {
        (Some(r), _, _) => r,
        (_, Some(r), _) => r,
        (_, _, Some(r)) => r,
        _ => &refined,
    };
    let postflop_path = postflop_path.context("--postflop")?;
    if let Some(out) = rebind {
        let tmp = out.with_extension("rebind-tmp");
        let result = l1::Strategies::new(&tree, buckets).rebind_average(
            &tree,
            &mut std::fs::File::open(&postflop_path)?,
            &mut std::fs::File::create(&tmp)?,
        );
        let old = match result {
            Ok(old) => old,
            Err(error) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(error);
            }
        };
        std::fs::rename(&tmp, &out)?;
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        println!(
            "rebound {} -> {}: fingerprint {} -> {}",
            postflop_path.display(),
            out.display(),
            hex(&old),
            hex(&tree.game_fingerprint)
        );
        return Ok(());
    }
    let document: l0::ClassProfileDocument =
        serde_json::from_slice(&std::fs::read(profile_path.context("--profile")?)?)?;
    let profile = l0::Profile::from_json(&tree, &document)?;
    drop(document);
    let z = l1::leaf_of_line(&tree, &line)?;
    // Only this leaf's accumulators: a fixed-lines average stores no others.
    let mut strategies = l1::Strategies::new_for(&tree, buckets, Some(&[z]));
    strategies.read_average_leaf(&tree, z, &mut std::fs::File::open(&postflop_path)?)?;
    let terminal = tree.nodes[z].terminal.as_ref().unwrap();
    let sets = match &flops {
        None => {
            let options = l1::Options {
                l1_eval_boards: boards,
                l1_eval_seed: seed,
                ..Default::default()
            };
            vec![(None, l1::evaluation_boards(buckets, &options))]
        }
        Some(path) => std::fs::read_to_string(path)?
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| {
                let flop: Vec<Card> = l
                    .split_whitespace()
                    .map(|c| c.parse().map_err(|e| anyhow::anyhow!("card {c}: {e:?}")))
                    .collect::<Result<_>>()?;
                ensure!(flop.len() == 3, "flop {l} is not three cards");
                let live: Vec<Card> = (0..52)
                    .map(Card::from_index)
                    .filter(|c| !flop.contains(c))
                    .collect();
                ensure!(live.len() == 49, "flop {l} repeats a card");
                let runouts = (0..49)
                    .flat_map(|t| {
                        let mut rest = live.clone();
                        let turn = rest.remove(t);
                        let flop = &flop;
                        (0..rivers).map(move |k| {
                            let river = rest[(t * 7 + 3 + k * 48 / rivers) % 48];
                            l1::Board::new([flop[0], flop[1], flop[2], turn, river], buckets)
                        })
                    })
                    .collect();
                Ok((Some(l.to_string()), runouts))
            })
            .collect::<Result<Vec<_>>>()?,
    };
    println!("leaf {z}: active {:?}", terminal.active);
    let catalog = Classes::get();
    for (flop, boards) in &sets {
        if let Some(flop) = flop {
            println!("flop {flop}");
        }
        for &hero in &terminal.active {
            if strategy {
                let dump =
                    l1::leaf_strategy(&tree, &profile, &model, &strategies, boards, z, hero)?;
                println!("hero {hero}");
                for node in &dump.nodes {
                    println!(
                        "node {} path {} actions {}",
                        node.node,
                        node.path.join("/"),
                        node.labels.join("|")
                    );
                    println!("class,reach,{}", node.labels.join(","));
                    for c in 0..169 {
                        if node.weight[c] <= 0.0 || dump.root[c] <= 0.0 {
                            continue;
                        }
                        let row: Vec<String> = node.frequencies[c]
                            .iter()
                            .map(|f| format!("{f:.4}"))
                            .collect();
                        println!(
                            "{},{:.4},{}",
                            catalog.name(c),
                            node.weight[c] / dump.root[c],
                            row.join(",")
                        );
                    }
                }
                continue;
            }
            let d = l1::leaf_diagnostic(&tree, &profile, &model, &strategies, boards, z, hero)?;
            let informed = if informed {
                Some(l1::leaf_informed_best_response(
                    &tree,
                    &profile,
                    &model,
                    &strategies,
                    boards,
                    z,
                    hero,
                )?)
            } else {
                None
            };
            println!("hero {hero}");
            println!(
                "class,ev_l1,ev_best,ev_checkdown,l1_minus_checkdown,best_minus_l1,matchup_mass{}",
                if informed.is_some() {
                    ",ev_informed"
                } else {
                    ""
                }
            );
            for c in 0..169 {
                if d.mass[c] <= 0.0 {
                    continue;
                }
                let (s, b, x) = (
                    d.profile[c] / d.mass[c],
                    d.best[c] / d.mass[c],
                    d.checkdown[c] / d.mass[c],
                );
                let extra = informed
                    .as_ref()
                    .map_or(String::new(), |v| format!(",{:.4}", v[c] / d.mass[c]));
                println!(
                    "{},{s:.4},{b:.4},{x:.4},{:.4},{:.4},{:.12}{extra}",
                    catalog.name(c),
                    s - x,
                    b - s,
                    catalog.n(c) as f64 * d.matchup_mass[c]
                );
            }
        }
    }
    Ok(())
}
