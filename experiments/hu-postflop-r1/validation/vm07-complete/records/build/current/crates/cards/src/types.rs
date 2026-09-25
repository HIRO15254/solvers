use std::fmt;
use std::ops::{Add, AddAssign, Index, IndexMut, Sub};

/// Chip amounts. Integral to keep terminal payoff baking exact; convert to
/// `f64` only inside utility models.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Chips(pub u32);

impl Chips {
    pub const ZERO: Chips = Chips(0);

    pub fn as_f64(self) -> f64 {
        self.0 as f64
    }
}

impl Add for Chips {
    type Output = Chips;
    fn add(self, rhs: Chips) -> Chips {
        Chips(self.0 + rhs.0)
    }
}

impl AddAssign for Chips {
    fn add_assign(&mut self, rhs: Chips) {
        self.0 += rhs.0;
    }
}

impl Sub for Chips {
    type Output = Chips;
    fn sub(self, rhs: Chips) -> Chips {
        Chips(self.0 - rhs.0)
    }
}

impl fmt::Display for Chips {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Betting street. `Preflop` doubles as "the only street" in toy games.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Street {
    Preflop,
    Flop,
    Turn,
    River,
}

/// One of the two players. The engine is intentionally heads-up only; this
/// type makes every N=2 assumption explicit and greppable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Player {
    P0,
    P1,
}

impl Player {
    pub const BOTH: [Player; 2] = [Player::P0, Player::P1];

    pub fn opponent(self) -> Player {
        match self {
            Player::P0 => Player::P1,
            Player::P1 => Player::P0,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Player::P0 => 0,
            Player::P1 => 1,
        }
    }

    pub fn from_index(index: usize) -> Player {
        match index {
            0 => Player::P0,
            1 => Player::P1,
            _ => panic!("player index out of range: {index}"),
        }
    }
}

/// A pair of values indexed by [`Player`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct PerPlayer<T>(pub [T; 2]);

impl<T> PerPlayer<T> {
    pub fn new(p0: T, p1: T) -> Self {
        PerPlayer([p0, p1])
    }

    pub fn map<U>(self, f: impl FnMut(T) -> U) -> PerPlayer<U> {
        PerPlayer(self.0.map(f))
    }

    pub fn as_ref(&self) -> PerPlayer<&T> {
        PerPlayer([&self.0[0], &self.0[1]])
    }
}

impl<T> Index<Player> for PerPlayer<T> {
    type Output = T;
    fn index(&self, player: Player) -> &T {
        &self.0[player.index()]
    }
}

impl<T> IndexMut<Player> for PerPlayer<T> {
    fn index_mut(&mut self, player: Player) -> &mut T {
        &mut self.0[player.index()]
    }
}
