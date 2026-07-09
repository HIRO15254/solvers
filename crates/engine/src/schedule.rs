/// Multiplicative factors applied at iteration `t` (1-indexed) *before*
/// adding that iteration's contribution. Derived from the number of
/// completed iterations `s = t - 1`, matching the paper convention
/// "after iteration s, discount accumulated values".
#[derive(Clone, Copy, Debug)]
pub struct Discounts {
    /// Factor for positive accumulated regrets.
    pub pos: f64,
    /// Factor for non-positive accumulated regrets.
    pub neg: f64,
    /// Factor for the accumulated average strategy.
    pub avg: f64,
    /// Floor regrets at zero after the add (regret-matching+).
    pub floor_neg: bool,
    /// Discard the accumulated average strategy this iteration (b-inary's
    /// power-of-4 reset trick).
    pub reset_avg: bool,
}

/// Discounting policy, queried once per iteration — cold path, `dyn` is
/// fine. This is the primary algorithm A/B axis: CFR variants that only
/// differ in weighting (vanilla, CFR+, DCFR, Linear CFR, HS-DCFR) are one
/// implementation each.
pub trait DiscountSchedule: Send + Sync {
    fn at(&self, t: u64, planned_iters: Option<u64>) -> Discounts;

    fn name(&self) -> &'static str;

    /// Schedules like HS-DCFR anneal over a known iteration budget.
    fn requires_planned_iters(&self) -> bool {
        false
    }
}

/// Original CFR: uniform weighting, negative regrets kept.
pub struct Vanilla;

impl DiscountSchedule for Vanilla {
    fn at(&self, _t: u64, _planned: Option<u64>) -> Discounts {
        Discounts {
            pos: 1.0,
            neg: 1.0,
            avg: 1.0,
            floor_neg: false,
            reset_avg: false,
        }
    }

    fn name(&self) -> &'static str {
        "vanilla"
    }
}

/// CFR+ (Tammelin 2014): regret-matching+ floor and linearly weighted
/// average strategy. Alternating updates are the solver's default and are
/// not part of the schedule.
pub struct CfrPlus;

impl DiscountSchedule for CfrPlus {
    fn at(&self, t: u64, _planned: Option<u64>) -> Discounts {
        let s = (t - 1) as f64;
        Discounts {
            pos: 1.0,
            neg: 1.0,
            // cum_T = sum_t (t/T) x_t  <=>  multiply by (t-1)/t before adding.
            avg: if t > 1 { s / (s + 1.0) } else { 0.0 },
            floor_neg: true,
            reset_avg: false,
        }
    }

    fn name(&self) -> &'static str {
        "cfr+"
    }
}

/// Discounted CFR (Brown & Sandholm 2019). After iteration s, positive
/// regrets scale by s^alpha/(s^alpha+1), negatives by s^beta/(s^beta+1),
/// the average strategy by (s/(s+1))^gamma.
pub struct Dcfr {
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    /// Reset the average strategy at power-of-4 iterations. Measured to
    /// improve convergence on poker trees (observed in b-inary's solver);
    /// defaults on.
    pub pow4_reset: bool,
}

impl Default for Dcfr {
    /// Project default: alpha=1.5, beta=0, gamma=3.0, power-of-4 resets.
    fn default() -> Self {
        Dcfr {
            alpha: 1.5,
            beta: 0.0,
            gamma: 3.0,
            pow4_reset: true,
        }
    }
}

fn power_discount(s: f64, exponent: f64) -> f64 {
    if s <= 0.0 {
        // No completed iterations yet; nothing to discount.
        return 1.0;
    }
    let p = s.powf(exponent);
    if p.is_infinite() { 1.0 } else { p / (p + 1.0) }
}

fn dcfr_discounts(t: u64, alpha: f64, beta: f64, gamma: f64, pow4_reset: bool) -> Discounts {
    let s = (t - 1) as f64;
    Discounts {
        pos: power_discount(s, alpha),
        neg: power_discount(s, beta),
        avg: if t > 1 {
            (s / (s + 1.0)).powf(gamma)
        } else {
            0.0
        },
        floor_neg: false,
        reset_avg: pow4_reset
            && t.is_power_of_two()
            && t.trailing_zeros().is_multiple_of(2)
            && t > 1,
    }
}

impl DiscountSchedule for Dcfr {
    fn at(&self, t: u64, _planned: Option<u64>) -> Discounts {
        dcfr_discounts(t, self.alpha, self.beta, self.gamma, self.pow4_reset)
    }

    fn name(&self) -> &'static str {
        "dcfr"
    }
}

/// Linear CFR = DCFR(1, 1, 1): the variant to pair with sampling, where
/// zeroing negative regrets interacts badly with noise.
pub fn linear_cfr() -> Dcfr {
    Dcfr {
        alpha: 1.0,
        beta: 1.0,
        gamma: 1.0,
        pow4_reset: false,
    }
}

/// Training-free hyperparameter schedule for DCFR (arXiv 2404.09097):
/// alpha = 1 + 3t/n, beta = -1 - 2t/n, gamma = gamma0 - 5t/n over a planned
/// budget of n iterations. `HsDcfr { gamma0: 30.0 }` is the paper's poker
/// recommendation.
pub struct HsDcfr {
    pub gamma0: f64,
}

impl DiscountSchedule for HsDcfr {
    fn at(&self, t: u64, planned: Option<u64>) -> Discounts {
        let n = planned.expect("HsDcfr requires planned_iters") as f64;
        let frac = t as f64 / n;
        let alpha = 1.0 + 3.0 * frac;
        let beta = -1.0 - 2.0 * frac;
        let gamma = self.gamma0 - 5.0 * frac;
        dcfr_discounts(t, alpha, beta, gamma, false)
    }

    fn name(&self) -> &'static str {
        "hs-dcfr"
    }

    fn requires_planned_iters(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dcfr_default_parameters() {
        let d = Dcfr::default();
        assert_eq!((d.alpha, d.beta, d.gamma), (1.5, 0.0, 3.0));
        // beta = 0 halves negative regrets every iteration.
        let disc = d.at(10, None);
        assert!((disc.neg - 0.5).abs() < 1e-12);
        assert!(disc.pos > 0.9);
    }

    #[test]
    fn power_of_4_reset() {
        let d = Dcfr::default();
        let reset_iters: Vec<u64> = (1..=300).filter(|&t| d.at(t, None).reset_avg).collect();
        assert_eq!(reset_iters, vec![4, 16, 64, 256]);
    }

    #[test]
    fn cfr_plus_is_linear_weighting() {
        let s = CfrPlus;
        assert_eq!(s.at(1, None).avg, 0.0);
        assert!((s.at(2, None).avg - 0.5).abs() < 1e-12);
        assert!((s.at(10, None).avg - 0.9).abs() < 1e-12);
        assert!(s.at(5, None).floor_neg);
    }

    #[test]
    fn hs_dcfr_needs_budget() {
        let s = HsDcfr { gamma0: 30.0 };
        assert!(s.requires_planned_iters());
        let d = s.at(500, Some(1000));
        // Halfway through: alpha = 2.5, beta = -2, gamma = 27.5.
        assert!(d.pos > 0.99);
        assert!(d.neg < 0.01);
    }
}
