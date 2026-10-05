//! M6 CLI acceptance. Production solves are explicit release acceptance because
//! a cold EHS² table build exceeds the ordinary workspace-test time budget.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn text(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(args)
        .output()
        .unwrap()
}
fn ok(args: &[&str]) -> Output {
    let output = cli(args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn json(args: &[&str]) -> serde_json::Value {
    serde_json::from_slice(&ok(args).stdout).unwrap()
}

#[test]
fn p2_validation_effective_resources_types_and_preflight_before_directory() {
    let started = Instant::now();
    let dir = tempfile::tempdir().unwrap();
    let config = root().join("examples/nlh/preflop_multiway_v1_production_smoke.toml");
    let effective = dir.path().join("effective.toml");
    let value = json(&[
        "validate",
        text(&config),
        "--format",
        "json",
        "--resources",
        "--show-effective",
        "--write-effective",
        text(&effective),
    ]);
    assert_eq!(value["product"], "MultiwayPreflop");
    assert_eq!(value["gameKind"], "mw-preflop");
    assert_eq!(value["table"]["firstActor"], "BTN");
    assert_eq!(value["ranges"][0]["combos"], 1326);
    assert_eq!(value["schema"], "solvers.nlh/v1");
    assert_eq!(value["resources"]["withinLimit"], true);
    assert!(value["resources"]["solverStateBytes"].as_u64().unwrap() > 0);
    assert_eq!(value["effectiveConfig"]["solver"]["stop"]["max_sweeps"], 1);
    let effective2 = dir.path().join("effective2.toml");
    let human = ok(&[
        "validate",
        text(&effective),
        "--show-effective",
        "--write-effective",
        text(&effective2),
    ]);
    assert!(String::from_utf8_lossy(&human.stdout).contains("P2 (Multiway Preflop)"));
    assert_eq!(
        std::fs::read(&effective).unwrap(),
        std::fs::read(effective2).unwrap()
    );
    let out = dir.path().join("refused");
    let failed = cli(&["solve", text(&config), "--out", text(&out), "--memory", "1"]);
    assert!(!failed.status.success());
    assert!(!out.exists());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("NLH003"));
    let input = dir.path().join("input.toml");
    for (extra, code, key) in [
        ("solver = 2", "NLH002", "solver"),
        ("output = 2", "NLH002", "output"),
        ("[solver]\nseed = '19'", "NLH002", "solver.seed"),
        (
            "[solver.stop]\ntarget = '0.05'",
            "NLH002",
            "solver.stop.target",
        ),
        (
            "[solver.abstraction.buckets]\nflop = '2'",
            "NLH002",
            "solver.abstraction.buckets.flop",
        ),
        (
            "[output]\nprobability_encoding = []",
            "NLH002",
            "output.probability_encoding",
        ),
        ("[solver]\nseed = -1", "NLH003", "solver.seed"),
        (
            "[solver]\nstorage = 'f32'",
            "NLH002",
            "this spot is solved by P2",
        ),
    ] {
        let (root_keys, sections) = if extra.starts_with('[') {
            ("", extra)
        } else {
            (extra, "")
        };
        std::fs::write(&input, format!("schema = 'solvers.nlh/v1'\n{root_keys}\n[table]\nplayers = 3\nstack_bb = 2\n{sections}\n")).unwrap();
        let error = cli(&["validate", text(&input)]);
        let stderr = String::from_utf8_lossy(&error.stderr);
        assert!(!error.status.success());
        assert!(stderr.contains(code) && stderr.contains(key), "{stderr}");
    }
    std::fs::write(&input, "schema = 'solvers.nlh/v1'\n[table]\nplayers = 3\nstack_bb = 2\n[tree]\nscript = 'preflop when players == 9 { add raise [a] }'\n").unwrap();
    assert_eq!(
        json(&["validate", text(&input), "--format", "json"])["warnings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    eprintln!(
        "M6 validate wall time: {:.3}s",
        started.elapsed().as_secs_f64()
    );
}

fn equivalent(old: &Path, new: &Path, old_run: &Path, new_run: &Path) {
    let started = Instant::now();
    // Let CI/review choose a prepared operational cache, exactly as production does.
    ok(&["solve", text(old), "--out", text(old_run), "--threads", "1"]);
    ok(&["solve", text(new), "--out", text(new_run), "--threads", "1"]);
    let old_sol = old_run.join("solution.mwsol");
    let watched = ok(&[
        "watch",
        text(new_run),
        "--format",
        "json",
        "--poll-secs",
        "0.05",
    ]);
    assert!(String::from_utf8_lossy(&watched.stdout).contains("completed"));
    let new_sol = new_run.join("solution.mwsol");
    for view in ["summary", "strategy", "range", "ev", "tree", "actions"] {
        let a = ok(&["export", text(&old_sol), view]);
        let b = ok(&["export", text(&new_sol), view]);
        assert_eq!(a.stdout, b.stdout, "export {view}: {}", new.display());
    }
    let a = ok(&[
        "evaluate",
        text(&old_sol),
        "--samples",
        "8",
        "--seed",
        "7",
        "--br-traversals",
        "1",
    ]);
    let b = ok(&[
        "evaluate",
        text(&new_sol),
        "--samples",
        "8",
        "--seed",
        "7",
        "--br-traversals",
        "1",
    ]);
    assert_eq!(a.stdout, b.stdout, "held-out evaluation");
    let a = mw_preflop::mwsol::MwSolReader::open(&old_sol).unwrap();
    let b = mw_preflop::mwsol::MwSolReader::open(&new_sol).unwrap();
    let left = serde_json::to_value(a.metadata()).unwrap();
    let right = serde_json::to_value(b.metadata()).unwrap();
    let differences: Vec<_> = left
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, v)| right[*k] != **v)
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(
        differences,
        [
            "config_fingerprint",
            "config_toml",
            "configuration_fingerprint",
            "game_fingerprint"
        ]
    );
    let run_a: serde_json::Value =
        serde_json::from_slice(&std::fs::read(old_run.join("run.json")).unwrap()).unwrap();
    let run_b: serde_json::Value =
        serde_json::from_slice(&std::fs::read(new_run.join("run.json")).unwrap()).unwrap();
    let run_differences: Vec<_> = run_b
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| run_a[*key] != run_b[*key])
        .cloned()
        .collect();
    for key in &run_differences {
        assert!(
            [
                "configHash",
                "effectiveConfig",
                "gameFingerprint",
                "configurationFingerprint",
                "gameKind",
                "configSchema",
                "elapsedSecs",
                "traversalsPerSecond",
                "handUpdatesPerSecond",
                "startedUnixMs",
                "finishedUnixMs"
            ]
            .contains(&key.as_str()),
            "unexpected run.json difference: {key}"
        );
    }
    assert_eq!(run_b["gameKind"], "mw-preflop");
    assert_eq!(run_b["configSchema"], "solvers.nlh/v1");
    let effective = std::fs::read(new_run.join("run.toml")).unwrap();
    assert_eq!(
        run_b["configHash"],
        runfiles::config_hash_hex(&runfiles::config_hash(&effective))
    );
    assert_eq!(b.metadata().config_toml.as_bytes(), effective);
    eprintln!("run.json differences={run_differences:?}");
    eprintln!(
        "{}: metadata differences={differences:?}; export/evaluate differences=[]; wall={:.3}s",
        new.display(),
        started.elapsed().as_secs_f64()
    );
}

#[test]
#[ignore = "production EHS² cache build and repeated artifact evaluation; run explicitly with --release --ignored"]
fn fixed_seed_smoke_equivalence_and_resume_inspect_lifecycle() {
    let started = Instant::now();
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "preflop_multiway_v1_3max_smoke.toml",
        "preflop_multiway_v1_production_smoke.toml",
        "3max_2bb.toml",
        "6max_2bb.toml",
    ] {
        let old = root()
            .join(if name.starts_with("preflop") {
                "examples"
            } else {
                "examples/bench_multiway"
            })
            .join(name);
        let new = root().join("examples/nlh").join(name);
        equivalent(
            &old,
            &new,
            &dir.path().join(format!("old-{name}")),
            &dir.path().join(format!("new-{name}")),
        );
    }
    let new_run = dir.path().join("new-preflop_multiway_v1_3max_smoke.toml");
    let old_run = dir.path().join("old-preflop_multiway_v1_3max_smoke.toml");
    let manifest = json(&["status", text(&new_run), "--format", "json"]);
    assert!(manifest.to_string().contains("mw-preflop"));
    ok(&["runs", "ls", text(dir.path()), "--format", "json"]);
    let new_sol = new_run.join("solution.mwsol");
    let old_sol = old_run.join("solution.mwsol");
    ok(&["compare", text(&new_sol), text(&new_sol)]);
    let mixed = cli(&["compare", text(&new_sol), text(&old_sol), "--cross-game"]);
    assert!(!mixed.status.success());
    assert!(String::from_utf8_lossy(&mixed.stderr).contains("old-family"));
    // Exercise the typed non-root sampler with a discount sentinel that cannot be represented in lowered TOML.
    let ev = json(&[
        "inspect",
        text(&new_sol),
        "--node",
        "1",
        "--view",
        "ev",
        "--samples",
        "8",
        "--seed",
        "9",
    ]);
    assert_eq!(ev["history"].as_str().unwrap().len(), 32);
    assert!(ev["evaluation"].is_object(), "{ev}");
    let resumed = dir.path().join("resumed");
    let checkpoint = new_run.join("checkpoint.mwckpt");
    ok(&[
        "resume",
        text(&checkpoint),
        "--out",
        text(&resumed),
        "--max-sweeps",
        "3",
        "--stop-target",
        "1000000",
        "--evaluation-samples",
        "8",
        "--evaluation-cadence",
        "1",
        "--checkpoint-interval",
        "1s",
        "--threads",
        "2",
        "--memory",
        "64MiB",
        "--max-time",
        "1m",
    ]);
    let effective: toml::Value = std::fs::read_to_string(resumed.join("run.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(effective["schema"].as_str(), Some("solvers.nlh/v1"));
    assert_eq!(
        effective["solver"]["stop"]["max_sweeps"].as_integer(),
        Some(3)
    );
    assert_eq!(effective["run"]["threads"].as_integer(), Some(2));
    assert_eq!(effective["run"]["checkpoint_interval"].as_str(), Some("1s"));
    assert_eq!(effective["run"]["max_time"].as_str(), Some("1m"));
    let metadata = mw_preflop::mwsol::MwSolReader::open(&resumed.join("solution.mwsol")).unwrap();
    assert_eq!(
        metadata.metadata().config_toml,
        std::fs::read_to_string(resumed.join("run.toml")).unwrap()
    );
    let ckpt = mw_preflop::checkpoint::MultiwayCheckpoint::load_unchecked(
        &resumed.join("checkpoint.mwckpt"),
    )
    .unwrap();
    assert_eq!(
        ckpt.config_toml.as_deref(),
        Some(metadata.metadata().config_toml.as_str())
    );
    eprintln!(
        "M6 smoke equivalence/lifecycle wall time: {:.3}s",
        started.elapsed().as_secs_f64()
    );
}

#[test]
fn tracked_p2_examples_preserve_legacy_abstraction_and_economics() {
    let started = Instant::now();
    for name in [
        "preflop_multiway_v1_3max_smoke.toml",
        "preflop_multiway_v1_default.toml",
        "preflop_multiway_v1_full_surface.toml",
        "preflop_multiway_v1_production_smoke.toml",
        "3max_2bb.toml",
        "6max_2bb.toml",
        "6max_20bb_checkdown.toml",
        "6max_position_selector.toml",
        "6max_100bb_nl50_partial_reference.toml",
        "6max_100bb_nl50_partial_reference_limp.toml",
        "6max_100bb_nl50_partial_simple_reference.toml",
    ] {
        let old_path = root()
            .join(if name.starts_with("preflop") {
                "examples"
            } else {
                "examples/bench_multiway"
            })
            .join(name);
        let new_path = root().join("examples/nlh").join(name);
        let old = cli::config::parse_solve_config_at(
            &std::fs::read_to_string(&old_path).unwrap(),
            &old_path,
        )
        .unwrap();
        let new_raw = std::fs::read_to_string(&new_path).unwrap();
        let new = cli::config::parse_solve_config_at(&new_raw, &new_path).unwrap();
        let cli::config::GameSection::PreflopMultiway(a) = old.game else {
            unreachable!()
        };
        let cli::config::GameSection::PreflopMultiway(b) = new.game else {
            unreachable!()
        };
        assert_eq!(a.abstraction, b.abstraction, "{name}");
        assert_eq!(
            serde_json::to_value(old.utility).unwrap(),
            serde_json::to_value(new.utility).unwrap(),
            "{name}"
        );
        assert_eq!(
            serde_json::to_value(old.rake).unwrap(),
            serde_json::to_value(new.rake).unwrap(),
            "{name}"
        );
        assert_eq!(
            serde_json::to_value(old.algorithm).unwrap(),
            serde_json::to_value(new.algorithm).unwrap(),
            "{name}"
        );
        let doc = spot::Document::parse(&new_raw, &new_path).unwrap();
        let effective = doc.normalize(&mw_preflop::input::P2Sections).unwrap();
        assert_eq!(
            effective,
            spot::Document::parse(&effective, &new_path)
                .unwrap()
                .normalize(&mw_preflop::input::P2Sections)
                .unwrap()
        );
    }
    eprintln!(
        "M6 tracked examples wall time: {:.3}s",
        started.elapsed().as_secs_f64()
    );
}
