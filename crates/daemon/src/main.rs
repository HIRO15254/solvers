//! `solversd`: a job daemon for the solvers CLI.
//!
//! It creates run directories, spawns `solvers` into them, and serves what
//! those directories say. It does not solve, and it keeps no state of its
//! own -- restart it and it picks up every run by reading the runs root
//! again (`docs/app-architecture.md` R1, R2).

mod api;
mod http;
mod jobs;
mod runs;
mod tls;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use rand::Rng;

use crate::api::Api;
use crate::http::Daemon;
use crate::jobs::JobRunner;
use crate::runs::RunsRoot;

#[derive(Parser)]
#[command(name = "solversd", version, about = "Job daemon for the solvers CLI")]
struct Cli {
    /// Directory holding run directories. This is the daemon's entire state.
    #[arg(long, default_value = "runs")]
    runs: PathBuf,
    /// Address to bind. Loopback by default; a non-loopback address
    /// requires TLS, since the bearer token would otherwise cross the
    /// network in clear.
    #[arg(long, default_value = "127.0.0.1:38127")]
    bind: String,
    /// PEM certificate chain to serve TLS with. Requires --tls-key.
    #[arg(long, requires = "tls_key")]
    tls_cert: Option<PathBuf>,
    /// PEM private key for --tls-cert.
    #[arg(long, requires = "tls_cert")]
    tls_key: Option<PathBuf>,
    /// The `solvers` binary to run. Defaults to one beside this executable,
    /// then to `solvers` on PATH.
    #[arg(long)]
    solver: Option<PathBuf>,
    /// Machine cache directory, passed through to every run.
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    /// How many runs may execute at once.
    ///
    /// One by default: a Multiway Preflop run commits a policy arena of
    /// several GiB before its first sweep, so the binding limit is memory,
    /// and only the operator knows the budget.
    #[arg(long, default_value_t = 1)]
    max_concurrent: usize,
    /// Bearer token clients must present. Falls back to `SOLVERSD_TOKEN`,
    /// then to a fresh random one, which is printed at startup.
    #[arg(long)]
    token: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let solver = cli.solver.unwrap_or_else(api::default_solver_path);
    let runs = RunsRoot::new(&cli.runs)
        .with_context(|| format!("opening the runs root {}", cli.runs.display()))?;
    let jobs = JobRunner::new(solver.clone(), cli.cache_dir.clone(), cli.max_concurrent);

    let tls = match (cli.tls_cert.as_deref(), cli.tls_key.as_deref()) {
        (Some(certificate), Some(key)) => Some(tls::Tls::load(certificate, key)?),
        _ => None,
    };
    tls::check_exposure(&cli.bind, tls.is_some())?;

    Daemon {
        api: Api { runs, jobs, solver },
        token: cli
            .token
            .or_else(|| std::env::var("SOLVERSD_TOKEN").ok())
            .unwrap_or_else(generate_token),
    }
    .serve(&cli.bind, tls)
}

/// A 256-bit token, hex-encoded.
fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_long_and_not_repeated() {
        let first = generate_token();
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, generate_token());
    }
}
