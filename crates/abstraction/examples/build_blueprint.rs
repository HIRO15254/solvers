//! Prebuilds the full-street EHS2 abstraction and blueprint artifacts into
//! disk caches, so a first `solvers solve` on a bucketed config doesn't pay
//! the ~10-minute cold build.
//!
//! Usage: `cargo run --release -p abstraction --example build_blueprint -- \
//!     <flop_buckets> <turn_buckets> <river_buckets> <abs_cache> <artifacts_cache>`

use std::path::PathBuf;
use std::time::Instant;

use abstraction::{BlueprintArtifacts, Ehs2Abstraction, Ehs2Params};
use cards::Street;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        eprintln!(
            "usage: build_blueprint <flop_buckets> <turn_buckets> <river_buckets> \
             <abs_cache> <artifacts_cache>"
        );
        std::process::exit(2);
    }
    let params = Ehs2Params {
        flop_buckets: args[0].parse().expect("flop_buckets"),
        turn_buckets: args[1].parse().expect("turn_buckets"),
        river_buckets: args[2].parse().expect("river_buckets"),
    };
    let abs_cache = PathBuf::from(&args[3]);
    let art_cache = PathBuf::from(&args[4]);
    for path in [&abs_cache, &art_cache] {
        if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).expect("create cache dir");
        }
    }

    let t = Instant::now();
    let abs = Ehs2Abstraction::load_or_build(
        params,
        &[Street::Flop, Street::Turn, Street::River],
        Some(&abs_cache),
    );
    println!("abstraction ready in {:.1}s", t.elapsed().as_secs_f64());

    let t = Instant::now();
    let _artifacts = BlueprintArtifacts::load_or_build(&abs, Some(&art_cache));
    println!("artifacts ready in {:.1}s", t.elapsed().as_secs_f64());
}
