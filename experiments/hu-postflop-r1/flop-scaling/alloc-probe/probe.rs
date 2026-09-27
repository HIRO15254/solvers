//! Research-only fixed-fixture correctness probe, not a CLI config parser.
//! Usage: solve.exe narrow|expanded 1|2 1|2 NEW_OUTPUT_DIRECTORY
//! Arguments after the case are worker count and exact iteration count.
//! External supervision supplies wall-time, memory and disk bounds.

// Research-only phase-window allocation counters. No solver semantics live here.
mod allocation_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    // The event driver is single-threaded. An odd epoch admits operations; an
    // even epoch excludes them. Epoch equality prevents a delayed old entrant
    // from being admitted after a stop/reset/start cycle.
    static EPOCH: AtomicU64 = AtomicU64::new(0);
    static INFLIGHT: AtomicUsize = AtomicUsize::new(0);
    static COUNTERS: [AtomicU64; 12] = [const { AtomicU64::new(0) }; 12];

    pub(crate) struct CountingSystem;

    #[global_allocator]
    pub(crate) static ALLOCATOR: CountingSystem = CountingSystem;

    fn enter() -> bool {
        let epoch = EPOCH.load(Ordering::SeqCst);
        if epoch & 1 == 0 {
            return false;
        }
        INFLIGHT.fetch_add(1, Ordering::SeqCst);
        if EPOCH.load(Ordering::SeqCst) != epoch {
            INFLIGHT.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        true
    }

    fn add(index: usize, value: u64) {
        COUNTERS[index].fetch_add(value, Ordering::Relaxed);
    }

    fn leave() {
        INFLIGHT.fetch_sub(1, Ordering::SeqCst);
    }

    // Each admitted operation stays inflight through System and all updates.
    // Allocator methods never format, allocate recursively, lock, or panic.
    unsafe impl GlobalAlloc for CountingSystem {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let counted = enter();
            let result = unsafe { System.alloc(layout) };
            if counted {
                add(0, 1);
                add(1, layout.size() as u64);
                add(2, result.is_null() as u64);
                leave();
            }
            result
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let counted = enter();
            let result = unsafe { System.alloc_zeroed(layout) };
            if counted {
                add(3, 1);
                add(4, layout.size() as u64);
                add(5, result.is_null() as u64);
                leave();
            }
            result
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let counted = enter();
            let result = unsafe { System.realloc(ptr, layout, size) };
            if counted {
                add(6, 1);
                add(7, size as u64);
                add(8, layout.size() as u64);
                add(9, result.is_null() as u64);
                leave();
            }
            result
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            let counted = enter();
            unsafe { System.dealloc(ptr, layout) };
            if counted {
                add(10, 1);
                add(11, layout.size() as u64);
                leave();
            }
        }
    }

    pub(crate) fn start() {
        let epoch = EPOCH.load(Ordering::SeqCst);
        assert_eq!(epoch & 1, 0, "allocation phase already active");
        // stop() drained every admitted operation before returning. A delayed
        // rejected entrant can still modify INFLIGHT but cannot update counters.
        // Do not assert INFLIGHT == 0 here: that would reject such a valid race.
        for counter in &COUNTERS {
            counter.store(0, Ordering::Relaxed);
        }
        EPOCH.store(epoch.checked_add(1).unwrap(), Ordering::SeqCst);
    }

    pub(crate) fn stop() -> Option<[u64; 12]> {
        let epoch = EPOCH.load(Ordering::SeqCst);
        if epoch & 1 == 0 {
            return None;
        }
        EPOCH.store(epoch.checked_add(1).unwrap(), Ordering::SeqCst);
        while INFLIGHT.load(Ordering::SeqCst) != 0 {
            std::hint::spin_loop();
        }
        Some(std::array::from_fn(|i| COUNTERS[i].load(Ordering::Relaxed)))
    }

    pub(crate) fn emit(phase: &str, counts: [u64; 12]) {
        println!(
            "{{\"event\":\"allocation_counts\",\"schema\":\"r1-phase-allocation/v1\",\"phase\":\"{phase}\",\"alloc_calls\":{},\"alloc_requested_bytes\":{},\"alloc_failed_calls\":{},\"alloc_zeroed_calls\":{},\"alloc_zeroed_requested_bytes\":{},\"alloc_zeroed_failed_calls\":{},\"realloc_calls\":{},\"realloc_requested_new_bytes\":{},\"realloc_old_layout_bytes\":{},\"realloc_failed_calls\":{},\"dealloc_calls\":{},\"dealloc_layout_bytes\":{}}}",
            counts[0], counts[1], counts[2], counts[3], counts[4], counts[5],
            counts[6], counts[7], counts[8], counts[9], counts[10], counts[11]
        );
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use cards::script::{POSTFLOP, PostflopVar, Script};
use cards::{Card, Chips, PerPlayer, Player, Range, Street};
use engine::{Dcfr, F32Storage, NodeKind, ParConfig, Solver, StorageStateRef};
use game::{ChipEv, NoRake, PayoffPipeline};
use holdem::{PerStreet, PostflopConfig, PostflopEvaluator, StreetTree, build_postflop_game};

const SCRIPT: &str = "flop, turn, river {\n  replace bet [75]\n  replace raise [75]\n}\n";
const BUFFER_BYTES: usize = 16 * 1024;

fn event(phase: &str, status: &str) {
    if status == "completed" {
        if let Some(counts) = allocation_probe::stop() {
            allocation_probe::emit(phase, counts);
        }
    }
    println!("{{\"phase\":\"{phase}\",\"status\":\"{status}\"}}");
    std::io::stdout().flush().unwrap();
    if status == "started" {
        allocation_probe::start();
    }
}

fn new_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn write_json(path: &Path, value: &str) -> std::io::Result<()> {
    let mut file = new_file(path)?;
    file.write_all(value.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()
}

fn fixture(case: &str) -> (PostflopConfig, [usize; 2], usize, f64) {
    let (oop, ip, support, elements, normalizer) = match case {
        "narrow" => (
            "TT+,AQs+,KQs",
            "JJ-99,AQs-ATs,KQs,QJs",
            [34, 30],
            10_176_768,
            870.0,
        ),
        "expanded" => (
            "TT+,AQs+,AQo+,A5s-A4s,KQs",
            "JJ-22,AQs-A2s,KQs-KTs,QJs-QTs,JTs,T9s,98s,AQo-ATo,KQo",
            [63, 160],
            35_459_676,
            8700.0,
        ),
        _ => panic!("case must be narrow or expanded"),
    };
    let board: Vec<Card> = "Qs Jh 2h"
        .split_whitespace()
        .map(|c| c.parse().unwrap())
        .collect();
    assert_eq!(board.iter().copied().collect::<BTreeSet<_>>().len(), 3);
    let ranges = PerPlayer::new(oop.parse::<Range>().unwrap(), ip.parse::<Range>().unwrap());
    for p in Player::BOTH {
        assert!(
            ranges[p]
                .weights()
                .iter()
                .all(|&w| w.is_finite() && (w == 0.0 || w == 1.0))
        );
    }
    let script = Script::<PostflopVar>::compile(SCRIPT, &BTreeMap::new(), &POSTFLOP).unwrap();
    assert!(script.params.is_empty());
    assert_eq!(script.rules.len(), 6);
    let street = |s| StreetTree::from_script(s, &script.rules, 2, false, None);
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(200),
        effective_stack: Chips(900),
        streets: PerStreet {
            flop: street(Street::Flop),
            turn: street(Street::Turn),
            river: street(Street::River),
        },
        min_bet: Chips(10),
        iso_merging: false,
        track_node_info: true,
        preflop_aggressor: Some(Player::P0),
    };
    (config, support, elements, normalizer)
}

fn write_state(
    path: &Path,
    solver: &Solver<PostflopEvaluator, F32Storage>,
    planned: u64,
) -> std::io::Result<u64> {
    // Borrow, never call state(): no second full regret/strategy allocation.
    let state = solver.state_ref();
    let StorageStateRef::F32 {
        regrets,
        strategy_sum,
    } = state.storage
    else {
        unreachable!("the probe only instantiates F32Storage")
    };
    let game = solver.game();
    let hands = game.evaluator.hands();
    let mut writer = BufWriter::with_capacity(BUFFER_BYTES, new_file(path)?);
    writer.write_all(b"R1F32S01")?;
    // Eight little-endian u64 fields after the 8-byte magic.
    for value in [
        state.iteration,
        planned,
        game.tree.nodes.len() as u64,
        game.tree.storage_refs.len() as u64,
        hands.len(Player::P0) as u64,
        hands.len(Player::P1) as u64,
        regrets.len() as u64,
        strategy_sum.len() as u64,
    ] {
        writer.write_all(&value.to_le_bytes())?;
    }
    for p in Player::BOTH {
        for &combo in hands.combos(p) {
            writer.write_all(&combo.to_le_bytes())?;
        }
    }
    let mut buffer = [0_u8; BUFFER_BYTES];
    for values in [regrets, strategy_sum] {
        for chunk in values.chunks(BUFFER_BYTES / 4) {
            for (value, bytes) in chunk.iter().zip(buffer.chunks_exact_mut(4)) {
                assert!(value.is_finite(), "nonfinite F32 solver state");
                bytes.copy_from_slice(&value.to_bits().to_le_bytes());
            }
            writer.write_all(&buffer[..chunk.len() * 4])?;
        }
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    let bytes = writer.get_ref().metadata()?.len();
    assert_eq!(
        bytes,
        72 + 2 * (hands.len(Player::P0) + hands.len(Player::P1)) as u64
            + 4 * (regrets.len() + strategy_sum.len()) as u64
    );
    Ok(bytes)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert_eq!(
        args.len(),
        4,
        "usage: solve narrow|expanded 1|2 1|2 NEW_OUTPUT_DIRECTORY"
    );
    let case = args[0].as_str();
    let threads: usize = args[1].parse()?;
    let iterations: u64 = args[2].parse()?;
    assert!((1..=2).contains(&threads), "worker limit is 2");
    assert!((1..=2).contains(&iterations), "iteration limit is 2");
    let (config, support, elements, normalizer) = fixture(case);
    let out = Path::new(&args[3]);
    // Parent must exist; existing output directories are rejected, not reused.
    std::fs::create_dir(out)?;
    write_json(
        &out.join("invocation.json"),
        &format!(
            "{{\"schema\":\"r1.flop-native-solve/v1\",\"case\":\"{case}\",\"threads\":{threads},\"iterations\":{iterations},\"planned_iterations\":{iterations},\"chance_depth\":2,\"min_children\":12,\"storage\":\"f32\",\"schedule\":\"dcfr\",\"alpha\":1.5,\"beta\":0,\"gamma\":3,\"pow4_reset\":true,\"cli_toml_normalized\":false,\"quality_target\":null,\"cfv_capture\":false}}"
        ),
    )?;
    event("build", "started");
    let started = Instant::now();
    let built = build_postflop_game(
        &config,
        PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        },
    );
    let build_seconds = started.elapsed().as_secs_f64();
    let game = built.game;
    // Keep the exact game, but release viewer strings before solver allocation.
    drop(built.node_info);
    drop(built.rule_hits);
    assert_eq!(game.tree.nodes.len(), 367_662);
    assert_eq!(game.tree.storage_refs.len(), 147_104);
    assert_eq!(game.tree.storage_len, elements);
    assert_eq!(game.normalizer, normalizer);
    assert!(game.zero_sum);
    let mut kinds = [0_usize; 3];
    for node in &game.tree.nodes {
        kinds[match node.kind {
            NodeKind::Action => 0,
            NodeKind::Chance => 1,
            NodeKind::Terminal => 2,
        }] += 1;
    }
    assert_eq!(kinds, [147_104, 1034, 219_524]);
    for p in Player::BOTH {
        assert_eq!(game.tree.root_dims[p] as usize, support[p.index()]);
        assert_eq!(game.evaluator.hands().len(p), support[p.index()]);
        assert_eq!(game.root_ranges[p].len(), support[p.index()]);
        assert!(game.root_ranges[p].iter().all(|&w| w == 1.0));
    }
    event("build", "completed");
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()?;
    event("solver_allocation", "started");
    let mut solver = Solver::<_, F32Storage>::new(
        game,
        Box::new(Dcfr {
            alpha: 1.5,
            beta: 0.0,
            gamma: 3.0,
            pow4_reset: true,
        }),
        Some(iterations),
    );
    solver.set_par(ParConfig {
        chance_depth: 2,
        min_children: 12,
    });
    event("solver_allocation", "completed");
    event("cfr", "started");
    let started = Instant::now();
    pool.install(|| {
        assert_eq!(rayon::current_num_threads(), threads);
        solver.run(iterations);
    });
    let cfr_seconds = started.elapsed().as_secs_f64();
    assert_eq!(solver.iteration(), iterations);
    event("cfr", "completed");
    event("state_write", "started");
    let started = Instant::now();
    let state_bytes = write_state(&out.join("state.bin"), &solver, iterations)?;
    let state_write_seconds = started.elapsed().as_secs_f64();
    event("state_write", "completed");

    // Public API calls, including their precise zero-sum exploitability policy.
    // No second metric definition or per-node/root CFV capture is introduced.
    let started = Instant::now();
    let mut ev = [0.0_f64; 2];
    let mut br = [0.0_f64; 2];
    for p in Player::BOTH {
        let phase = if p == Player::P0 { "ev_p0" } else { "ev_p1" };
        event(phase, "started");
        ev[p.index()] = pool.install(|| solver.expected_value(p));
        assert!(ev[p.index()].is_finite());
        event(phase, "completed");
    }
    for p in Player::BOTH {
        let phase = if p == Player::P0 { "br_p0" } else { "br_p1" };
        event(phase, "started");
        br[p.index()] = pool.install(|| solver.best_response_value(p));
        assert!(br[p.index()].is_finite());
        event(phase, "completed");
    }
    event("exploitability", "started");
    let gains = pool.install(|| solver.exploitability()).0;
    assert!(gains.iter().all(|v| v.is_finite()));
    event("exploitability", "completed");
    let quality_seconds = started.elapsed().as_secs_f64();
    assert_eq!(solver.iteration(), iterations);
    let quality = format!(
        "{{\"schema\":\"r1.flop-native-quality/v1\",\"case\":\"{case}\",\"iterations\":{iterations},\"root_support\":{support:?},\"normalizer_bits\":\"{:016x}\",\"ev\":{ev:?},\"ev_bits\":[\"{:016x}\",\"{:016x}\"],\"br\":{br:?},\"br_bits\":[\"{:016x}\",\"{:016x}\"],\"exploitability\":{gains:?},\"exploitability_bits\":[\"{:016x}\",\"{:016x}\"],\"metric_source\":\"public expected_value and best_response_value for both seats, then public exploitability\",\"ev_basis\":\"solver internal chip utility; no starting-share reporting offset\",\"quality_target\":null,\"cfv_capture\":false}}",
        normalizer.to_bits(),
        ev[0].to_bits(),
        ev[1].to_bits(),
        br[0].to_bits(),
        br[1].to_bits(),
        gains[0].to_bits(),
        gains[1].to_bits(),
    );
    write_json(&out.join("quality.json"), &quality)?;
    write_json(
        &out.join("result.json"),
        &format!(
            "{{\"schema\":\"r1.flop-native-solve/v1\",\"status\":\"completed\",\"case\":\"{case}\",\"threads\":{threads},\"iterations\":{iterations},\"state_file\":\"state.bin\",\"state_bytes\":{state_bytes},\"quality_file\":\"quality.json\",\"build_seconds\":{build_seconds},\"cfr_seconds\":{cfr_seconds},\"state_write_seconds\":{state_write_seconds},\"quality_seconds\":{quality_seconds},\"cfv_capture\":false,\"performance_claim\":false}}"
        ),
    )?;
    event("probe", "completed");
    Ok(())
}
