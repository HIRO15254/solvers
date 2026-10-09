//! Exact T2 integer counts and independently seeded Monte Carlo T3 entries.
//!
//! Cache v1 is little endian: magic[8], version u32, kind u32 (2/3),
//! samples u64, seed u64, payload length u64, payload BLAKE3[32], class
//! membership[169] (0/1), complete-board flag u8; then dense ordered T2
//! triples of u64 or hero-major, triangular opponent T3 arrays of 13 f32.
//! Selected classes are ascending. T3 pairs are (0,0),(0,1),…,(1,1),… .

use super::classes::{Classes, Ordering3, class};
use crate::card_abstraction::buckets::canonical_river_sets;
use anyhow::{Context, Result, bail, ensure};
use nlh::{Card, NUM_CLASSES, NUM_COMBOS, rank_of};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;
use std::io::Write;
use std::path::Path;

pub const BOARDS_48: u64 = 1_712_304;
pub const DEFAULT_T3_SAMPLES: u64 = 4096;
const MAGIC: &[u8; 8] = b"P2TRUNK\0";
const VERSION: u32 = 1;
const HEADER_LEN: usize = 242;

#[derive(Clone)]
struct Selection {
    classes: Vec<usize>,
    positions: [Option<usize>; NUM_CLASSES],
}

impl Selection {
    fn new(classes: &[usize]) -> Result<Self> {
        ensure!(!classes.is_empty(), "empty class subset");
        ensure!(
            classes.iter().all(|&c| c < NUM_CLASSES),
            "class index outside 0..169"
        );
        let mut sorted = classes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        ensure!(sorted.len() == classes.len(), "duplicate class in subset");
        let mut positions = [None; NUM_CLASSES];
        for (i, &c) in sorted.iter().enumerate() {
            positions[c] = Some(i);
        }
        Ok(Self {
            classes: sorted,
            positions,
        })
    }
    fn pos(&self, c: usize) -> Result<usize> {
        self.positions
            .get(c)
            .copied()
            .flatten()
            .with_context(|| format!("class {c} absent from table"))
    }
    fn len(&self) -> usize {
        self.classes.len()
    }
}

/// Canonical data are integer W counts; probabilities are derived on demand.
pub struct HuShowdownTable {
    selection: Selection,
    w: Vec<[u64; 3]>,
    complete_boards: bool,
}

impl HuShowdownTable {
    /// Full builds are deliberately unavailable in debug mode.
    pub fn build() -> Result<Self> {
        ensure!(
            !cfg!(debug_assertions),
            "full T2 build requires release mode"
        );
        Self::build_subset(&(0..NUM_CLASSES).collect::<Vec<_>>())
    }
    /// All canonical boards, but only pairs within the selected classes.
    pub fn build_subset(classes: &[usize]) -> Result<Self> {
        let boards = canonical_river_sets();
        let mut table = Self::build_for_boards(classes, &boards)?;
        table.complete_boards = true;
        table.validate()?;
        Ok(table)
    }
    /// Supplied boards and positive multiplicities; W counts only, no full-board T2.
    pub fn build_for_boards(classes: &[usize], boards: &[([Card; 5], u32)]) -> Result<Self> {
        let selection = Selection::new(classes)?;
        for (board, m) in boards {
            ensure!(*m > 0, "zero board multiplicity");
            let bits = board.iter().fold(0_u64, |bits, c| bits | (1 << c.index()));
            ensure!(bits.count_ones() == 5, "board contains duplicate cards");
        }
        let catalog = Classes::get();
        let combos: Vec<_> = (0..NUM_COMBOS)
            .filter_map(|v| selection.positions[class(v)].map(|c| (v, c)))
            .collect();
        let size = selection.len() * selection.len();
        // Each fold owns reusable counters and scratch. Reduction adds integers only.
        let (w, _) = boards
            .par_iter()
            .fold(
                || (vec![[0_u64; 3]; size], Vec::with_capacity(combos.len())),
                |(mut w, mut live), (board, multiplicity)| {
                    live.clear();
                    let dead = board.iter().fold(0_u64, |bits, c| bits | (1 << c.index()));
                    for &(v, c) in &combos {
                        if catalog.combo_mask(v) & dead == 0 {
                            let rank = rank_of(board.iter().copied().chain(catalog.cards(v))).0;
                            live.push((catalog.combo_mask(v), c, rank));
                        }
                    }
                    let m = u64::from(*multiplicity);
                    for (i, &(mask_h, c, h)) in live.iter().enumerate() {
                        for &(mask_v, d, v) in &live[..i] {
                            if mask_h & mask_v != 0 {
                                continue;
                            }
                            let o = if h > v {
                                0
                            } else if h == v {
                                1
                            } else {
                                2
                            };
                            w[c * selection.len() + d][o] += m;
                            w[d * selection.len() + c][2 - o] += m;
                        }
                    }
                    (w, live)
                },
            )
            .reduce(
                || (vec![[0_u64; 3]; size], Vec::new()),
                |(mut a, scratch), (b, _)| {
                    for (a, b) in a.iter_mut().zip(b) {
                        for i in 0..3 {
                            a[i] += b[i];
                        }
                    }
                    (a, scratch)
                },
            );
        Ok(Self {
            selection,
            w,
            complete_boards: false,
        })
    }
    pub fn counts(&self, c: usize, d: usize) -> Result<[u64; 3]> {
        Ok(self.w[self.selection.pos(c)? * self.selection.len() + self.selection.pos(d)?])
    }
    /// Returns T2 (sums to K, rather than one).
    pub fn t2(&self, c: usize, d: usize) -> Result<[f64; 3]> {
        ensure!(
            self.complete_boards,
            "T2 probabilities require complete board enumeration"
        );
        let denominator = Classes::get().n(c) as f64 * BOARDS_48 as f64;
        Ok(self.counts(c, d)?.map(|w| w as f64 / denominator))
    }
    pub fn validate(&self) -> Result<()> {
        let catalog = Classes::get();
        for &c in &self.selection.classes {
            for &d in &self.selection.classes {
                let a = self.counts(c, d)?;
                let b = self.counts(d, c)?;
                ensure!(
                    a[0] == b[2] && a[1] == b[1],
                    "T2 symmetry failed at ({c},{d})"
                );
                if self.complete_boards {
                    ensure!(
                        a.iter().sum::<u64>()
                            == catalog.n(c) as u64 * u64::from(catalog.k(c, d)) * BOARDS_48,
                        "T2 total failed at ({c},{d})"
                    );
                }
            }
        }
        Ok(())
    }
    fn payload(&self) -> Vec<u8> {
        self.w
            .iter()
            .flatten()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }
    pub fn payload_hash(&self) -> blake3::Hash {
        blake3::hash(&self.payload())
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        write_cache(
            path,
            2,
            0,
            0,
            &self.selection,
            self.complete_boards,
            &self.payload(),
        )
    }
    pub fn load(path: &Path) -> Result<Self> {
        let (selection, complete_boards, payload) = read_cache(path, 2, 0, 0, 24)?;
        let w = payload
            .as_chunks::<24>()
            .0
            .iter()
            .map(|entry| {
                std::array::from_fn(|i| {
                    u64::from_le_bytes(entry[i * 8..i * 8 + 8].try_into().expect("fixed chunk"))
                })
            })
            .collect();
        let table = Self {
            selection,
            w,
            complete_boards,
        };
        table.validate()?;
        Ok(table)
    }
    pub fn load_or_build(dir: &Path) -> Result<Self> {
        let path = dir.join("t2-v1.bin");
        if path.try_exists()? {
            let table = Self::load(&path)?;
            ensure!(
                table.complete_boards && table.selection.len() == NUM_CLASSES,
                "cache is not a full T2 table"
            );
            return Ok(table);
        }
        let table = Self::build()?;
        table.save(&path)?;
        Ok(table)
    }
}

/// 13 f32 probabilities per hero and unordered opponent-class pair.
pub struct ThreeWayTable {
    selection: Selection,
    probabilities: Vec<[f32; 13]>,
    samples: u64,
    seed: u64,
}

fn pair_index(d: usize, e: usize, n: usize) -> usize {
    d * n - d * d.saturating_sub(1) / 2 + e - d
}

/// Every entry owns its RNG; this encoding is fixed by cache format v1.
fn entry_rng(seed: u64, c: usize, d: usize, e: usize) -> ChaCha8Rng {
    let mut hash = blake3::Hasher::new();
    hash.update(b"solvers.p2.trunk.t3.v1");
    for value in [seed, c as u64, d as u64, e as u64] {
        hash.update(&value.to_le_bytes());
    }
    ChaCha8Rng::from_seed(*hash.finalize().as_bytes())
}

/// Uniform ordered sampling without replacement by rejection. Sorting is
/// unnecessary: every unordered board has exactly 5! equiprobable orders.
fn sample_board(rng: &mut ChaCha8Rng, mut dead: u64) -> [Card; 5] {
    std::array::from_fn(|_| {
        loop {
            let i = rng.gen_range(0..52_u8);
            let bit = 1_u64 << i;
            if dead & bit == 0 {
                dead |= bit;
                break Card::from_index(i);
            }
        }
    })
}

fn sample_entry(c: usize, d: usize, e: usize, samples: u64, seed: u64) -> [f32; 13] {
    let catalog = Classes::get();
    let hero = catalog.representative(c);
    let dead_h = catalog.combo_mask(hero);
    let live = |c| {
        catalog
            .combos(c)
            .iter()
            .copied()
            .filter(|&v| catalog.combo_mask(v) & dead_h == 0)
            .collect::<Vec<_>>()
    };
    let aa = live(d);
    let bb = live(e);
    let mut rng = entry_rng(seed, c, d, e);
    let mut counts = [0_u64; 13];
    for _ in 0..samples {
        let a = aa[rng.gen_range(0..aa.len())];
        let b = bb[rng.gen_range(0..bb.len())];
        let board = sample_board(
            &mut rng,
            dead_h | catalog.combo_mask(a) | catalog.combo_mask(b),
        );
        let rank = |v| rank_of(board.into_iter().chain(catalog.cards(v))).0;
        counts[Ordering3::from_ranks(rank(hero), rank(a), rank(b)).index()] += 1;
    }
    std::array::from_fn(|i| {
        if d == e {
            let j = Ordering3::ALL[i].swap_opponents().index();
            ((counts[i] as f64 + counts[j] as f64) / (2.0 * samples as f64)) as f32
        } else {
            (counts[i] as f64 / samples as f64) as f32
        }
    })
}

impl ThreeWayTable {
    pub fn build(samples: u64, seed: u64) -> Result<Self> {
        ensure!(
            !cfg!(debug_assertions),
            "full T3 build requires release mode"
        );
        Self::build_subset(&(0..NUM_CLASSES).collect::<Vec<_>>(), samples, seed)
    }
    pub fn build_subset(classes: &[usize], samples: u64, seed: u64) -> Result<Self> {
        let selection = Selection::new(classes)?;
        ensure!(samples > 0, "T3 sample count must be positive");
        let n = selection.len();
        let pairs: Vec<_> = (0..n).flat_map(|d| (d..n).map(move |e| (d, e))).collect();
        let probabilities = (0..n * pairs.len())
            .into_par_iter()
            .map(|i| {
                let c = selection.classes[i / pairs.len()];
                let (d, e) = pairs[i % pairs.len()];
                sample_entry(c, selection.classes[d], selection.classes[e], samples, seed)
            })
            .collect();
        Ok(Self {
            selection,
            probabilities,
            samples,
            seed,
        })
    }
    /// Build only a single entry, with the same seed and symmetry as full tables.
    pub fn build_entry(c: usize, d: usize, e: usize, samples: u64, seed: u64) -> Result<[f32; 13]> {
        ensure!(
            [c, d, e].iter().all(|&c| c < NUM_CLASSES),
            "class index outside 0..169"
        );
        ensure!(samples > 0, "T3 sample count must be positive");
        if d <= e {
            Ok(sample_entry(c, d, e, samples, seed))
        } else {
            let p = sample_entry(c, e, d, samples, seed);
            Ok(swap(p))
        }
    }
    pub fn p3(&self, c: usize, d: usize, e: usize) -> Result<[f32; 13]> {
        let c = self.selection.pos(c)?;
        let d = self.selection.pos(d)?;
        let e = self.selection.pos(e)?;
        let n = self.selection.len();
        let p = self.probabilities[c * n * (n + 1) / 2 + pair_index(d.min(e), d.max(e), n)];
        Ok(if d > e { swap(p) } else { p })
    }
    fn payload(&self) -> Vec<u8> {
        self.probabilities
            .iter()
            .flatten()
            .flat_map(|p| p.to_le_bytes())
            .collect()
    }
    pub fn payload_hash(&self) -> blake3::Hash {
        blake3::hash(&self.payload())
    }
    pub fn payload_len(&self) -> usize {
        self.probabilities.len() * 13 * 4
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        write_cache(
            path,
            3,
            self.samples,
            self.seed,
            &self.selection,
            true,
            &self.payload(),
        )
    }
    pub fn load(path: &Path, samples: u64, seed: u64) -> Result<Self> {
        ensure!(samples > 0, "T3 sample count must be positive");
        let (selection, complete, payload) = read_cache(path, 3, samples, seed, 52)?;
        ensure!(complete, "invalid T3 complete flag");
        let probabilities: Vec<[f32; 13]> = payload
            .as_chunks::<52>()
            .0
            .iter()
            .map(|entry| {
                std::array::from_fn(|i| {
                    f32::from_le_bytes(entry[i * 4..i * 4 + 4].try_into().expect("fixed chunk"))
                })
            })
            .collect();
        for p in &probabilities {
            ensure!(
                p.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    && (p.iter().map(|&v| f64::from(v)).sum::<f64>() - 1.0).abs() < 1e-6,
                "invalid T3 probabilities"
            );
        }
        let table = Self {
            selection,
            probabilities,
            samples,
            seed,
        };
        for &c in &table.selection.classes {
            for &d in &table.selection.classes {
                let p = table.p3(c, d, d)?;
                ensure!(p == swap(p), "invalid T3 equal-opponent symmetry");
            }
        }
        Ok(table)
    }
    pub fn load_or_build(dir: &Path, samples: u64, seed: u64) -> Result<Self> {
        ensure!(samples > 0, "T3 sample count must be positive");
        let path = dir.join(format!("t3-v1-n{samples}-seed{seed}.bin"));
        if path.try_exists()? {
            let table = Self::load(&path, samples, seed)?;
            ensure!(
                table.selection.len() == NUM_CLASSES,
                "cache is not a full T3 table"
            );
            return Ok(table);
        }
        let table = Self::build(samples, seed)?;
        table.save(&path)?;
        Ok(table)
    }
}

fn swap(p: [f32; 13]) -> [f32; 13] {
    std::array::from_fn(|i| p[Ordering3::ALL[i].swap_opponents().index()])
}

fn write_cache(
    path: &Path,
    kind: u32,
    samples: u64,
    seed: u64,
    selection: &Selection,
    complete: bool,
    payload: &[u8],
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(MAGIC)?;
    file.write_all(&VERSION.to_le_bytes())?;
    file.write_all(&kind.to_le_bytes())?;
    for value in [samples, seed, payload.len() as u64] {
        file.write_all(&value.to_le_bytes())?;
    }
    file.write_all(blake3::hash(payload).as_bytes())?;
    file.write_all(&selection.positions.map(|p| u8::from(p.is_some())))?;
    file.write_all(&[u8::from(complete)])?;
    file.write_all(payload)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .with_context(|| format!("atomically writing {}", path.display()))?;
    Ok(())
}

fn read_cache(
    path: &Path,
    kind: u32,
    samples: u64,
    seed: u64,
    entry_size: usize,
) -> Result<(Selection, bool, Vec<u8>)> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    ensure!(bytes.len() >= HEADER_LEN, "truncated trunk cache header");
    ensure!(&bytes[..8] == MAGIC, "bad trunk cache magic");
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().expect("header checked"));
    let u64_at = |i| u64::from_le_bytes(bytes[i..i + 8].try_into().expect("header checked"));
    ensure!(u32_at(8) == VERSION, "unsupported trunk cache version");
    ensure!(u32_at(12) == kind, "wrong trunk cache table kind");
    ensure!(
        u64_at(16) == samples && u64_at(24) == seed,
        "trunk cache parameters mismatch"
    );
    let members = &bytes[72..241];
    ensure!(
        members.iter().all(|&v| v <= 1),
        "invalid class membership header"
    );
    let selection = Selection::new(
        &members
            .iter()
            .enumerate()
            .filter_map(|(c, &v)| (v == 1).then_some(c))
            .collect::<Vec<_>>(),
    )?;
    let n = selection.len();
    let entries = if kind == 2 {
        n * n
    } else {
        n * n * (n + 1) / 2
    };
    let length = entries * entry_size;
    ensure!(
        u64_at(32) == length as u64 && bytes.len() == HEADER_LEN + length,
        "trunk cache payload length mismatch"
    );
    ensure!(bytes[241] <= 1, "invalid complete-board header");
    let payload = &bytes[HEADER_LEN..];
    if blake3::hash(payload).as_bytes() != &bytes[40..72] {
        bail!("trunk cache payload BLAKE3 mismatch");
    }
    Ok((selection, bytes[241] == 1, payload.to_vec()))
}

#[cfg(test)]
mod tests;
