//! Fixed-iteration F32 HU postflop layout/thread comparison.
//!
//! Required: --config FILE --threads N --iterations N --layout dense|compact
//! --out NEW_DIRECTORY. The explicit iteration/thread arguments override the
//! config's run controls. By default there is no early stopping, checkpointing
//! or periodic evaluation. Optional --target-nash-conv with --check-every
//! enables a bounded quality-stop experiment: --iterations is the cap, and
//! run_seconds includes solve plus every exact EV/BR quality check. This mode
//! records all checks and whether the target was reached; an unmet cap still
//! emits artifacts for diagnosis. Configured game, economics, discount
//! schedule and chance-parallel thresholds are retained. This adds no CLI flag.
//!
//! Canonical binary v1 (all integers and raw float bits are little-endian):
//! header = 8-byte magic, u64 iterations, u32 node count, u32 action-node count,
//! f64 normalizer, then P0/P1 each: u16 root-support count and repeated
//! (u16 global combo id, f32 root weight). Topology follows for all node ids:
//! (u8 kind: action=0/chance=1/terminal=2, u8 player, u16 child count,
//! u32 first child). A node's layout-dependent aux/storage offsets are omitted.
//! Next come u32 deal count and repeated (u8 representative-card index, f32
//! deal weight), in compiled deal order; layout-dependent map ids are omitted.
//! `canonical.bin` magic HUCAN001 then stores each action node in node-id order:
//! (u32 node id, u8 player, u16 action count), followed by average-strategy f32
//! values in action-major, ascending retained-global-combo order. Finally P0
//! then P1 CFV sections contain u8 player and, for each action node, u32 node id,
//! u8 presence, and present vectors of f32 CFVs in that player's root-support
//! order. `state.bin` magic HUSTA001 has the same header/topology and node
//! records, followed per node by supported regret columns then strategy sums.
//! Values are neither quantized nor normalized/rebased during serialization.
//! Thus layout, threads, machine paths and timings do not enter either file.
//!
//! CFV capture uses the existing all-node evaluator, one player at a time.
//! Those allocations and file writes occur after run/EV/BR timing. External
//! process RSS includes them; it must not be described as solve-only memory.
//! Source, executable, input identities and resource limits belong to the
//! external runner. Artifact BLAKE3 hashes below identify only emitted bytes.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use cards::{PerPlayer, Player};
use clap::{Parser, ValueEnum};
use cli::config::{AlgorithmSection, GameSection, SolveConfig, StorageKind};
use cli::{economics, postflop_setup};
use engine::{F32Storage, Node, NodeKind, Solver, StorageStateRef};
use game::PayoffPipeline;
use holdem::{
    PostflopConfig, PostflopEvaluator, PostflopHands, build_postflop_game,
    build_postflop_game_dense,
};
use serde::Serialize;
use serde_json::json;

#[derive(Clone, Copy, Debug, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
enum Layout {
    Dense,
    Compact,
}

#[derive(Parser)]
#[command(
    name = "hu_scaling_bench",
    about = "Fixed-iteration F32 HU layout/thread research measurement"
)]
struct Args {
    #[arg(long)]
    config: PathBuf,
    #[arg(long)]
    threads: usize,
    #[arg(long)]
    iterations: u64,
    #[arg(long, value_enum)]
    layout: Layout,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, requires = "check_every")]
    target_nash_conv: Option<f64>,
    #[arg(long, requires = "target_nash_conv")]
    check_every: Option<u64>,
}

fn phase<T>(name: &str, clock: &Instant, work: impl FnOnce() -> Result<T>) -> Result<(T, f64)> {
    let event = |status: &str| {
        eprintln!(
            "{}",
            json!({"event": "phase", "phase": name, "status": status,
            "process_elapsed_seconds": clock.elapsed().as_secs_f64(),
            "unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis())})
        );
    };
    event("started");
    let started = Instant::now();
    let result = work();
    let elapsed = started.elapsed().as_secs_f64();
    event(if result.is_ok() {
        "completed"
    } else {
        "failed"
    });
    Ok((result?, elapsed))
}

fn create_file(path: &Path) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("creating new output {}", path.display()))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = create_file(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[derive(Serialize)]
struct Artifact {
    file: String,
    bytes: u64,
    blake3: String,
}

struct RawFile {
    output: BufWriter<File>,
    hash: blake3::Hasher,
    bytes: u64,
    name: String,
}

impl RawFile {
    fn new(directory: &Path, name: &str) -> Result<Self> {
        Ok(Self {
            output: BufWriter::new(create_file(&directory.join(name))?),
            hash: blake3::Hasher::new(),
            bytes: 0,
            name: name.to_owned(),
        })
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        self.output.write_all(data)?;
        self.hash.update(data);
        self.bytes = self
            .bytes
            .checked_add(data.len() as u64)
            .context("artifact size overflow")?;
        Ok(())
    }

    fn u32(&mut self, value: usize) -> Result<()> {
        self.write(
            &u32::try_from(value)
                .context("canonical u32 overflow")?
                .to_le_bytes(),
        )
    }

    fn f32(&mut self, value: f32) -> Result<()> {
        ensure!(value.is_finite(), "nonfinite canonical f32");
        self.write(&value.to_bits().to_le_bytes())
    }

    fn finish(mut self) -> Result<Artifact> {
        self.output.flush()?;
        self.output.get_ref().sync_all()?;
        Ok(Artifact {
            file: self.name,
            bytes: self.bytes,
            blake3: self.hash.finalize().to_hex().to_string(),
        })
    }
}

struct Support {
    hands: PostflopHands,
    indices: PerPlayer<Vec<usize>>,
}

impl Support {
    fn new(config: &PostflopConfig, actual: &PostflopHands) -> Result<Self> {
        let hands = PostflopHands::from_ranges(&config.board, &config.ranges);
        let indices = |player| -> Result<Vec<usize>> {
            hands
                .combos(player)
                .iter()
                .map(|&combo| {
                    actual
                        .index(player, combo as usize)
                        .context("root support missing from measured layout")
                })
                .collect()
        };
        let indices = PerPlayer::new(indices(Player::P0)?, indices(Player::P1)?);
        Ok(Self { hands, indices })
    }

    fn columns(
        &self,
        output: &mut RawFile,
        player: Player,
        values: &[f32],
        actions: usize,
        stride: usize,
    ) -> Result<()> {
        ensure!(
            values.len()
                == actions
                    .checked_mul(stride)
                    .context("column shape overflow")?,
            "canonical column dimensions differ"
        );
        for action in 0..actions {
            for &local in &self.indices[player] {
                ensure!(local < stride, "canonical hand index outside column");
                output.f32(values[action * stride + local])?;
            }
        }
        Ok(())
    }
}

type HuSolver = Solver<PostflopEvaluator, F32Storage>;

fn header(
    output: &mut RawFile,
    magic: &[u8; 8],
    solver: &HuSolver,
    support: &Support,
) -> Result<()> {
    let game = solver.game();
    output.write(magic)?;
    output.write(&solver.iteration().to_le_bytes())?;
    output.u32(game.tree.nodes.len())?;
    output.u32(game.tree.storage_refs.len())?;
    ensure!(
        game.normalizer.is_finite() && game.normalizer > 0.0,
        "invalid game normalizer"
    );
    output.write(&game.normalizer.to_bits().to_le_bytes())?;
    for player in Player::BOTH {
        output.write(&(support.hands.len(player) as u16).to_le_bytes())?;
        for (&combo, &local) in support
            .hands
            .combos(player)
            .iter()
            .zip(&support.indices[player])
        {
            output.write(&combo.to_le_bytes())?;
            output.f32(game.root_ranges[player][local])?;
        }
    }
    for node in &game.tree.nodes {
        let kind = match node.kind {
            NodeKind::Action => 0,
            NodeKind::Chance => 1,
            NodeKind::Terminal => 2,
        };
        output.write(&[kind, seat(node.player)])?;
        output.write(&node.num_children.to_le_bytes())?;
        output.write(&node.first_child.to_le_bytes())?;
    }
    output.u32(game.tree.deals.len())?;
    for (index, deal) in game.tree.deals.iter().enumerate() {
        output.write(&[game.evaluator.deal_card(index).index() as u8])?;
        output.f32(deal.weight)?;
    }
    Ok(())
}

fn seat(player: Player) -> u8 {
    match player {
        Player::P0 => 0,
        Player::P1 => 1,
    }
}

fn node_header(output: &mut RawFile, id: usize, node: &Node) -> Result<()> {
    output.u32(id)?;
    output.write(&[seat(node.player)])?;
    output.write(&node.num_children.to_le_bytes())
}

fn write_strategies(output: &mut RawFile, solver: &HuSolver, support: &Support) -> Result<()> {
    header(output, b"HUCAN001", solver, support)?;
    for (id, node) in solver.game().tree.nodes.iter().enumerate() {
        if node.kind == NodeKind::Action {
            let sref = solver.game().tree.storage_ref(node);
            node_header(output, id, node)?;
            support.columns(
                output,
                node.player,
                &solver.average_strategy_at(id as u32),
                sref.num_actions as usize,
                sref.num_hands as usize,
            )?;
        }
    }
    output.output.flush()?;
    Ok(())
}

fn write_values(
    output: &mut RawFile,
    solver: &HuSolver,
    support: &Support,
    player: Player,
    values: Vec<Option<Vec<f32>>>,
) -> Result<()> {
    ensure!(
        values.len() == solver.game().tree.storage_refs.len(),
        "CFV node count differs"
    );
    output.write(&[seat(player)])?;
    let mut values = values;
    for (id, node) in solver.game().tree.nodes.iter().enumerate() {
        if node.kind != NodeKind::Action {
            continue;
        }
        let sref = solver.game().tree.storage_ref(node);
        output.u32(id)?;
        match values[sref.index as usize].take() {
            Some(values) => {
                output.write(&[1])?;
                support.columns(
                    output,
                    player,
                    &values,
                    1,
                    solver.game().tree.root_dims[player] as usize,
                )?;
            }
            None => bail!("missing all-node CFV for node {id}, player {player:?}"),
        }
    }
    output.output.flush()?;
    Ok(())
}

fn write_state(directory: &Path, solver: &HuSolver, support: &Support) -> Result<Artifact> {
    let mut output = RawFile::new(directory, "state.bin")?;
    header(&mut output, b"HUSTA001", solver, support)?;
    let state = solver.state_ref();
    let StorageStateRef::F32 {
        regrets,
        strategy_sum,
    } = state.storage
    else {
        bail!("expected F32 state");
    };
    for (id, node) in solver.game().tree.nodes.iter().enumerate() {
        if node.kind != NodeKind::Action {
            continue;
        }
        let sref = solver.game().tree.storage_ref(node);
        node_header(&mut output, id, node)?;
        for values in [regrets, strategy_sum] {
            support.columns(
                &mut output,
                node.player,
                &values[sref.offset..sref.offset + sref.len()],
                sref.num_actions as usize,
                sref.num_hands as usize,
            )?;
        }
    }
    output.finish()
}

fn postflop(config: &SolveConfig) -> Result<PostflopConfig> {
    let GameSection::Postflop {
        board,
        oop_range,
        ip_range,
        pot,
        effective_stack,
        min_bet,
        iso_merging,
        preflop_aggressor,
        tree,
    } = &config.game
    else {
        bail!("only solvers.postflop/v1 is supported");
    };
    postflop_setup::build_postflop_config(
        board,
        oop_range,
        ip_range,
        *pot,
        *effective_stack,
        *iso_merging,
        *min_bet,
        tree.lower()?,
        preflop_aggressor,
    )
}

fn measure(args: &Args, config: &SolveConfig, clock: &Instant) -> Result<serde_json::Value> {
    ensure!(
        rayon::current_num_threads() == args.threads,
        "unexpected worker pool size"
    );
    let ((pf, rake, utility), prepare_seconds) = phase("prepare", clock, || {
        Ok((
            postflop(config)?,
            economics::build_rake(&config.rake)?,
            economics::build_utility(&config.utility)?,
        ))
    })?;
    let offset = postflop_setup::subgame_ev_offset(&pf, utility.as_ref());
    let (game, build_seconds) = phase("build", clock, || {
        let pipeline = PayoffPipeline {
            rake: rake.as_ref(),
            utility: utility.as_ref(),
        };
        Ok(match args.layout {
            Layout::Dense => build_postflop_game_dense(&pf, pipeline),
            Layout::Compact => build_postflop_game(&pf, pipeline),
        })
    })?;
    let support = Support::new(&pf, game.game.evaluator.hands())?;
    let mut support_union = support.hands.combos(Player::P0).to_vec();
    support_union.extend_from_slice(support.hands.combos(Player::P1));
    support_union.sort_unstable();
    support_union.dedup();
    let counts = json!({"nodes": game.game.tree.nodes.len(), "action_nodes": game.game.tree.storage_refs.len(),
        "deals": game.game.tree.deals.len(), "root_dims": game.game.tree.root_dims.as_ref().0,
        "retained_support_counts": [support.hands.len(Player::P0), support.hands.len(Player::P1)],
        "storage_elements_per_buffer": game.game.tree.storage_len,
        "root_subtree_has_chance": game.game.tree.subtree_has_chance[0],
        "f32_storage_payload_bytes": game.game.tree.storage_len as u64 * 8,
        "normalizer": game.game.normalizer});
    let (mut solver, solver_init_seconds) = phase("solver_init", clock, || {
        let mut solver = Solver::<_, F32Storage>::new(
            game.game,
            postflop_setup::build_schedule(&config.algorithm),
            Some(args.iterations),
        );
        postflop_setup::configure_solver(&mut solver, &config.run);
        Ok(solver)
    })?;
    let (stopping, run_seconds) = phase("run", clock, || {
        let Some(target) = args.target_nash_conv else {
            solver.run(args.iterations);
            return Ok(None);
        };
        let cadence = args.check_every.context("missing quality-check cadence")?;
        let mut checks = Vec::new();
        loop {
            let chunk = cadence.min(args.iterations - solver.iteration());
            let solve_started = Instant::now();
            solver.run(chunk);
            let solve_seconds = solve_started.elapsed().as_secs_f64();
            let check_started = Instant::now();
            let ev = Player::BOTH.map(|p| solver.expected_value(p));
            let br = Player::BOTH.map(|p| solver.best_response_value(p));
            let nash_conv = (br[0] - ev[0]) + (br[1] - ev[1]);
            let quality_seconds = check_started.elapsed().as_secs_f64();
            ensure!(
                ev.iter().chain(&br).all(|v| v.is_finite()) && nash_conv.is_finite(),
                "nonfinite stopping quality"
            );
            checks.push(json!({"iterations": solver.iteration(), "solver_ev": ev,
                "solver_br": br, "nash_conv": nash_conv,
                "solve_seconds": solve_seconds, "quality_seconds": quality_seconds}));
            let target_met = nash_conv <= target;
            if target_met || solver.iteration() == args.iterations {
                return Ok(Some(
                    json!({"criterion": "nash-conv-sum-of-unclamped-deviation-gains",
                    "target_nash_conv": target, "check_every": cadence,
                    "max_iterations": args.iterations, "target_met": target_met,
                    "reason": if target_met { "target-met" } else { "iteration-cap" },
                    "checks": checks}),
                ));
            }
        }
    })?;
    ensure!(
        if stopping.is_some() {
            solver.iteration() > 0 && solver.iteration() <= args.iterations
        } else {
            solver.iteration() == args.iterations
        },
        "iteration count differs"
    );
    let mut ev = [0.0; 2];
    let mut br = [0.0; 2];
    let mut ev_seconds = [0.0; 2];
    let mut br_seconds = [0.0; 2];
    for player in Player::BOTH {
        let index = seat(player) as usize;
        (ev[index], ev_seconds[index]) = phase(&format!("ev_p{index}"), clock, || {
            Ok(solver.expected_value(player))
        })?;
        (br[index], br_seconds[index]) = phase(&format!("br_p{index}"), clock, || {
            Ok(solver.best_response_value(player))
        })?;
    }
    ensure!(
        ev.iter().chain(&br).all(|value| value.is_finite()),
        "nonfinite root quality value"
    );
    let gain = [br[0] - ev[0], br[1] - ev[1]];
    let nash_conv = gain[0] + gain[1];
    let subgame_ev = [ev[0] + offset[Player::P0], ev[1] + offset[Player::P1]];
    let subgame_br = [br[0] + offset[Player::P0], br[1] + offset[Player::P1]];
    let mut canonical = RawFile::new(&args.out, "canonical.bin")?;
    let (_, strategy_write_seconds) = phase("strategy_write", clock, || {
        write_strategies(&mut canonical, &solver, &support)
    })?;
    let mut cfv_capture_seconds = [0.0; 2];
    let mut cfv_write_seconds = [0.0; 2];
    for player in Player::BOTH {
        let index = seat(player) as usize;
        let (values, elapsed) = phase(&format!("cfv_capture_p{index}"), clock, || {
            Ok(solver.expected_values_everywhere(player))
        })?;
        cfv_capture_seconds[index] = elapsed;
        (_, cfv_write_seconds[index]) = phase(&format!("cfv_write_p{index}"), clock, || {
            write_values(&mut canonical, &solver, &support, player, values)
        })?;
    }
    let (canonical, canonical_finish_seconds) =
        phase("canonical_finish", clock, || canonical.finish())?;
    let (state, state_write_seconds) = phase("state_write", clock, || {
        write_state(&args.out, &solver, &support)
    })?;
    let mut report = json!({
        "schema": "r1.hu-scaling-bench/v1", "status": "completed", "layout": args.layout,
        "config": args.config, "threads": args.threads, "iterations": solver.iteration(), "storage": "f32",
        "configured_run": config.run, "algorithm": config.algorithm, "rake": config.rake, "utility": config.utility,
        "execution_policy": "Fixed explicit iterations and threads; configured early stops/checkpoints/check cadence are not used. Configured chance thresholds are retained.",
        "counts": counts,
        "quality": {"solver_ev": ev, "solver_br": br, "subgame_ev": subgame_ev, "subgame_br": subgame_br,
            "deviation_gains": gain, "nash_conv": nash_conv, "exploitability_nash_conv_over_two": nash_conv / 2.0,
            "solver_ev_f64_bits_hex": ev.map(|v| format!("{:016x}", v.to_bits())),
            "solver_br_f64_bits_hex": br.map(|v| format!("{:016x}", v.to_bits())),
            "nash_conv_f64_bits_hex": format!("{:016x}", nash_conv.to_bits())},
        "timing": {"prepare_seconds": prepare_seconds, "build_seconds": build_seconds,
            "solver_init_seconds": solver_init_seconds, "run_seconds": run_seconds,
            "ev_seconds": ev_seconds, "br_seconds": br_seconds, "strategy_write_seconds": strategy_write_seconds,
            "cfv_capture_seconds": cfv_capture_seconds, "cfv_write_seconds": cfv_write_seconds,
            "canonical_finish_seconds": canonical_finish_seconds, "state_write_seconds": state_write_seconds},
        "canonical": {"format": "hu-scaling-rawbits/v1", "support_order": "P0 then P1; ascending global combo id among positive, starting-board-compatible root weights",
            "global_combos": [support.hands.combos(Player::P0), support.hands.combos(Player::P1)],
            "union_global_combos": support_union,
            "strategy_and_cfv": canonical, "supported_state": state},
        "resources": {"measurement": "external supervisor", "rss_scope": "Full process, including build, solve, EV/BR, and subsequent all-node CFV capture/output. Not solve-only RSS.",
            "cfv_capture": "One player's all-node vectors are captured at a time in the measured layout, then streamed and released before the other player. Capture is outside run and EV/BR timers."}
    });
    if let Some(stopping) = stopping {
        report["execution_policy"] = json!(
            "Explicit maximum iterations, threads, NashConv target and check cadence. The run timer includes all solve chunks and EV/BR checks through the first passing check or the cap. Final report queries and artifact capture remain outside this timer. No checkpoints or configured early stops."
        );
        report["timing"]["time_to_target_seconds"] = json!(run_seconds);
        report["stopping"] = stopping;
    }
    Ok(report)
}

fn main() -> Result<()> {
    let clock = Instant::now();
    let mut args = Args::parse();
    ensure!(args.threads > 0, "--threads must be positive");
    ensure!(args.iterations > 0, "--iterations must be positive");
    if let Some(target) = args.target_nash_conv {
        ensure!(
            target.is_finite() && target >= 0.0,
            "--target-nash-conv must be finite and nonnegative"
        );
        ensure!(
            args.check_every.is_some_and(|value| value > 0),
            "--check-every must be positive in target mode"
        );
    }
    args.config = fs::canonicalize(&args.config).context("locating config")?;
    let raw = fs::read_to_string(&args.config).context("reading UTF-8 config")?;
    let normalized = cli::solver_config_v1::normalized_toml_at(&raw, &args.config)?;
    let config = cli::config::parse_solve_config(&normalized)?;
    ensure!(
        config.schema.as_deref() == Some("solvers.postflop/v1"),
        "only solvers.postflop/v1 is supported"
    );
    ensure!(
        config.run.storage == StorageKind::F32,
        "only storage=f32 is supported"
    );
    ensure!(
        !matches!(
            &config.algorithm,
            AlgorithmSection::ExternalSamplingMccfr { .. }
        ),
        "sampled algorithms are unsupported"
    );
    fs::create_dir(&args.out)
        .with_context(|| format!("creating new output directory {}", args.out.display()))?;
    args.out = fs::canonicalize(&args.out)?;
    write_file(&args.out.join("config.original.toml"), raw.as_bytes())?;
    write_file(
        &args.out.join("config.normalized.toml"),
        normalized.as_bytes(),
    )?;
    let (pool, pool_build_seconds) = phase("pool_build", &clock, || {
        Ok(rayon::ThreadPoolBuilder::new()
            .num_threads(args.threads)
            .build()?)
    })?;
    let mut report = pool.install(|| measure(&args, &config, &clock))?;
    report["timing"]["pool_build_seconds"] = json!(pool_build_seconds);
    report["timing"]["process_elapsed_seconds"] = json!(clock.elapsed().as_secs_f64());
    let mut bytes = serde_json::to_vec_pretty(&report)?;
    bytes.push(b'\n');
    write_file(&args.out.join("result.json"), &bytes)?;
    std::io::stdout().write_all(&bytes)?;
    Ok(())
}
