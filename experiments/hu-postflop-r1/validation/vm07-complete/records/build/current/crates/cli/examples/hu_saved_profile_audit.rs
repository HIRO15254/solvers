//! Research-only evaluation of a frozen Full HU postflop `.sol` policy.

use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "hu_saved_profile_audit")]
struct Args {
    /// Full NLH HU postflop `.sol` artifact. NoRivers is rejected.
    #[arg(long)]
    sol: PathBuf,

    /// Positive worker count for the dedicated load/evaluation Rayon pool.
    #[arg(long)]
    threads: usize,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let audit = cli::sol::audit_saved_full_profile(&args.sol, args.threads)?;
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &audit)?;
    writeln!(stdout)?;
    Ok(())
}
