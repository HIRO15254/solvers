//! Integer-chip HU allocation used by the bounded R1 prototypes.
//! High receives an odd split chip; ties award their odd chip to P0.
//! Rake is removed from each pot before high/low allocation.

use cards::{PerPlayer, Player};

use super::BoundaryError;
use crate::UtilityModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Award {
    P0,
    P1,
    Both,
}

#[derive(Clone, Debug)]
pub struct Pot {
    pub amount: u32,
    pub rake: u32,
    pub eligible: PerPlayer<bool>,
    pub high: Award,
    /// None means no qualifying low, so high receives the entire net pot.
    pub low: Option<Award>,
}

#[derive(Clone, Debug)]
pub struct Settlement {
    pub stacks_before: PerPlayer<u32>,
    pub contributions: PerPlayer<u32>,
    pub returned: PerPlayer<u32>,
    pub dead_money: u32,
    pub pots: Vec<Pot>,
}

#[derive(Clone, Debug)]
pub struct Settled {
    pub awards: PerPlayer<u32>,
    pub stacks_after: PerPlayer<u32>,
    pub rake: u32,
    pub utility: PerPlayer<f64>,
}

fn add(left: u32, right: u32) -> Result<u32, BoundaryError> {
    left.checked_add(right)
        .ok_or(BoundaryError::ArithmeticOverflow)
}

fn award(
    chips: u32,
    winners: Award,
    pot: &Pot,
    out: &mut PerPlayer<u32>,
) -> Result<(), BoundaryError> {
    let shares = match winners {
        Award::P0 => PerPlayer::new(chips, 0),
        Award::P1 => PerPlayer::new(0, chips),
        Award::Both => PerPlayer::new(chips.div_ceil(2), chips / 2),
    };
    for p in Player::BOTH {
        let is_winner = winners == Award::Both
            || matches!(
                (p, winners),
                (Player::P0, Award::P0) | (Player::P1, Award::P1)
            );
        if is_winner && !pot.eligible[p] {
            return Err(BoundaryError::InvalidSettlement);
        }
        out[p] = add(out[p], shares[p])?;
    }
    Ok(())
}

/// Build-time only: combine all chip awards/returns before invoking the
/// utility model. Never interpolate nonlinear win/tie/lose utilities.
pub fn settle(input: &Settlement, utility: &dyn UtilityModel) -> Result<Settled, BoundaryError> {
    for p in Player::BOTH {
        if input.contributions[p] > input.stacks_before[p]
            || input.returned[p] > input.contributions[p]
        {
            return Err(BoundaryError::InvalidSettlement);
        }
    }
    let supplied = add(
        add(
            input.contributions[Player::P0],
            input.contributions[Player::P1],
        )?,
        input.dead_money,
    )?;
    let mut accounted = add(input.returned[Player::P0], input.returned[Player::P1])?;
    let mut awards = PerPlayer::new(0, 0);
    let mut rake = 0;
    for pot in &input.pots {
        if pot.rake > pot.amount {
            return Err(BoundaryError::InvalidSettlement);
        }
        accounted = add(accounted, pot.amount)?;
        rake = add(rake, pot.rake)?;
        let net = pot.amount - pot.rake;
        if let Some(low) = pot.low {
            award(net.div_ceil(2), pot.high, pot, &mut awards)?;
            award(net / 2, low, pot, &mut awards)?;
        } else {
            award(net, pot.high, pot, &mut awards)?;
        }
    }
    if supplied != accounted {
        return Err(BoundaryError::InvalidSettlement);
    }
    let mut stacks_after = PerPlayer::new(0, 0);
    for p in Player::BOTH {
        stacks_after[p] = add(
            add(
                input.stacks_before[p] - input.contributions[p],
                input.returned[p],
            )?,
            awards[p],
        )?;
    }
    let baseline = utility.utility(&input.stacks_before.map(f64::from));
    let after = utility.utility(&stacks_after.map(f64::from));
    let values = PerPlayer::new(
        after[Player::P0] - baseline[Player::P0],
        after[Player::P1] - baseline[Player::P1],
    );
    if baseline
        .0
        .iter()
        .chain(after.0.iter())
        .chain(values.0.iter())
        .any(|value| !value.is_finite())
    {
        return Err(BoundaryError::NonFiniteUtility);
    }
    Ok(Settled {
        awards,
        stacks_after,
        rake,
        utility: values,
    })
}
