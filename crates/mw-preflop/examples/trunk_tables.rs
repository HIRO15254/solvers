//! Experimental S4-1a table cache builder and independent-checker exports.
use anyhow::{Context, Result, bail};
use mw_preflop::trunk::{
    classes::Classes,
    tables::{DEFAULT_T3_SAMPLES, HuShowdownTable, ThreeWayTable},
};
use std::{
    io::{BufWriter, Write},
    path::PathBuf,
    time::Instant,
};

fn main() -> Result<()> {
    let mut dir = PathBuf::from(".cache/p2-trunk");
    let mut samples = DEFAULT_T3_SAMPLES;
    let mut seed = 0;
    let mut export_t2 = None;
    let mut export_classes = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!(
                "trunk_tables [--dir PATH] [--t3-samples N] [--seed U64] [--export-t2 CSV] [--export-classes CSV]"
            );
            return Ok(());
        }
        let value = args
            .next()
            .with_context(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--dir" => dir = value.into(),
            "--t3-samples" => samples = value.parse()?,
            "--seed" => seed = value.parse()?,
            "--export-t2" => export_t2 = Some(PathBuf::from(value)),
            "--export-classes" => export_classes = Some(PathBuf::from(value)),
            _ => bail!("unknown argument {arg}"),
        }
    }
    let start = Instant::now();
    let t2 = HuShowdownTable::load_or_build(&dir)?;
    println!(
        "T2 build/load: {:.3}s; payload BLAKE3 {}",
        start.elapsed().as_secs_f64(),
        t2.payload_hash()
    );
    let catalog = Classes::get();
    let aa = 0;
    let kk = 14;
    let p = t2.t2(aa, kk)?;
    println!(
        "AA vs KK equity: {:.12}",
        (p[0] + p[1] / 2.0) / f64::from(catalog.k(aa, kk))
    );
    if let Some(path) = export_t2 {
        let mut file = BufWriter::new(std::fs::File::create(path)?);
        writeln!(
            file,
            "hero_class,villain_class,hero_name,villain_name,n_hero,k,w_win,w_tie,w_lose"
        )?;
        for c in 0..169 {
            for d in 0..169 {
                let [win, tie, lose] = t2.counts(c, d)?;
                writeln!(
                    file,
                    "{c},{d},{},{},{},{},{win},{tie},{lose}",
                    catalog.name(c),
                    catalog.name(d),
                    catalog.n(c),
                    catalog.k(c, d)
                )?;
            }
        }
        file.flush()?;
    }
    if let Some(path) = export_classes {
        let mut file = BufWriter::new(std::fs::File::create(path)?);
        writeln!(file, "class,name,n,rep_combo,k_row_sum")?;
        for c in 0..169 {
            let sum: usize = (0..169).map(|d| usize::from(catalog.k(c, d))).sum();
            writeln!(
                file,
                "{c},{},{},{},{sum}",
                catalog.name(c),
                catalog.n(c),
                catalog.representative(c)
            )?;
        }
        file.flush()?;
    }
    let start = Instant::now();
    let t3 = ThreeWayTable::load_or_build(&dir, samples, seed)?;
    println!(
        "T3 build/load: {:.3}s; payload {} bytes; BLAKE3 {}",
        start.elapsed().as_secs_f64(),
        t3.payload_len(),
        t3.payload_hash()
    );
    Ok(())
}
