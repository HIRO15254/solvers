//! Experimental deterministic DCFR solver for the public L0 preflop trunk.
use anyhow::{Context, Result, bail, ensure};
use mw_preflop::trunk::{
    l0::{self, EvaluationOptions, Model, SolveOptions, SolveTimings, Tree},
    l1,
    tables::{HuShowdownTable, ThreeWayTable},
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

struct HashingReader<'a, R> {
    input: &'a mut R,
    hash: &'a mut blake3::Hasher,
}

impl<R: std::io::Read> std::io::Read for HashingReader<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let n = self.input.read(bytes)?;
        self.hash.update(&bytes[..n]);
        Ok(n)
    }
}

fn sampling(value: &str) -> Result<l1::Sampling> {
    match value {
        "random" => Ok(l1::Sampling::Random),
        "stratified" => Ok(l1::Sampling::Stratified),
        _ => bail!("board sampling must be random or stratified"),
    }
}

/// Fixed-profile mode inputs: the class profile, the leaf lines, the training
/// flops and the iterations after which the Postflop average is also written.
struct Fixed<'a> {
    profile: &'a Path,
    lines: Option<&'a str>,
    flops: Option<&'a Path>,
    snapshots: &'a [u64],
}

#[derive(Default)]
struct RunState {
    file: Option<PathBuf>,
    every: Option<f64>,
    snapshots: Option<PathBuf>,
}

fn write_state_atomic(path: &Path, p: &l1::Progress<'_>) -> Result<()> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut file = std::fs::File::create(&tmp)?;
    p.write_state(&mut file)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)?;
    #[cfg(unix)]
    std::fs::File::open(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?
    .sync_all()?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct OverrideDocument {
    format: String,
    version: u32,
    entries: Vec<OverrideEntry>,
}

#[derive(serde::Deserialize)]
struct OverrideEntry {
    line: String,
    seat: usize,
    values: std::collections::BTreeMap<String, f64>,
}

fn main() -> Result<()> {
    let mut run_state = RunState::default();
    let mut leaf_model = String::from("l0");
    let mut ehs2_cache = None;
    let mut l1_options = l1::Options::default();
    let mut stop_file: Option<PathBuf> = None;
    let mut config = None;
    let mut tables_dir = PathBuf::from(".cache/p2-trunk");
    let mut t3_samples = 4096;
    let mut t3_seed = 0;
    let mut model_options = EvaluationOptions::default();
    let mut options = SolveOptions::default();
    let mut beta = None;
    let mut threads = None;
    let mut print_every = 0;
    let mut output = None;
    let mut output_profile = None;
    let mut output_postflop = None;
    let mut evaluate_profile = None;
    let mut evaluate_postflop = None;
    let mut evaluate_override = None;
    let mut train_override = None;
    let mut refinement = l1::Refinement::NONE;
    let mut relative: Option<(usize, usize, PathBuf)> = None;
    let mut relative_prior: Option<f64> = None;
    let mut relative_prior_fraction: Option<f64> = None;
    let mut opponent_levels: Option<[bool; 3]> = None;
    let mut strength_buckets = 128_u32;
    let mut relative_reach = None;
    let mut flop_groups: Option<String> = None;
    let mut flop_subset_path: Option<PathBuf> = None;
    let mut hand = None;
    let mut runout = None;
    let mut cards: Option<PathBuf> = None;
    let mut low_spr: Option<String> = None;
    let mut leaf_flop_groups: Option<l1::LeafFlopGroups> = None;
    let mut runout_max_slots: Option<usize> = None;
    let mut low_spr_levels: Option<Option<l1::LowSprLevels>> = None;
    let mut fixed_profile: Option<PathBuf> = None;
    let mut fixed_lines: Option<String> = None;
    let mut fixed_flops_path: Option<PathBuf> = None;
    let mut fixed_snapshots: Vec<u64> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!(
                "trunk_solve --config TOML [--leaf-model l0|l1] [--ehs2-cache PATH (required for l1)] [--l1-boards 32] [--l1-seed 0] [--l1-eval-boards 1024 (even, >=2)] [--l1-eval-seed 0] [--l1-train-control true] [--l1-train-regression true] [--l1-eval-control true] [--l1-eval-regression true] [--l1-sampling random|stratified (stratified)] [--l1-eval-sampling random|stratified (random)] [--l1-postflop-beta 0] [--l1-refine none|texture:0|1|2,recall:N (none)] [--l1-cards recommended:PATH (the recommended card abstraction of l1::CardSpec over the 128-bucket EHS2 table PATH; the --l1-relative*, --l1-flop-groups, --l1-leaf-flop-groups, --l1-hand and --l1-runout flags given replace its parts; its own flop groups per L1 leaf come from the --fixed-profile in fixed-profile mode and otherwise need --l1-leaf-flop-groups own:PROFILE or none)] [--l1-relative BUCKETS[xLEVELS]:PATH (128-bucket EHS2 table; percentile buckets in the actor's reach, crossed with LEVELS strength ranges and the --l1-refine texture)] [--l1-relative-prior COMBOS (default 0; finite, >=0; requires --l1-relative)] [--l1-relative-reach current|average (default current; requires --l1-relative; training mapping only)] [--l1-hand none|PART[,PART...] (experimental; PART made:N|draws|potential:N|flop-draws|pairs|backdoor|gutshot, or category[:N] alone; splits the relative buckets on the flop and turn by made-hand percentile levels, flush/open-ended straight draws, pocket-pair status and percentile levels of the variance of the strength bucket over the next card, or on every street by the made-hand and draw category (10 made hands x 6 draws, the river 10 made hands) times N strength levels within the category; requires --l1-relative)] [--l1-runout none|KIND[,KIND...] (experimental; KIND flush|pair|straight|over; splits the relative buckets on the turn and river by the first enabled kind, in the order flush, pair, straight, over, that the card just dealt matches, else blank; requires --l1-relative)] [--l1-flop-groups none|recommended|FILE[:flop] (experimental; `:flop` splits only the flop; FILE has `card card card group` for each of the 1755 isomorphic flops, recommended is the bundled 49 groups; splits the relative buckets by the flop group on every street; requires --l1-relative)] [--l1-flop-subset FILE (weighted representative-flop training game)] [--tables-dir PATH] [--t3-samples 4096] [--t3-seed 0] [--k4-samples 2048] [--seed 0] [--solver-k4-samples N] [--solver-k4-min-samples M (requires N)] [--threads N] [--iterations 1000] [--eval-every 100] [--target-nash-conv X] [--alpha 1.5] [--beta 0 (l0) / 1 (l1)] [--gamma 2] [--print-every 0] [--output JSON] [--output-profile JSON] [--output-postflop FILE (l1 only)] [--state FILE (l1 live or fixed-profile solve; resume if present, atomic saves)] [--state-every SECONDS (l1 only; default 1800; also saves at completion)] [--max-seconds SECONDS (l1 only; total solve time including resume)] [--snapshot-dir DIR (live l1 only; profile/postflop at every checkpoint)] [--evaluate-profile JSON --evaluate-postflop FILE (l1 only)] [--fixed-profile JSON [--fixed-lines LINE[;LINE...]] [--fixed-flops FILE] [--fixed-snapshots N[,N...] (also writes the Postflop average after these iterations to FILE.itN)] (l1 only; requires --output-postflop; supports --state and --state-every)]\nEvaluation mode skips solving; solver-only flags are ignored, except --output-profile and --output-postflop which are rejected; state, time limit and snapshot options are also rejected.\nFixed-profile mode keeps the trunk at the class profile and trains only the Postflop of the L1 leaves at the action-label lines (label/label/..., all leaves without --fixed-lines), on the flops of --fixed-flops (one per line, `Ks 7h 2d` for historical round robin with uniform turns and rivers, or `Ks 7h 2d 24` for weighted stratified flops and runouts) when given; it writes the Postflop average and rejects --output-profile and --snapshot-dir. --state resumes the full Postflop training state after its saved iteration; the iteration target may be extended, and fixed snapshots at past iterations are not rewritten.\n--k4-samples and --seed define the model/evaluator; solver K4 flags use iteration-varying samples only during solving."
            );
            println!(
                "--l1-flop-subset FILE (experimental full L1 solve; card card card weight lines; positive weights; one representative per --l1-flop-groups group; training only, evaluation uses all flops)\n--fixed-flops FILE accepts card card card lines (historical round robin) or card card card weight lines (weighted stratified sampling); do not mix formats."
            );
            println!(
                "--l1-flop-subset-max-spr X (experimental; finite nonnegative flop SPR limit, inclusive; requires --l1-flop-subset or weighted --fixed-flops; higher-SPR leaves use standard training boards)"
            );
            println!("--stop-file PATH (l1 only; stop normally when PATH exists)");
            println!(
                "--l1-eval-override FILE (experimental class-value overrides; requires --evaluate-profile)"
            );
            println!(
                "--l1-train-override FILE (experimental; fix the L1 leaf values of the file's (line, seat) classes during a live L1 solve and its checkpoints; same format as --l1-eval-override; not with --l1-qre-prior)"
            );
            println!(
                "--l1-boards-late BOARDS:FROM (experimental; from iteration FROM on, train on BOARDS boards per iteration instead of --l1-boards; L1 live and fixed-profile solves)"
            );
            println!(
                "--l1-qre-prior LAMBDA (experimental mass-preserving logit prior, bb^-1; live current-relative L1 only; requires positive prior; disables training regression)"
            );
            println!(
                "--l1-relative-prior-fraction F (experimental; adds F times the actor's reach total at each node to the --l1-relative-prior combos, default 0; requires --l1-relative; not with --l1-qre-prior)"
            );
            println!(
                "--l1-relative-opponent-levels none|STREET[,STREET...] (STREET flop|turn|river; on those streets the LEVELS of --l1-relative or --l1-cards are equal ranges of the combo's showdown equity on the board so far against the opponent's reach at the node instead of the strength table; --l1-cards recommended: flop,turn,river; requires LEVELS > 1; not with --l1-low-spr)"
            );
            println!(
                "--l1-relative-low-spr-levels MAX_SPR:[BUCKETSx]LEVELS|none (L1 leaves whose flop SPR is at most MAX_SPR have BUCKETS percentile buckets (default those of --l1-relative or --l1-cards) and LEVELS strength levels instead of the LEVELS of --l1-relative or --l1-cards; LEVELS divides those; --l1-cards recommended: 2.5:2; requires --l1-relative-opponent-levels flop,turn,river; not with --l1-low-spr or --l1-flop-subset)"
            );
            println!(
                "--l1-low-spr MAX_SPR:BUCKETS[xLEVELS]:GROUPS (experimental; refine only low-SPR leaves; GROUPS = none|recommended|FILE[:flop]; requires --l1-relative or --l1-cards)"
            );
            println!(
                "--l1-leaf-flop-groups none|own[:K]:PROFILE|FILE (which L1 leaves have their own flop groups instead of --l1-flop-groups. none: no leaf. own: every leaf that a seat reaches under the class profile PROFILE, with K (default 49) groups weighted by the leaf's two ranges; for a full solve, PROFILE is the --output-profile of the same solve with none. FILE (experimental): `LINE GROUPS` rows, GROUPS = none|recommended|FILE[:flop] relative to FILE's directory. own and FILE require --l1-relative or --l1-cards and exclude --l1-low-spr, --l1-flop-subset and weighted --fixed-flops)\n--l1-runout-max-slots N (experimental; the --l1-runout split only at leaves whose strategy with it has at most N slots, none elsewhere: the size of the tree after the leaf decides; same requirements)"
            );
            println!(
                "--l1-regret-weight counterfactual|own (experimental; own weights each combo's postflop regret by its own reach over its bucket's mean, default counterfactual)"
            );
            println!(
                "--l1-strength-buckets K (experimental; the --l1-relative or --l1-cards strength table has K buckets per street instead of 128)"
            );
            println!(
                "--l1-refine recall:N keeps N coarse ranges of the original flop strength on the turn and river in relative mode (previous-street strength in fixed-bucket mode).\n--l1-hand flop-draws (can combine with draws) keeps a binary flop flush-draw/OESD class on the turn and river; no backdoor/gutshot split.\n--l1-hand pairs adds a pocket-pair / non-pair split on the flop and turn; can combine with made, draws, potential and flop-draws.\n--l1-hand backdoor / gutshot (with draws) add a third flush level (backdoor flush draw) / straight level (gutshot) on the flop only."
            );
            return Ok(());
        }
        let value = args
            .next()
            .with_context(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--state" => run_state.file = Some(value.into()),
            "--state-every" => run_state.every = Some(value.parse()?),
            "--snapshot-dir" => run_state.snapshots = Some(value.into()),
            "--max-seconds" => l1_options.max_seconds = Some(value.parse()?),
            "--stop-file" => stop_file = Some(value.into()),
            "--leaf-model" => leaf_model = value,
            "--ehs2-cache" => ehs2_cache = Some(PathBuf::from(value)),
            "--l1-boards" => l1_options.l1_boards = value.parse()?,
            "--l1-boards-late" => l1_options.l1_boards_late = Some(l1::LateBoards::parse(&value)?),
            "--l1-seed" => l1_options.l1_seed = value.parse()?,
            "--l1-eval-boards" => l1_options.l1_eval_boards = value.parse()?,
            "--l1-eval-seed" => l1_options.l1_eval_seed = value.parse()?,
            "--l1-train-control" => l1_options.l1_train_control = value.parse()?,
            "--l1-train-regression" => l1_options.l1_train_regression = value.parse()?,
            "--l1-eval-control" => l1_options.l1_eval_control = value.parse()?,
            "--l1-eval-regression" => l1_options.l1_eval_regression = value.parse()?,
            "--l1-sampling" => l1_options.l1_sampling = sampling(&value)?,
            "--l1-eval-sampling" => l1_options.l1_eval_sampling = sampling(&value)?,
            "--l1-postflop-beta" => l1_options.l1_postflop_beta = value.parse()?,
            "--l1-regret-weight" => {
                l1_options.l1_own_reach_regret = match value.as_str() {
                    "counterfactual" => false,
                    "own" => true,
                    _ => bail!("--l1-regret-weight must be counterfactual or own"),
                }
            }
            "--config" => config = Some(PathBuf::from(value)),
            "--tables-dir" => tables_dir = value.into(),
            "--t3-samples" => t3_samples = value.parse()?,
            "--t3-seed" => t3_seed = value.parse()?,
            "--k4-samples" => model_options.k4_samples = value.parse()?,
            "--solver-k4-samples" => options.k4_samples = Some(value.parse()?),
            "--solver-k4-min-samples" => options.k4_min_samples = Some(value.parse()?),
            "--seed" => model_options.seed = value.parse()?,
            "--threads" => threads = Some(value.parse::<usize>()?),
            "--iterations" => options.iterations = value.parse()?,
            "--eval-every" => options.eval_every = value.parse()?,
            "--target-nash-conv" => options.target_nash_conv = Some(value.parse()?),
            "--alpha" => options.alpha = value.parse()?,
            "--beta" => beta = Some(value.parse()?),
            "--gamma" => options.gamma = value.parse()?,
            "--print-every" => print_every = value.parse::<u64>()?,
            "--output" => output = Some(PathBuf::from(value)),
            "--output-profile" => output_profile = Some(PathBuf::from(value)),
            "--output-postflop" => output_postflop = Some(PathBuf::from(value)),
            "--evaluate-profile" => evaluate_profile = Some(PathBuf::from(value)),
            "--fixed-profile" => fixed_profile = Some(PathBuf::from(value)),
            "--fixed-lines" => fixed_lines = Some(value),
            "--fixed-flops" => fixed_flops_path = Some(PathBuf::from(value)),
            "--fixed-snapshots" => {
                fixed_snapshots = value
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .context("--fixed-snapshots is a comma list of iterations")?
            }
            "--evaluate-postflop" => evaluate_postflop = Some(PathBuf::from(value)),
            "--l1-eval-override" => evaluate_override = Some(PathBuf::from(value)),
            "--l1-train-override" => train_override = Some(PathBuf::from(value)),
            "--l1-refine" => {
                refinement = l1::Refinement::parse(&value)
                    .context("--l1-refine is none or texture:0|1|2,recall:N")?
            }
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
            "--l1-qre-prior" => l1_options.qre = Some(l1::Qre::parse(&value)?),
            "--l1-flop-groups" => flop_groups = Some(value),
            "--l1-flop-subset" => flop_subset_path = Some(PathBuf::from(value)),
            "--l1-flop-subset-max-spr" => l1_options.l1_flop_subset_max_spr = Some(value.parse()?),
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
            "--l1-relative-reach" => {
                relative_reach = Some(match value.as_str() {
                    "current" => l1::RelativeReach::Current,
                    "average" => l1::RelativeReach::Average,
                    _ => bail!("--l1-relative-reach must be current or average"),
                });
            }
            _ => bail!("unknown argument {arg}"),
        }
    }
    let flop_subset = flop_subset_path
        .as_deref()
        .map(|path| {
            l1::WeightedFlopSubset::parse(
                &std::fs::read_to_string(path)
                    .with_context(|| format!("reading {}", path.display()))?,
            )
        })
        .transpose()?;
    l1_options.l1_flop_subset = flop_subset.as_ref();
    ensure!(
        l1_options.l1_flop_subset_max_spr.is_none()
            || flop_subset.is_some()
            || (fixed_profile.is_some() && fixed_flops_path.is_some()),
        "--l1-flop-subset-max-spr requires --l1-flop-subset (or weighted --fixed-flops in fixed-profile mode)"
    );
    l1_options.stop_file = stop_file.as_deref();
    // The recommended card abstraction fills every part not given explicitly.
    if let Some(path) = cards {
        let recommended = l1::CardSpec::recommended();
        relative.get_or_insert((recommended.buckets[0], recommended.levels, path));
        relative_prior.get_or_insert(recommended.prior);
        relative_reach.get_or_insert(recommended.reach);
        if recommended.flops.is_some() {
            flop_groups.get_or_insert_with(|| "recommended".into());
        }
        hand.get_or_insert(recommended.hand);
        runout.get_or_insert(recommended.runout);
        opponent_levels.get_or_insert(recommended.opponent_levels);
        if leaf_model == "l1" {
            low_spr_levels.get_or_insert(recommended.low_spr_levels);
        }
        // Each leaf's own groups need its ranges: the fixed profile's, or a
        // profile named explicitly.
        if let Some(groups) = recommended.leaf_flops
            && leaf_model == "l1"
            && leaf_flop_groups.is_none()
        {
            let profile = fixed_profile.clone().context(
                "--l1-cards recommended gives every L1 leaf its own flop groups from the leaf's \
                 ranges: add --l1-leaf-flop-groups own:PROFILE (a class profile, such as the \
                 --output-profile of the same solve with --l1-leaf-flop-groups none) or none",
            )?;
            leaf_flop_groups = Some(l1::LeafFlopGroups::Own { groups, profile });
        }
    }
    let low_spr_levels = low_spr_levels.flatten();
    ensure!(
        low_spr.is_none() || (leaf_model == "l1" && relative.is_some()),
        "--l1-low-spr requires L1 with --l1-relative or --l1-cards"
    );
    let per_leaf = leaf_flop_groups
        .as_ref()
        .is_some_and(|g| *g != l1::LeafFlopGroups::None)
        || runout_max_slots.is_some()
        || low_spr_levels.is_some();
    ensure!(
        !per_leaf
            || (leaf_model == "l1"
                && relative.is_some()
                && low_spr.is_none()
                && flop_subset_path.is_none()),
        "--l1-leaf-flop-groups, --l1-runout-max-slots and --l1-relative-low-spr-levels require L1 \
         with --l1-relative or --l1-cards, and exclude --l1-low-spr and --l1-flop-subset"
    );
    ensure!(
        runout_max_slots.is_none() || runout.is_some_and(|r| r != l1::Runout::default()),
        "--l1-runout-max-slots requires --l1-runout KINDS"
    );
    ensure!(
        relative.is_some()
            || (relative_prior.is_none()
                && relative_prior_fraction.is_none()
                && relative_reach.is_none()
                && opponent_levels.is_none()
                && flop_groups.is_none()
                && hand.is_none()
                && runout.is_none()),
        "--l1-relative-prior, --l1-relative-prior-fraction, --l1-relative-reach, --l1-relative-opponent-levels, --l1-flop-groups, --l1-hand and --l1-runout require --l1-relative or --l1-cards"
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
    ensure!(
        relative_prior_fraction == 0.0 || l1_options.qre.is_none(),
        "--l1-relative-prior-fraction cannot be combined with --l1-qre-prior"
    );
    let relative_reach = relative_reach.unwrap_or_default();
    ensure!(
        evaluate_profile.is_some() == evaluate_postflop.is_some(),
        "--evaluate-profile and --evaluate-postflop must be provided together"
    );
    ensure!(
        evaluate_override.is_none() || evaluate_profile.is_some(),
        "--l1-eval-override requires --evaluate-profile"
    );
    let evaluating = evaluate_profile.is_some();
    ensure!(
        train_override.is_none() || (leaf_model == "l1" && !evaluating && fixed_profile.is_none()),
        "--l1-train-override requires a live L1 trunk solve"
    );
    ensure!(
        train_override.is_none() || l1_options.qre.is_none(),
        "--l1-train-override cannot be combined with --l1-qre-prior"
    );
    ensure!(
        flop_subset.is_none() || (leaf_model == "l1" && !evaluating && fixed_profile.is_none()),
        "--l1-flop-subset requires a live full L1 trunk solve; use weighted --fixed-flops for fixed-profile mode"
    );
    ensure!(
        l1_options.qre.is_none() || (leaf_model == "l1" && !evaluating && fixed_profile.is_none()),
        "--l1-qre-prior requires a live L1 trunk solve"
    );
    if l1_options.qre.is_some() {
        l1_options.l1_train_regression = false;
    }
    ensure!(
        fixed_profile.is_some()
            || (fixed_lines.is_none() && fixed_flops_path.is_none() && fixed_snapshots.is_empty()),
        "--fixed-lines, --fixed-flops and --fixed-snapshots require --fixed-profile"
    );
    ensure!(
        fixed_profile.is_none()
            || (leaf_model == "l1"
                && !evaluating
                && output_postflop.is_some()
                && output_profile.is_none()
                && run_state.snapshots.is_none()),
        "--fixed-profile requires --leaf-model l1 and --output-postflop, and excludes evaluation, \
         --output-profile and --snapshot-dir"
    );
    let state_flags = run_state.file.is_some()
        || run_state.every.is_some()
        || run_state.snapshots.is_some()
        || l1_options.max_seconds.is_some()
        || l1_options.stop_file.is_some();
    ensure!(
        !state_flags || (leaf_model == "l1" && !evaluating),
        "state, time limit, stop file and snapshot options require L1 solving"
    );
    ensure!(
        run_state.every.is_none_or(|x| x.is_finite() && x > 0.0),
        "--state-every must be finite and positive"
    );
    ensure!(
        l1_options
            .max_seconds
            .is_none_or(|x| x.is_finite() && x > 0.0),
        "--max-seconds must be finite and positive"
    );
    ensure!(
        leaf_model == "l1" || (!evaluating && output_postflop.is_none()),
        "--output-postflop and saved evaluation require --leaf-model l1"
    );
    ensure!(
        !evaluating || (output_profile.is_none() && output_postflop.is_none()),
        "--output-profile and --output-postflop are not allowed in evaluation mode"
    );
    if !evaluating {
        ensure!(
            options.k4_samples != Some(0),
            "solver K4 samples must be positive"
        );
        ensure!(
            options
                .k4_min_samples
                .is_none_or(|min| min >= 1 && options.k4_samples.is_some_and(|n| min <= n)),
            "--solver-k4-min-samples requires --solver-k4-samples and 1 <= min <= samples"
        );
    }
    ensure!(
        leaf_model == "l0" || leaf_model == "l1",
        "--leaf-model must be l0 or l1"
    );
    // The leaf models' own defaults; --beta overrides either.
    if leaf_model == "l1" {
        options.beta = l1_options.trunk.beta;
    }
    if let Some(beta) = beta {
        options.beta = beta;
    }
    ensure!(
        leaf_model != "l1" || ehs2_cache.is_some(),
        "--ehs2-cache is required for l1"
    );
    ensure!(
        (evaluating || l1_options.l1_boards > 0)
            && l1_options.l1_eval_boards >= 2
            && l1_options.l1_eval_boards.is_multiple_of(2),
        "invalid L1 board counts"
    );
    let source = config.context("--config is required")?;
    ensure!(threads != Some(0), "threads must be positive");
    let total = Instant::now();
    let game = l0::game_from_config(&std::fs::read_to_string(&source)?, &source)?;
    let start = Instant::now();
    let tree = Tree::build_with(
        &game,
        if leaf_model == "l1" {
            l0::FlopLeaves::L1
        } else {
            l0::FlopLeaves::Checkdown
        },
    )?;
    if leaf_model == "l0" && tree.flop_leaves > 0 {
        println!("Postflop menus solved in L0 Checkdown mode.");
    }
    let tree_seconds = start.elapsed().as_secs_f64();
    println!(
        "Tree: {} decisions, {} terminals ({tree_seconds:.3}s)",
        tree.nodes.iter().filter(|n| n.actor.is_some()).count(),
        tree.terminal_counts().iter().sum::<usize>()
    );
    let start = Instant::now();
    let t2 = HuShowdownTable::load_or_build(&tables_dir)?;
    let t3 = ThreeWayTable::load_or_build(&tables_dir, t3_samples, t3_seed)?;
    let tables_seconds = start.elapsed().as_secs_f64();
    let tables = (&t2, &t3);
    let start = Instant::now();
    let model = Model::new(&game, &tables, model_options)?;
    let model_seconds = start.elapsed().as_secs_f64();
    println!("Tables: {tables_seconds:.3}s; model: {model_seconds:.3}s");
    let mut builder = rayon::ThreadPoolBuilder::new();
    if let Some(threads) = threads {
        builder = builder.num_threads(threads);
    }
    let pool = builder.build()?;
    if leaf_model == "l1" {
        let (train_overrides, train_override_metadata) = train_override
            .as_deref()
            .map(|path| load_overrides(&tree, path))
            .transpose()?
            .unwrap_or_else(|| (Vec::new(), serde_json::Value::Null));
        l1_options.l1_train_overrides = &train_overrides;
        l1_options.trunk = options;
        return run_l1(
            &source,
            ehs2_cache.as_ref().unwrap(),
            &tree,
            &model,
            &pool,
            l1_options,
            run_state,
            print_every,
            output.as_deref(),
            output_profile.as_deref(),
            output_postflop.as_deref(),
            evaluate_profile
                .as_deref()
                .zip(evaluate_postflop.as_deref())
                .map(|(profile, postflop)| (profile, postflop, evaluate_override.as_deref())),
            train_override_metadata,
            &t2,
            refinement,
            relative,
            (relative_prior, relative_prior_fraction),
            strength_buckets,
            flop_groups.as_deref(),
            hand,
            runout,
            relative_reach,
            opponent_levels,
            low_spr.as_deref(),
            (leaf_flop_groups.as_ref(), runout_max_slots, low_spr_levels),
            fixed_profile.as_deref().map(|profile| Fixed {
                profile,
                lines: fixed_lines.as_deref(),
                flops: fixed_flops_path.as_deref(),
                snapshots: &fixed_snapshots,
            }),
            &t3,
            tree_seconds,
            tables_seconds,
            model_seconds,
            total,
        );
    }
    let mut previous = SolveTimings::default();
    let mut iteration_start = Instant::now();
    let observer = |progress: &l0::Progress<'_>| {
        let now = Instant::now();
        if print_every > 0
            && progress.iteration > 0
            && progress.iteration.is_multiple_of(print_every)
        {
            let t = progress.timings;
            println!(
                "Iteration {}: wall {:.6}s; reaches {:.6}; t2 {:.6}; t3 {:.6}; k4 {:.6}; update {:.6}; evaluation {:.6}",
                progress.iteration,
                now.duration_since(iteration_start).as_secs_f64(),
                t.reaches - previous.reaches,
                t.t2 - previous.t2,
                t.t3 - previous.t3,
                t.k4 - previous.k4,
                t.update - previous.update,
                t.evaluation - previous.evaluation
            );
        }
        if let Some(c) = progress.checkpoint {
            println!(
                "Checkpoint {}: NashConv {:.12}; gains {:?}; {:.6}s",
                c.iteration,
                c.nash_conv,
                c.seats.iter().map(|s| s.gain).collect::<Vec<_>>(),
                c.seconds
            );
        }
        previous = progress.timings.clone();
        iteration_start = Instant::now();
    };
    let solution = pool.install(|| l0::solve(&tree, &model, options, observer))?;
    println!(
        "Iterations: {}; reached target: {}; solve phases {:?}",
        solution.iterations, solution.reached_target, solution.timings
    );
    for warning in &model.warnings {
        println!("Warning: {warning}");
    }
    if let Some(path) = output_profile {
        write_json(&path, &solution.average.export(&tree))?;
    }
    if let Some(path) = output {
        let fingerprint: String = tree
            .game_fingerprint
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        write_json(
            &path,
            &json!({
                "format": "p2-trunk-solve",
                "version": 1,
                "source": source,
                "game_fingerprint": fingerprint,
                "tables": {
                    "t2": {"file": "t2-v1.bin", "payload_blake3": t2.payload_hash().to_hex().as_str()},
                    "t3": {
                        "file": format!("t3-v1-n{t3_samples}-seed{t3_seed}.bin"),
                        "payload_blake3": t3.payload_hash().to_hex().as_str(),
                        "samples": t3_samples,
                        "seed": t3_seed
                    }
                },
                "k4": {"samples": model_options.k4_samples, "seed": model_options.seed},
                "options": options,
                "iterations": solution.iterations,
                "reached_target": solution.reached_target,
                "checkpoints": solution.checkpoints,
                "terminals": {"counts_by_k": tree.terminal_counts()},
                "warnings": model.warnings,
                "timings": {
                    "tree": tree_seconds,
                    "tables": tables_seconds,
                    "model": model_seconds,
                    "solve": solution.timings,
                    "total": total.elapsed().as_secs_f64()
                }
            }),
        )?;
    }
    Ok(())
}

fn load_overrides(tree: &Tree, path: &Path) -> Result<(Vec<l1::LeafOverride>, serde_json::Value)> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let document: OverrideDocument = serde_json::from_slice(&bytes)?;
    ensure!(
        document.format == "p2-l1-eval-override" && document.version == 1,
        "unsupported L1 evaluation override format/version"
    );
    let catalog = mw_preflop::trunk::classes::Classes::get();
    let mut overrides = Vec::new();
    for entry in document.entries {
        let terminal = l1::leaf_of_line(tree, &entry.line)?;
        ensure!(
            tree.nodes[terminal]
                .terminal
                .as_ref()
                .unwrap()
                .active
                .contains(&entry.seat),
            "seat {} is not at leaf {terminal}",
            entry.seat
        );
        let mut values = [None; 169];
        for (name, value) in entry.values {
            let c = (0..169)
                .find(|&c| catalog.name(c) == name)
                .with_context(|| format!("unknown class {name}"))?;
            values[c] = Some(value);
        }
        overrides.push(l1::LeafOverride {
            terminal,
            seat: entry.seat,
            values,
        });
    }
    let metadata = json!({
        "path": path,
        "blake3": blake3::hash(&bytes).to_hex().as_str(),
        "entries": overrides.len(),
    });
    Ok((overrides, metadata))
}

#[allow(clippy::too_many_arguments)]
fn run_l1(
    source: &Path,
    cache: &Path,
    tree: &Tree,
    model: &Model<'_>,
    pool: &rayon::ThreadPool,
    options: l1::Options,
    run_state: RunState,
    print_every: u64,
    output: Option<&Path>,
    output_profile: Option<&Path>,
    output_postflop: Option<&Path>,
    evaluation_inputs: Option<(&Path, &Path, Option<&Path>)>,
    train_override_metadata: serde_json::Value,
    t2: &HuShowdownTable,
    refinement: l1::Refinement,
    relative: Option<(usize, usize, PathBuf)>,
    relative_prior: (f64, f64),
    strength_buckets: u32,
    flop_groups: Option<&str>,
    hand: l1::HandSplit,
    runout: l1::Runout,
    relative_reach: l1::RelativeReach,
    opponent_levels: [bool; 3],
    low_spr: Option<&str>,
    (leaf_flop_groups, runout_max_slots, low_spr_levels): (
        Option<&l1::LeafFlopGroups>,
        Option<usize>,
        Option<l1::LowSprLevels>,
    ),
    fixed: Option<Fixed<'_>>,
    t3: &ThreeWayTable,
    tree_seconds: f64,
    tables_seconds: f64,
    model_seconds: f64,
    total: Instant,
) -> Result<()> {
    use mw_preflop::{
        Street,
        card_abstraction::{Ehs2Abstraction, Ehs2Params},
    };
    let counts = &model.game().config().abstraction;
    let params = Ehs2Params {
        flop_buckets: u32::from(counts.flop_buckets),
        turn_buckets: u32::from(counts.turn_buckets),
        river_buckets: u32::from(counts.river_buckets),
    };
    ensure!(
        [
            params.flop_buckets,
            params.turn_buckets,
            params.river_buckets
        ]
        .iter()
        .all(|&n| n > 0 && n <= u16::MAX as u32),
        "invalid bucket counts"
    );
    let abstraction = Ehs2Abstraction::load_or_build(
        params,
        &[Street::Flop, Street::Turn, Street::River],
        Some(cache),
    );
    let mut hasher = blake3::Hasher::new();
    let bytes = std::io::copy(
        &mut std::fs::File::open(cache).with_context(|| format!("reading {}", cache.display()))?,
        &mut hasher,
    )?;
    let leaf_metadata = match leaf_flop_groups {
        None => serde_json::Value::Null,
        Some(l1::LeafFlopGroups::None) => json!("none"),
        Some(l1::LeafFlopGroups::Rows(path)) => json!(path),
        Some(l1::LeafFlopGroups::Own { groups, profile }) => json!({
            "own": groups,
            "profile": profile,
            "blake3": blake3::hash(
                &std::fs::read(profile).with_context(|| format!("reading {}", profile.display()))?
            )
            .to_hex()
            .as_str(),
        }),
    };
    let ehs2_metadata = json!({
        "path": cache,
        "bucket_counts": params,
        "bytes": bytes,
        "blake3": hasher.finalize().to_hex().as_str(),
        "refinement": refinement,
        "relative": relative.as_ref().map(|(n, levels, path)| json!({"buckets": n, "levels": levels, "strength_table": path, "strength_buckets": strength_buckets, "prior": relative_prior.0, "prior_fraction": relative_prior.1, "reach": relative_reach, "opponent_levels": opponent_levels, "flop_groups": flop_groups, "hand": format!("{hand:?}"), "runout": format!("{runout:?}")})),
        "low_spr": low_spr,
        "leaf_flop_groups": leaf_metadata,
        "runout_max_slots": runout_max_slots,
        "low_spr_levels": low_spr_levels.map(|rule| json!({"max_spr": rule.max_spr, "buckets": rule.buckets, "levels": rule.levels})),
    });
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
        .map(l1::FlopClusters::from_spec)
        .transpose()?
        .flatten();
    if let (Some(subset), Some(groups)) = (options.l1_flop_subset, flop_groups.as_ref()) {
        subset.validate_groups(groups)?;
    }
    let relative_source = relative
        .as_ref()
        .zip(strength.as_ref())
        .map(|((n, levels, _), s)| l1::Relative {
            strength: s,
            counts: [*n; 3],
            levels: *levels,
            prior: relative_prior.0,
            prior_fraction: relative_prior.1,
            reach: relative_reach,
            refinement,
            flops: flop_groups.as_ref(),
            hand,
            runout,
            opponent_levels,
        });
    let low_spr_spec = low_spr.map(l1::LowSprSpec::parse).transpose()?;
    let low_spr_source = relative_source
        .as_ref()
        .zip(low_spr_spec.as_ref())
        .map(|(base, spec)| l1::LowSprCards::new(*base, spec))
        .transpose()?;
    // Per-leaf cards: listed or own flop groups, and the runout split only
    // where it keeps the leaf within the slot limit (the base has none then).
    let leaf_groups = match (leaf_flop_groups, strength.as_ref()) {
        (Some(spec), Some(strength)) => {
            let start = Instant::now();
            let groups = pool.install(|| spec.resolve(tree, model, strength))?;
            if let l1::LeafFlopGroups::Own { .. } = spec {
                println!("L1 own flop groups: {:.3}s", start.elapsed().as_secs_f64());
            }
            groups
        }
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
            l1::LeafCards::new(tree, base, &spec)
        })
        .transpose()?;
    let buckets: &dyn l1::BucketSource = match (&leaf_source, &low_spr_source, &relative_source) {
        (Some(r), _, _) => r,
        (_, Some(r), _) => r,
        (_, _, Some(r)) => r,
        _ => &refined,
    };
    let (leaf_count, decisions, slots) = l1::Strategies::layout(tree, buckets);
    println!(
        "L1: {leaf_count} leaves; {decisions} postflop decisions; {slots} slots; {} storage bytes",
        slots * 16
    );
    if let Some(cards) = &leaf_source {
        let (flops, runout, levels) = cards.changed_leaves();
        println!(
            "L1 leaf cards: {flops} leaves with their own flop groups, {runout} with the runout split, \
             {levels} with other percentile buckets or strength levels"
        );
    }
    if let Some(Fixed {
        profile: profile_path,
        lines,
        flops: flops_path,
        snapshots,
    }) = fixed
    {
        // Fixed solving allocates only the trained leaves (`Strategies::new_for`).
        let fixed_flops = flops_path
            .map(|path| {
                l1::FixedFlops::parse(
                    &std::fs::read_to_string(path)
                        .with_context(|| format!("reading {}", path.display()))?,
                )
            })
            .transpose()?;
        if let (Some(l1::FixedFlops::Weighted(subset)), Some(groups)) =
            (fixed_flops.as_ref(), flop_groups.as_ref())
        {
            subset.validate_groups(groups)?;
        }
        ensure!(
            leaf_source.is_none() || !matches!(fixed_flops, Some(l1::FixedFlops::Weighted(_))),
            "per-leaf cards exclude weighted --fixed-flops"
        );
        let document: l0::ClassProfileDocument = serde_json::from_slice(
            &std::fs::read(profile_path)
                .with_context(|| format!("reading {}", profile_path.display()))?,
        )?;
        let profile = l0::Profile::from_json(tree, &document)?;
        drop(document);
        let leaves = lines
            .map(|lines| {
                lines
                    .split(';')
                    .map(|line| l1::leaf_of_line(tree, line))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let start = Instant::now();
        let mut last = Instant::now();
        let postflop = pool.install(|| {
            l1::solve_postflop_fixed_with_state(
                tree,
                model,
                buckets,
                &profile,
                options,
                leaves.as_deref(),
                fixed_flops.as_ref(),
                run_state.file.as_deref().map(|path| l1::FixedState {
                    path,
                    every_seconds: run_state.every.unwrap_or(1800.0),
                }),
                |t, postflop| {
                    if print_every > 0 && t.is_multiple_of(print_every) {
                        println!("Iteration {t}: wall {:.6}s", last.elapsed().as_secs_f64());
                        last = Instant::now();
                    }
                    if snapshots.contains(&t) {
                        let mut path = output_postflop.unwrap().as_os_str().to_owned();
                        path.push(format!(".it{t}"));
                        postflop.write_average(tree, &mut std::fs::File::create(path)?)?;
                    }
                    Ok(())
                },
            )
        })?;
        let seconds = start.elapsed().as_secs_f64();
        postflop.write_average(tree, &mut std::fs::File::create(output_postflop.unwrap())?)?;
        println!("Fixed-profile postflop: {seconds:.3}s");
        if let Some(path) = output {
            write_json(
                path,
                &json!({
                    "format": "p2-trunk-fixed-postflop",
                    "version": 1,
                    "source": source,
                    "game_fingerprint": blake3::Hash::from(tree.game_fingerprint).to_hex().as_str(),
                    "profile": profile_path,
                    "lines": lines,
                    "flops": flops_path,
                    "leaves": leaves,
                    "ehs2": ehs2_metadata,
                    "options": options,
                    "seconds": seconds,
                }),
            )?;
        }
        return Ok(());
    }
    let mut storage = l1::Strategies::new(tree, buckets);
    if let Some((profile_path, postflop_path, override_path)) = evaluation_inputs {
        let start = Instant::now();
        let profile_bytes = std::fs::read(profile_path)
            .with_context(|| format!("reading {}", profile_path.display()))?;
        let profile_hash = blake3::hash(&profile_bytes);
        let document: l0::ClassProfileDocument = serde_json::from_slice(&profile_bytes)?;
        let profile = l0::Profile::from_json(tree, &document)?;
        drop(document);
        drop(profile_bytes);
        let mut postflop_file = std::fs::File::open(postflop_path)
            .with_context(|| format!("reading {}", postflop_path.display()))?;
        // Hash the same stream that read_average validates, including the checksum.
        let mut postflop_hash = blake3::Hasher::new();
        storage.read_average(
            tree,
            &mut HashingReader {
                input: &mut postflop_file,
                hash: &mut postflop_hash,
            },
        )?;
        let (overrides, override_metadata) = override_path
            .map(|path| load_overrides(tree, path))
            .transpose()?
            .unwrap_or_else(|| (Vec::new(), serde_json::Value::Null));
        let load_seconds = start.elapsed().as_secs_f64();
        let start = Instant::now();
        let boards = pool.install(|| l1::evaluation_boards(buckets, &options));
        let boards_seconds = start.elapsed().as_secs_f64();
        let evaluation = pool.install(|| {
            l1::evaluate_with_overrides(
                tree,
                &profile,
                model,
                &storage,
                &boards,
                options.l1_eval_control,
                options.l1_eval_regression,
                &overrides,
            )
        })?;
        let e = &evaluation;
        let gains = |f: fn(&l1::SeatEvaluation) -> f64| e.seats.iter().map(f).collect::<Vec<_>>();
        println!(
            "Evaluation: NashConv {:.12}; held {:.12}; auxiliary {:.12}; auxiliary held {:.12}; \
             gains {:?}; held gains {:?}; auxiliary gains {:?}; auxiliary held gains {:?}; \
             evaluation {:.6}s",
            e.nash_conv,
            e.held_nash_conv,
            e.auxiliary_nash_conv,
            e.auxiliary_held_nash_conv,
            gains(|s| s.gain),
            gains(|s| s.held_gain),
            gains(|s| s.auxiliary_gain),
            gains(|s| s.auxiliary_held_gain),
            e.seconds
        );
        if let Some(path) = output {
            write_json(
                path,
                &json!({
                    "format": "p2-trunk-evaluation",
                    "version": 1,
                    "leaf_model": "l1",
                    "source": source,
                    "game_fingerprint": blake3::Hash::from(tree.game_fingerprint).to_hex().as_str(),
                    "profile": {"path": profile_path, "blake3": profile_hash.to_hex().as_str()},
                    "postflop": {"path": postflop_path, "blake3": postflop_hash.finalize().to_hex().as_str()},
                    "override": override_metadata,
                    "ehs2": ehs2_metadata,
                    "tables": {
                        "t2": {"payload_blake3": t2.payload_hash().to_hex().as_str()},
                        "t3": {"payload_blake3": t3.payload_hash().to_hex().as_str()},
                    },
                    "k4": {"samples": model.evaluation_options().k4_samples, "seed": model.evaluation_options().seed},
                    "evaluation_options": {
                        "boards": options.l1_eval_boards, "seed": options.l1_eval_seed,
                        "sampling": options.l1_eval_sampling, "control": options.l1_eval_control,
                        "regression": options.l1_eval_regression,
                    },
                    "evaluation": evaluation,
                    "timings": {
                        "tree": tree_seconds, "tables": tables_seconds, "model": model_seconds,
                        "load": load_seconds, "boards": boards_seconds, "evaluation": e.seconds,
                        "total": total.elapsed().as_secs_f64(),
                    },
                }),
            )?;
        }
        return Ok(());
    }
    drop(storage);
    let state = if let Some(path) = run_state.file.as_ref().filter(|p| p.exists()) {
        let state = l1::read_state(
            tree,
            model,
            buckets,
            options,
            &mut std::fs::File::open(path)?,
        )?;
        println!("Resuming from iteration {}", state.iteration());
        Some(state)
    } else {
        None
    };
    if let Some(dir) = &run_state.snapshots {
        std::fs::create_dir_all(dir)?;
    }
    let mut last_save = Instant::now();
    let mut previous = l1::Timings::default();
    let mut iteration_start = Instant::now();
    let observer = |p: &l1::Progress<'_>| -> Result<()> {
        let now = Instant::now();
        if print_every > 0 && p.iteration > 0 && p.iteration.is_multiple_of(print_every) {
            let (t, q) = (p.timings, &previous);
            println!(
                "Iteration {}: wall {:.6}s; reaches {:.6}; t2 {:.6}; t3 {:.6}; k4 {:.6}; update {:.6}; \
                 board preparation {:.6}; postflop {:.6}; evaluation {:.6}",
                p.iteration,
                now.duration_since(iteration_start).as_secs_f64(),
                t.trunk.reaches - q.trunk.reaches,
                t.trunk.t2 - q.trunk.t2,
                t.trunk.t3 - q.trunk.t3,
                t.trunk.k4 - q.trunk.k4,
                t.trunk.update - q.trunk.update,
                t.board_preparation - q.board_preparation,
                t.postflop - q.postflop,
                t.trunk.evaluation - q.trunk.evaluation
            );
        }
        if let Some(c) = p.checkpoint {
            let e = &c.evaluation;
            let gains =
                |f: fn(&l1::SeatEvaluation) -> f64| e.seats.iter().map(f).collect::<Vec<_>>();
            println!(
                "Checkpoint {}: NashConv {:.12}; held {:.12}; auxiliary {:.12}; auxiliary held {:.12}; \
                 gains {:?}; held gains {:?}; auxiliary gains {:?}; auxiliary held gains {:?}; \
                 evaluation {:.6}s",
                c.iteration,
                e.nash_conv,
                e.held_nash_conv,
                e.auxiliary_nash_conv,
                e.auxiliary_held_nash_conv,
                gains(|s| s.gain),
                gains(|s| s.held_gain),
                gains(|s| s.auxiliary_gain),
                gains(|s| s.auxiliary_held_gain),
                e.seconds
            );
        }
        if let Some(dir) = &run_state.snapshots
            && p.checkpoint.is_some()
        {
            let base = format!("it{:06}", p.iteration);
            write_json(
                &dir.join(format!("{base}.profile.json")),
                &p.average.unwrap().export(tree),
            )?;
            p.postflop.write_average(
                tree,
                &mut std::fs::File::create(dir.join(format!("{base}.postflop.bin")))?,
            )?;
        }
        if let Some(path) = &run_state.file
            && (p.finished
                || last_save.elapsed().as_secs_f64() >= run_state.every.unwrap_or(1800.0))
        {
            write_state_atomic(path, p)?;
            last_save = Instant::now();
        }
        previous = p.timings.clone();
        iteration_start = now;
        Ok(())
    };
    let solution =
        pool.install(|| l1::solve_with_state(tree, model, buckets, options, state, observer))?;
    println!(
        "Iterations: {}; reached target: {}; stopped by stop file: {}; solve phases {:?}",
        solution.iterations,
        solution.reached_target,
        solution.stopped_by_stop_file,
        solution.timings
    );
    for warning in &model.warnings {
        println!("Warning: {warning}");
    }
    if let Some(path) = output_profile {
        write_json(path, &solution.average.export(tree))?;
    }
    if let Some(path) = output_postflop {
        solution
            .postflop
            .write_average(tree, &mut std::fs::File::create(path)?)?;
    }
    if let Some(path) = output {
        write_json(
            path,
            &json!({
                "format": "p2-trunk-solve",
                "version": 2,
                "leaf_model": "l1",
                "k4": {
                    "samples": model.evaluation_options().k4_samples,
                    "seed": model.evaluation_options().seed,
                },
                "source": source,
                "game_fingerprint": blake3::Hash::from(tree.game_fingerprint).to_hex().as_str(),
                "options": options,
                "train_override": train_override_metadata,
                "ehs2": ehs2_metadata,
                "tables": {
                    "t2": {"payload_blake3": t2.payload_hash().to_hex().as_str()},
                    "t3": {"payload_blake3": t3.payload_hash().to_hex().as_str()},
                },
                "iterations": solution.iterations,
                "reached_target": solution.reached_target,
                "stopped_by_stop_file": solution.stopped_by_stop_file,
                "l1": {
                    "leaves": solution.postflop.leaves.len(),
                    "postflop_nodes": solution.postflop.nodes,
                    "slots": solution.postflop.slots,
                    "storage_bytes": solution.postflop.storage_bytes(),
                },
                "terminals": {"counts_by_k": tree.terminal_counts()},
                "checkpoints": solution.checkpoints,
                "warnings": model.warnings,
                "timings": {
                    "tree": tree_seconds,
                    "tables": tables_seconds,
                    "model": model_seconds,
                    "solve": solution.timings,
                    "total": solution.seconds + tree_seconds + tables_seconds + model_seconds,
                },
            }),
        )?;
    }
    Ok(())
}
