use crate::schedule::Discounts;

/// Location of one action node's data inside a storage buffer. Layout is
/// action-major: element `(a, h)` lives at `offset + a * num_hands + h`.
#[derive(Clone, Copy, Debug)]
pub struct StorageRef {
    pub offset: usize,
    pub num_actions: u16,
    pub num_hands: u32,
}

impl StorageRef {
    pub fn len(&self) -> usize {
        self.num_actions as usize * self.num_hands as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Backend holding cumulative regrets and the cumulative (average) strategy.
///
/// The solver is generic over this trait (monomorphized — no `dyn` in the
/// hot loop) so that a quantized `I16Storage` backend can slot in later
/// without touching the traversal.
pub trait Storage: Send {
    fn new(len: usize) -> Self;

    /// Writes the regret-matching (current) strategy into `out` (`A*H`,
    /// action-major). Hands whose positive regrets sum to zero get the
    /// uniform strategy.
    fn regret_matching(&self, r: StorageRef, out: &mut [f32]);

    /// Applies pre-add discounting to stored regrets, then adds `inst`.
    fn update_regrets(&mut self, r: StorageRef, inst: &[f32], d: &Discounts);

    /// Applies pre-add discounting (or reset) to the cumulative strategy,
    /// then adds the reach-weighted current strategy `weighted`.
    fn accumulate_strategy(&mut self, r: StorageRef, weighted: &[f32], d: &Discounts);

    /// Writes the normalized average strategy into `out`. Hands never
    /// reached get the uniform strategy.
    fn average_strategy(&self, r: StorageRef, out: &mut [f32]);
}

/// Plain `f32` backend: two flat arenas.
pub struct F32Storage {
    regrets: Vec<f32>,
    strategy_sum: Vec<f32>,
}

impl F32Storage {
    pub fn snapshot(&self) -> (Vec<f32>, Vec<f32>) {
        (self.regrets.clone(), self.strategy_sum.clone())
    }

    pub fn restore(&mut self, snapshot: (Vec<f32>, Vec<f32>)) {
        assert_eq!(snapshot.0.len(), self.regrets.len());
        assert_eq!(snapshot.1.len(), self.strategy_sum.len());
        self.regrets = snapshot.0;
        self.strategy_sum = snapshot.1;
    }
}

fn normalize_columns(data: &[f32], r: StorageRef, out: &mut [f32]) {
    let (num_actions, num_hands) = (r.num_actions as usize, r.num_hands as usize);
    debug_assert_eq!(out.len(), r.len());
    for h in 0..num_hands {
        let mut total = 0.0f64;
        for a in 0..num_actions {
            total += data[a * num_hands + h].max(0.0) as f64;
        }
        if total > 0.0 {
            for a in 0..num_actions {
                out[a * num_hands + h] = (data[a * num_hands + h].max(0.0) as f64 / total) as f32;
            }
        } else {
            let uniform = 1.0 / num_actions as f32;
            for a in 0..num_actions {
                out[a * num_hands + h] = uniform;
            }
        }
    }
}

impl Storage for F32Storage {
    fn new(len: usize) -> Self {
        F32Storage {
            regrets: vec![0.0; len],
            strategy_sum: vec![0.0; len],
        }
    }

    fn regret_matching(&self, r: StorageRef, out: &mut [f32]) {
        normalize_columns(&self.regrets[r.offset..r.offset + r.len()], r, out);
    }

    fn update_regrets(&mut self, r: StorageRef, inst: &[f32], d: &Discounts) {
        let slice = &mut self.regrets[r.offset..r.offset + r.len()];
        debug_assert_eq!(inst.len(), slice.len());
        let (pos, neg) = (d.pos as f32, d.neg as f32);
        for (regret, &delta) in slice.iter_mut().zip(inst) {
            let factor = if *regret > 0.0 { pos } else { neg };
            let mut updated = *regret * factor + delta;
            if d.floor_neg && updated < 0.0 {
                updated = 0.0;
            }
            *regret = updated;
        }
    }

    fn accumulate_strategy(&mut self, r: StorageRef, weighted: &[f32], d: &Discounts) {
        let slice = &mut self.strategy_sum[r.offset..r.offset + r.len()];
        debug_assert_eq!(weighted.len(), slice.len());
        let avg = d.avg as f32;
        if d.reset_avg {
            slice.copy_from_slice(weighted);
        } else {
            for (sum, &w) in slice.iter_mut().zip(weighted) {
                *sum = *sum * avg + w;
            }
        }
    }

    fn average_strategy(&self, r: StorageRef, out: &mut [f32]) {
        normalize_columns(&self.strategy_sum[r.offset..r.offset + r.len()], r, out);
    }
}
