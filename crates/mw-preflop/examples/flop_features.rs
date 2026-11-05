//! Experimental: one CSV row per strategically distinct flop (suit
//! isomorphism) with its number of raw flops and, per hand class, the mean
//! flop bucket of its live combos in an EHS2 table (the 128-bucket table of
//! `--l1-relative`), for choosing representative flop subsets
//! (experiments/p2-method-2026-10/gtow-fixed/flop_subset.py).
use anyhow::{Context, Result};
use mw_preflop::{
    Street,
    card_abstraction::{Ehs2Abstraction, Ehs2Params},
    trunk::{
        classes::{Classes, class},
        l1::{BucketSource, FlopClass, canonical_flop},
    },
};
use nlh::{Card, NUM_COMBOS, combo_cards};
use std::{collections::BTreeMap, io::Write, path::PathBuf};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let table = PathBuf::from(args.next().context(
        "flop_features STRENGTH_TABLE_128 OUTPUT_CSV (the 128-bucket EHS2 table of --l1-relative)",
    )?);
    let output = PathBuf::from(args.next().context("missing OUTPUT_CSV")?);
    let strength = Ehs2Abstraction::load_or_build(
        Ehs2Params {
            flop_buckets: 128,
            turn_buckets: 128,
            river_buckets: 128,
        },
        &[Street::Flop, Street::Turn, Street::River],
        Some(&table),
    );
    let mut flops: BTreeMap<FlopClass, ([Card; 3], u32)> = BTreeMap::new();
    for a in 0..52 {
        for b in a + 1..52 {
            for c in b + 1..52 {
                let flop = [a, b, c].map(Card::from_index);
                flops.entry(canonical_flop(flop)).or_insert((flop, 0)).1 += 1;
            }
        }
    }
    let catalog = Classes::get();
    let mut out = std::io::BufWriter::new(std::fs::File::create(&output)?);
    write!(out, "flop,raw")?;
    for c in 0..169 {
        write!(out, ",{}", catalog.name(c))?;
    }
    writeln!(out)?;
    for (flop, raw) in flops.values() {
        let row = strength.row(flop);
        let mut sums = [0.0_f64; 169];
        let mut counts = [0_u32; 169];
        for (combo, &bucket) in row.iter().enumerate().take(NUM_COMBOS) {
            let (x, y) = combo_cards(combo);
            if flop.contains(&x) || flop.contains(&y) {
                continue;
            }
            sums[class(combo)] += f64::from(bucket);
            counts[class(combo)] += 1;
        }
        write!(out, "{} {} {},{raw}", flop[0], flop[1], flop[2])?;
        for c in 0..169 {
            if counts[c] == 0 {
                write!(out, ",")?;
            } else {
                write!(out, ",{:.3}", sums[c] / f64::from(counts[c]))?;
            }
        }
        writeln!(out)?;
    }
    println!("{} flops", flops.len());
    Ok(())
}
