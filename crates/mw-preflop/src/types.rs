//! Small, table-sized primitives used by the sampled multiway path.
//!
//! The heads-up crates use a deliberately coarse chip grid.  Multiway uses
//! one thousand integral units per big blind so antes, odd chips, short
//! all-ins, and rake can all be settled without floating-point money.

use std::fmt;
use std::ops::{Add, AddAssign, Index, IndexMut, Sub, SubAssign};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

pub const CHIPS_PER_BB: u64 = 1_000;
pub const MIN_SEATS: usize = 2;
pub const MAX_SEATS: usize = 9;

/// Integral chip amount on the multiway solver's 0.001bb grid.
#[derive(
    Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct MwChips(pub u64);

impl MwChips {
    pub const ZERO: Self = Self(0);
    pub const ONE_BB: Self = Self(CHIPS_PER_BB);

    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub fn as_bb(self) -> f64 {
        self.0 as f64 / CHIPS_PER_BB as f64
    }

    /// Converts a UI/config big-blind amount to the integral 0.001bb grid.
    /// Half-grid values round away from zero (all accepted values are
    /// non-negative).
    pub fn try_from_bb(bb: f64) -> Result<Self, ChipAmountError> {
        if !bb.is_finite() {
            return Err(ChipAmountError::NotFinite);
        }
        if bb < 0.0 {
            return Err(ChipAmountError::Negative);
        }
        let scaled = bb * CHIPS_PER_BB as f64;
        if scaled > u64::MAX as f64 {
            return Err(ChipAmountError::Overflow);
        }
        Ok(Self(scaled.round() as u64))
    }

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(Self)
    }

    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        self.0.checked_sub(rhs.0).map(Self)
    }

    pub fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    pub const fn min(self, rhs: Self) -> Self {
        if self.0 <= rhs.0 { self } else { rhs }
    }

    pub const fn max(self, rhs: Self) -> Self {
        if self.0 >= rhs.0 { self } else { rhs }
    }
}

impl fmt::Display for MwChips {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add for MwChips {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        self.checked_add(rhs)
            .expect("multiway chip addition overflow")
    }
}

impl AddAssign for MwChips {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for MwChips {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self.checked_sub(rhs)
            .expect("multiway chip subtraction underflow")
    }
}

impl SubAssign for MwChips {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Error)]
pub enum ChipAmountError {
    #[error("chip amount must be finite")]
    NotFinite,
    #[error("chip amount may not be negative")]
    Negative,
    #[error("chip amount is too large")]
    Overflow,
}

/// Stable table index.  Seats are stored clockwise and never renumbered
/// after a fold or all-in.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize)]
#[serde(transparent)]
pub struct SeatId(pub u8);

impl SeatId {
    pub const fn new_unchecked(index: u8) -> Self {
        Self(index)
    }

    pub fn new(index: usize, num_seats: usize) -> Result<Self, SeatError> {
        validate_seat_count(num_seats)?;
        if index >= num_seats {
            return Err(SeatError::OutOfRange { index, num_seats });
        }
        Ok(Self(index as u8))
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub fn next(self, num_seats: usize) -> Self {
        debug_assert!((MIN_SEATS..=MAX_SEATS).contains(&num_seats));
        Self(((self.index() + 1) % num_seats) as u8)
    }

    pub fn advance(self, steps: usize, num_seats: usize) -> Self {
        debug_assert!((MIN_SEATS..=MAX_SEATS).contains(&num_seats));
        Self(((self.index() + steps) % num_seats) as u8)
    }
}

impl<'de> Deserialize<'de> for SeatId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let index = u8::deserialize(deserializer)?;
        if index as usize >= MAX_SEATS {
            return Err(D::Error::custom(format_args!(
                "seat index must be below {MAX_SEATS}, got {index}"
            )));
        }
        Ok(Self(index))
    }
}

impl fmt::Display for SeatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Error)]
pub enum SeatError {
    #[error("table must contain {MIN_SEATS} through {MAX_SEATS} seats, got {0}")]
    InvalidCount(usize),
    #[error("seat {index} is outside a {num_seats}-seat table")]
    OutOfRange { index: usize, num_seats: usize },
}

pub fn validate_seat_count(num_seats: usize) -> Result<(), SeatError> {
    if (MIN_SEATS..=MAX_SEATS).contains(&num_seats) {
        Ok(())
    } else {
        Err(SeatError::InvalidCount(num_seats))
    }
}

/// Compact set of table seats.  The upper seven bits are always clear for a
/// validated value, leaving room for cheap copies throughout the state
/// machine.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug, Serialize)]
#[serde(transparent)]
pub struct SeatMask(u16);

impl SeatMask {
    pub const EMPTY: Self = Self(0);

    pub fn all(num_seats: usize) -> Result<Self, SeatError> {
        validate_seat_count(num_seats)?;
        Ok(Self((1u16 << num_seats) - 1))
    }

    pub const fn from_seat(seat: SeatId) -> Self {
        Self(1u16 << seat.0)
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub const fn contains(self, seat: SeatId) -> bool {
        self.0 & (1u16 << seat.0) != 0
    }

    pub fn insert(&mut self, seat: SeatId) {
        debug_assert!(seat.index() < MAX_SEATS);
        self.0 |= 1u16 << seat.0;
    }

    pub fn remove(&mut self, seat: SeatId) {
        self.0 &= !(1u16 << seat.0);
    }

    pub const fn union(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }

    pub const fn intersection(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }

    pub const fn difference(self, rhs: Self) -> Self {
        Self(self.0 & !rhs.0)
    }

    pub fn iter(self) -> SeatMaskIter {
        SeatMaskIter(self.0)
    }
}

impl<'de> Deserialize<'de> for SeatMask {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bits = u16::deserialize(deserializer)?;
        if bits >> MAX_SEATS != 0 {
            return Err(D::Error::custom(format_args!(
                "seat mask contains bits beyond the {MAX_SEATS}-seat limit"
            )));
        }
        Ok(Self(bits))
    }
}

impl IntoIterator for SeatMask {
    type Item = SeatId;
    type IntoIter = SeatMaskIter;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct SeatMaskIter(u16);

impl Iterator for SeatMaskIter {
    type Item = SeatId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0 == 0 {
            return None;
        }
        let index = self.0.trailing_zeros();
        self.0 &= self.0 - 1;
        Some(SeatId(index as u8))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.0.count_ones() as usize;
        (len, Some(len))
    }
}

impl ExactSizeIterator for SeatMaskIter {}

/// A table-sized vector indexed by [`SeatId`].
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(transparent)]
pub struct SeatVec<T>(Vec<T>);

impl<T> SeatVec<T> {
    pub fn try_new(values: Vec<T>) -> Result<Self, SeatError> {
        validate_seat_count(values.len())?;
        Ok(Self(values))
    }

    pub(crate) fn new_unchecked(values: Vec<T>) -> Self {
        debug_assert!((MIN_SEATS..=MAX_SEATS).contains(&values.len()));
        Self(values)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.0.iter_mut()
    }

    pub fn seats(&self) -> impl ExactSizeIterator<Item = SeatId> + '_ {
        (0..self.len()).map(|index| SeatId(index as u8))
    }

    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.0
    }

    pub fn into_vec(self) -> Vec<T> {
        self.0
    }

    pub fn map<U>(self, mut f: impl FnMut(SeatId, T) -> U) -> SeatVec<U> {
        SeatVec::new_unchecked(
            self.0
                .into_iter()
                .enumerate()
                .map(|(index, value)| f(SeatId(index as u8), value))
                .collect(),
        )
    }
}

impl<'de, T> Deserialize<'de> for SeatVec<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<T>::deserialize(deserializer)?;
        Self::try_new(values).map_err(D::Error::custom)
    }
}

impl<T> Index<SeatId> for SeatVec<T> {
    type Output = T;

    fn index(&self, index: SeatId) -> &Self::Output {
        &self.0[index.index()]
    }
}

impl<T> IndexMut<SeatId> for SeatVec<T> {
    fn index_mut(&mut self, index: SeatId) -> &mut Self::Output {
        &mut self.0[index.index()]
    }
}

impl<T> IntoIterator for SeatVec<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a SeatVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Street {
    Preflop,
    Flop,
    Turn,
    River,
}

impl Street {
    pub const ALL: [Self; 4] = [Self::Preflop, Self::Flop, Self::Turn, Self::River];

    pub const fn index(self) -> usize {
        match self {
            Self::Preflop => 0,
            Self::Flop => 1,
            Self::Turn => 2,
            Self::River => 3,
        }
    }

    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Preflop => Some(Self::Flop),
            Self::Flop => Some(Self::Turn),
            Self::Turn => Some(Self::River),
            Self::River => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bb_conversion_uses_thousand_chip_grid() {
        assert_eq!(MwChips::try_from_bb(1.0).unwrap(), MwChips(1_000));
        assert_eq!(MwChips::try_from_bb(0.1254).unwrap(), MwChips(125));
        assert_eq!(MwChips(2_500).as_bb(), 2.5);
        assert!(MwChips::try_from_bb(f64::NAN).is_err());
        assert!(MwChips::try_from_bb(-0.1).is_err());
    }

    #[test]
    fn seat_masks_iterate_in_table_order() {
        let mut mask = SeatMask::EMPTY;
        mask.insert(SeatId(8));
        mask.insert(SeatId(2));
        mask.insert(SeatId(0));
        assert_eq!(mask.len(), 3);
        assert_eq!(
            mask.iter().collect::<Vec<_>>(),
            vec![SeatId(0), SeatId(2), SeatId(8)]
        );
        mask.remove(SeatId(2));
        assert!(!mask.contains(SeatId(2)));
    }

    #[test]
    fn seat_vec_checks_table_size_and_indexes_by_seat() {
        assert!(SeatVec::<u8>::try_new(vec![1]).is_err());
        let mut values = SeatVec::try_new(vec![10, 20, 30]).unwrap();
        values[SeatId(1)] = 25;
        assert_eq!(values[SeatId(1)], 25);
        assert_eq!(
            values.seats().collect::<Vec<_>>(),
            vec![SeatId(0), SeatId(1), SeatId(2)]
        );
    }

    #[test]
    fn serde_rejects_invalid_seat_primitives() {
        assert!(serde_json::from_str::<SeatId>("9").is_err());
        assert!(serde_json::from_str::<SeatMask>("512").is_err());
        assert!(serde_json::from_str::<SeatVec<u8>>("[1]").is_err());
        assert_eq!(
            serde_json::from_str::<SeatVec<u8>>("[1,2]")
                .unwrap()
                .as_slice(),
            &[1, 2]
        );
    }
}
