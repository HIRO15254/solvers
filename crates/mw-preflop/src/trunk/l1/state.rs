//! Finished-iteration state. All floats are stored as raw little-endian bits.
//! Board/K4 streams depend only on seeds and t. Scratch pools are reset before
//! use; terminal ordering caches memoize pure settlement results, not learning.
use super::{BucketSource, Checkpoint, Options, Strategies, Timings, solve::Moments};
use crate::trunk::l0::{Model, Profile, Tree, eval::Reach};
use anyhow::{Context, Result, ensure};
use serde::{Serialize, Serializer, ser::SerializeSeq};
use std::io::{BufReader, BufWriter, Read, Write};

// Public JSON structs use flatten, which postcard cannot decode. Encode fixed
// tuples instead, borrowing the evaluations rather than cloning checkpoints.
struct Checkpoints<'a>(&'a [Checkpoint]);
impl Serialize for Checkpoints<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for c in self.0 {
            seq.serialize_element(&(c.iteration, &c.evaluation, c.seconds))?;
        }
        seq.end()
    }
}

type Metadata = (
    crate::trunk::l0::SolveTimings,
    [f64; 3],
    Vec<(u64, super::Evaluation, f64)>,
);

pub struct State {
    pub(super) iteration: u64,
    pub(super) elapsed: f64,
    pub(super) identity: [u8; 32],
    pub(super) profile: Profile,
    pub(super) regrets: Profile,
    pub(super) sums: Profile,
    pub(super) logit: Option<Profile>,
    pub(super) reach: Vec<Reach>,
    pub(super) postflop: Strategies,
    pub(super) regression: Vec<Moments>,
    pub(super) timings: Timings,
    pub(super) checkpoints: Vec<Checkpoint>,
}

impl State {
    pub fn iteration(&self) -> u64 {
        self.iteration
    }
}

pub(super) struct StateRef<'a> {
    pub iteration: u64,
    pub elapsed: f64,
    pub identity: &'a [u8; 32],
    pub tree: &'a Tree,
    pub profile: &'a Profile,
    pub regrets: &'a Profile,
    pub sums: &'a Profile,
    pub logit: Option<&'a Profile>,
    pub reach: &'a [Reach],
    pub postflop: &'a Strategies,
    pub regression: &'a [Moments],
    pub timings: &'a Timings,
    pub checkpoints: &'a [Checkpoint],
}

pub(super) fn identity(
    tree: &Tree,
    model: &Model<'_>,
    source: &dyn BucketSource,
    mut options: Options,
    storage: &Strategies,
) -> Result<[u8; 32]> {
    // Stop bounds may be extended without changing the iteration sequence.
    options.trunk.iterations = 0;
    options.max_seconds = None;
    options.stop_file = None;
    let mut hash = blake3::Hasher::new();
    hash.update(&tree.game_fingerprint);
    hash.update(&serde_json::to_vec(&options)?);
    if let Some(subset) = options.l1_flop_subset {
        hash.update(b"weighted-flop-subset.v1");
        hash.update(&subset.state_identity());
    }
    if !options.l1_train_overrides.is_empty() {
        hash.update(b"l1-train-override");
        for ((terminal, seat), values) in
            super::eval::override_map(tree, options.l1_train_overrides)?
        {
            hash.update(&(terminal as u64).to_le_bytes());
            hash.update(&(seat as u64).to_le_bytes());
            for value in values {
                hash.update(&[u8::from(value.is_some())]);
                if let Some(value) = value {
                    hash.update(&value.to_bits().to_le_bytes());
                }
            }
        }
    }
    hash.update(&source.state_identity());
    hash.update(&model.tables().state_identity());
    let m = model.evaluation_options();
    hash.update(&postcard::to_allocvec(&(
        m.k4_samples,
        m.seed,
        m.top_infosets,
    ))?);
    #[cfg(test)]
    hash.update(&[u8::from(m.sample_three_way)]);
    for w in model.weights() {
        for x in w {
            hash.update(&x.to_le_bytes());
        }
    }
    for street in [
        crate::Street::Flop,
        crate::Street::Turn,
        crate::Street::River,
    ] {
        hash.update(&(source.count(street) as u64).to_le_bytes());
        hash.update(&(source.classes(street) as u64).to_le_bytes());
    }
    hash.update(&[u8::from(source.relative())]);
    hash.update(&postcard::to_allocvec(&(
        tree.seats,
        tree.nodes.len(),
        storage.nodes,
        storage.slots,
    ))?);
    for n in &tree.nodes {
        hash.update(&postcard::to_allocvec(&(n.actor, &n.children, &n.labels))?);
    }
    for l in &storage.leaves {
        hash.update(&postcard::to_allocvec(&(
            l.terminal,
            &l.offsets,
            l.sums.len(),
        ))?);
    }
    Ok(*hash.finalize().as_bytes())
}

impl StateRef<'_> {
    pub fn write(&self, out: &mut impl Write) -> Result<()> {
        let mut out = BufWriter::new(out);
        let mut hash = blake3::Hasher::new();
        let mut put = |b: &[u8]| -> Result<()> {
            out.write_all(b)?;
            hash.update(b);
            Ok(())
        };
        put(b"P2L1STA1")?;
        put(&1u32.to_le_bytes())?;
        put(&self.tree.game_fingerprint)?;
        put(self.identity)?;
        put(&self.iteration.to_le_bytes())?;
        put(&self.elapsed.to_le_bytes())?;
        for p in [self.profile, self.regrets, self.sums] {
            // Solver profiles start explicit/uniform and never change defaulted
            // flags or fingerprint. Restore those invariants with uniform().
            for row in &p.rows {
                for x in row {
                    put(&x.to_le_bytes())?;
                }
            }
        }
        if let Some(logit) = self.logit {
            for row in &logit.rows {
                for x in row {
                    put(&x.to_le_bytes())?;
                }
            }
        }
        for r in self.reach {
            for x in r.pi.iter().chain(&r.mass) {
                put(&x.to_le_bytes())?;
            }
        }
        for l in &self.postflop.leaves {
            for x in l.regrets.iter().chain(&l.sums) {
                put(&x.to_le_bytes())?;
            }
        }
        for m in self.regression {
            put(&m.weight.to_le_bytes())?;
            for x in m.sums.iter().flatten() {
                put(&x.to_le_bytes())?;
            }
        }
        let metadata = postcard::to_allocvec(&(
            &self.timings.trunk,
            [
                self.timings.board_preparation,
                self.timings.postflop,
                self.timings.evaluation_board_preparation,
            ],
            Checkpoints(self.checkpoints),
        ))?;
        put(&(metadata.len() as u64).to_le_bytes())?;
        put(&metadata)?;
        out.write_all(hash.finalize().as_bytes())?;
        out.flush()?;
        Ok(())
    }
}

/// Decode and validate into private storage; no running state is modified on
/// failure. Dimensions are derived from the expected tree, never the file.
pub fn read_state(
    tree: &Tree,
    model: &Model<'_>,
    source: &dyn BucketSource,
    options: Options,
    input: &mut impl Read,
) -> Result<State> {
    let mut input = BufReader::new(input);
    let mut hash = blake3::Hasher::new();
    fn get<const N: usize>(input: &mut impl Read, hash: &mut blake3::Hasher) -> Result<[u8; N]> {
        let mut b = [0; N];
        input
            .read_exact(&mut b)
            .context("truncated L1 state or read error")?;
        hash.update(&b);
        Ok(b)
    }
    ensure!(
        &get::<8>(&mut input, &mut hash)? == b"P2L1STA1",
        "wrong L1 state magic"
    );
    ensure!(
        u32::from_le_bytes(get(&mut input, &mut hash)?) == 1,
        "unsupported L1 state version"
    );
    ensure!(
        get::<32>(&mut input, &mut hash)? == tree.game_fingerprint,
        "L1 state game fingerprint mismatch"
    );
    let mut postflop = Strategies::new(tree, source);
    let expected = identity(tree, model, source, options, &postflop)?;
    ensure!(
        get::<32>(&mut input, &mut hash)? == expected,
        "L1 state options/source/layout identity mismatch"
    );
    let iteration = u64::from_le_bytes(get(&mut input, &mut hash)?);
    ensure!(
        iteration <= options.trunk.iterations,
        "L1 state iteration exceeds iterations upper bound"
    );
    let elapsed = f64::from_le_bytes(get(&mut input, &mut hash)?);
    ensure!(
        elapsed.is_finite() && elapsed >= 0.0,
        "invalid L1 state elapsed time"
    );
    let mut profile = Profile::uniform(tree);
    let mut regrets = Profile::uniform(tree);
    let mut sums = Profile::uniform(tree);
    for p in [&mut profile, &mut regrets, &mut sums] {
        for row in &mut p.rows {
            for x in row {
                *x = f64::from_le_bytes(get(&mut input, &mut hash)?);
            }
        }
    }
    let mut logit = options.qre.map(|_| Profile::uniform(tree));
    if let Some(logit) = &mut logit {
        for row in &mut logit.rows {
            for x in row {
                *x = f64::from_le_bytes(get(&mut input, &mut hash)?);
            }
        }
        logit.validate(tree)?;
    }
    let mut reach = Vec::with_capacity(tree.nodes.len());
    for _ in &tree.nodes {
        let mut r = Reach {
            pi: vec![0.0; tree.seats * 169],
            mass: vec![0.0; tree.seats * 169],
        };
        for x in r.pi.iter_mut().chain(&mut r.mass) {
            *x = f64::from_le_bytes(get(&mut input, &mut hash)?);
        }
        reach.push(r);
    }
    for l in &mut postflop.leaves {
        for x in l.regrets.iter_mut().chain(&mut l.sums) {
            *x = f64::from_le_bytes(get(&mut input, &mut hash)?);
        }
    }
    let mut regression = vec![
        Moments {
            weight: 0.0,
            sums: [[0.0; 169]; 4]
        };
        postflop.leaves.len() * tree.seats
    ];
    for m in &mut regression {
        m.weight = f64::from_le_bytes(get(&mut input, &mut hash)?);
        for x in m.sums.iter_mut().flatten() {
            *x = f64::from_le_bytes(get(&mut input, &mut hash)?);
        }
    }
    let len = u64::from_le_bytes(get(&mut input, &mut hash)?);
    // Read in bounded chunks so a corrupt length cannot cause a giant allocation.
    let mut metadata = Vec::new();
    let mut remaining = len;
    while remaining > 0 {
        let n = remaining.min(8192) as usize;
        let mut b = [0; 8192];
        input
            .read_exact(&mut b[..n])
            .context("truncated L1 state metadata")?;
        hash.update(&b[..n]);
        metadata.extend_from_slice(&b[..n]);
        remaining -= n as u64;
    }
    let mut checksum = [0; 32];
    input
        .read_exact(&mut checksum)
        .context("truncated L1 state checksum")?;
    ensure!(
        checksum == *hash.finalize().as_bytes(),
        "L1 state checksum mismatch"
    );
    ensure!(input.read(&mut [0; 1])? == 0, "trailing bytes in L1 state");
    let (trunk, phases, saved): Metadata =
        postcard::from_bytes(&metadata).context("invalid L1 state metadata")?;
    let timings = Timings {
        trunk,
        board_preparation: phases[0],
        postflop: phases[1],
        evaluation_board_preparation: phases[2],
    };
    let checkpoints: Vec<_> = saved
        .into_iter()
        .map(|(iteration, evaluation, seconds)| Checkpoint {
            iteration,
            evaluation,
            seconds,
        })
        .collect();
    ensure!(
        checkpoints.iter().all(|c| c.iteration <= iteration),
        "L1 state checkpoint beyond saved iteration"
    );
    Ok(State {
        iteration,
        elapsed,
        identity: expected,
        profile,
        regrets,
        sums,
        logit,
        reach,
        postflop,
        regression,
        timings,
        checkpoints,
    })
}

/// Atomic checkpoint settings for fixed-profile Postflop training.
#[derive(Clone, Copy)]
pub struct FixedState<'a> {
    pub path: &'a std::path::Path,
    pub every_seconds: f64,
}

pub(super) fn fixed_identity(
    base: [u8; 32],
    profile: &Profile,
    storage: &Strategies,
    leaves: Option<&[usize]>,
    flops: Option<&super::FixedFlops>,
) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"p2-l1-fixed-state.v1");
    hash.update(&base);
    for x in profile.rows.iter().flatten() {
        hash.update(&x.to_bits().to_le_bytes());
    }
    for leaf in &storage.leaves {
        hash.update(&[u8::from(leaves.is_none_or(|l| l.contains(&leaf.terminal)))]);
    }
    match flops {
        None => {
            hash.update(b"all");
        }
        Some(super::FixedFlops::Weighted(subset)) => {
            hash.update(b"weighted");
            hash.update(&subset.state_identity());
        }
        Some(super::FixedFlops::RoundRobin(flops)) => {
            hash.update(b"round-robin");
            for flop in flops {
                for card in flop {
                    hash.update(&[card.index() as u8]);
                }
            }
        }
    }
    *hash.finalize().as_bytes()
}

pub(super) fn write_fixed(
    path: &std::path::Path,
    identity: &[u8; 32],
    iteration: u64,
    elapsed: f64,
    storage: &Strategies,
) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let mut file = std::fs::File::create(&tmp)?;
    {
        let mut out = BufWriter::new(&mut file);
        let mut hash = blake3::Hasher::new();
        let mut put = |bytes: &[u8]| -> Result<()> {
            out.write_all(bytes)?;
            hash.update(bytes);
            Ok(())
        };
        put(b"P2L1FIX1")?;
        put(identity)?;
        put(&iteration.to_le_bytes())?;
        put(&elapsed.to_le_bytes())?;
        for leaf in &storage.leaves {
            for x in leaf.regrets.iter().chain(&leaf.sums) {
                put(&x.to_bits().to_le_bytes())?;
            }
        }
        out.write_all(hash.finalize().as_bytes())?;
        out.flush()?;
    }
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)?;
    #[cfg(unix)]
    std::fs::File::open(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new(".")),
    )?
    .sync_all()?;
    Ok(())
}

pub(super) fn read_fixed(
    path: &std::path::Path,
    identity: &[u8; 32],
    storage: &mut Strategies,
) -> Result<(u64, f64)> {
    let mut input = BufReader::new(std::fs::File::open(path)?);
    let mut hash = blake3::Hasher::new();
    fn get<const N: usize>(input: &mut impl Read, hash: &mut blake3::Hasher) -> Result<[u8; N]> {
        let mut bytes = [0; N];
        input
            .read_exact(&mut bytes)
            .context("truncated fixed-profile state")?;
        hash.update(&bytes);
        Ok(bytes)
    }
    ensure!(
        &get::<8>(&mut input, &mut hash)? == b"P2L1FIX1",
        "invalid fixed-profile state"
    );
    ensure!(
        &get::<32>(&mut input, &mut hash)? == identity,
        "fixed-profile state identity mismatch"
    );
    let iteration = u64::from_le_bytes(get(&mut input, &mut hash)?);
    let elapsed = f64::from_le_bytes(get(&mut input, &mut hash)?);
    ensure!(
        elapsed.is_finite() && elapsed >= 0.0,
        "invalid fixed-profile state elapsed time"
    );
    for leaf in &mut storage.leaves {
        for x in leaf.regrets.iter_mut().chain(&mut leaf.sums) {
            *x = f64::from_le_bytes(get(&mut input, &mut hash)?);
            ensure!(x.is_finite(), "nonfinite fixed-profile state value");
        }
    }
    let mut checksum = [0; 32];
    input.read_exact(&mut checksum)?;
    ensure!(
        checksum == *hash.finalize().as_bytes(),
        "fixed-profile state checksum mismatch"
    );
    let mut trailing = [0];
    ensure!(
        input.read(&mut trailing)? == 0,
        "trailing fixed-profile state bytes"
    );
    Ok((iteration, elapsed))
}
