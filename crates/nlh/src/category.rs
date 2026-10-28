//! Board-relative made hands and independent draw flags.
//!
//! Inputs must be distinct cards on a three-, four-, or five-card board.
//! Made hands partition combinations, in the order below. On the river,
//! `boardplay` takes precedence whenever the best five cards equal the board,
//! including when a hole card could substitute for a board card of equal rank.
//!
//! | Name | Definition |
//! |---|---|
//! | `boardplay` | River best five has the same rank as the board alone. |
//! | `straightflush`, `quads`, `fullhouse`, `flush`, `straight` | Best hand has the named standard poker rank. |
//! | `set` | Three of a kind from a pocket pair and board rank. |
//! | `trips` | Other three of a kind, including board trips with a better kicker. |
//! | `twopair` | Best hand is two pair, including pairs already on the board. |
//! | `overpair` | Pocket pair above every board rank. |
//! | `underpair` | Pocket pair below every board rank. |
//! | `toppair`, `secondpair`, `thirdpair` | Pair rank matches respectively the highest, second highest, or any lower distinct board rank; pocket pairs between board ranks use the same rank ordering. |
//! | `highcard` | No pair or better involving a hole card; includes board-only one-pair hands and ace-high. |
//!
//! Draws are independent flags, not mutually exclusive. `flushdraw` requires
//! exactly four cards of a suit, at least one in the hole; `oesd` and `gutshot`
//! require respectively at least two or exactly one distinct rank completing a
//! straight, excluding straights already present and outs for which the board
//! makes an equal or higher straight. `backdoorflush` and `backdoorstraight` are flop-only, requiring
//! respectively three cards of a suit (at least one in the hole), or a
//! five-rank straight window missing exactly two ranks that cannot already be
//! completed by the board alone. Draws do not promise a winning hand. `nodraw`
//! means no flags; `draw` means any flag; `combodraw` means a flush draw and
//! either straight draw. On the river all draw flags are false.
//!
//! `pair` groups all one-pair classes; `twopairplus` groups two pair and
//! stronger, excluding `boardplay`. Expressions use `!`, `&`, `|` in that
//! precedence order and parentheses.

use crate::{Card, rank_of};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MadeHand {
    BoardPlay,
    StraightFlush,
    Quads,
    FullHouse,
    Flush,
    Straight,
    Set,
    Trips,
    TwoPair,
    Overpair,
    TopPair,
    SecondPair,
    ThirdPair,
    Underpair,
    HighCard,
}

/// Independent draw flags. Several may be true at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Draw {
    pub flushdraw: bool,
    pub oesd: bool,
    pub gutshot: bool,
    pub backdoorflush: bool,
    pub backdoorstraight: bool,
}
impl Draw {
    fn any(self) -> bool {
        self.flushdraw || self.oesd || self.gutshot || self.backdoorflush || self.backdoorstraight
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classification {
    pub made: MadeHand,
    pub draw: Draw,
}

// Ten five-rank windows, including the wheel.
fn windows() -> impl Iterator<Item = u16> {
    std::iter::once((1 << 12) | 15).chain((0..=8).map(|i| 31 << i))
}
fn straight(mask: u16) -> bool {
    windows().any(|w| mask & w == w)
}

/// Classifies a live combo. Panics for an invalid board or repeated cards.
pub fn classify(board: &[Card], hole: [Card; 2]) -> Classification {
    assert!(
        (3..=5).contains(&board.len()),
        "board must contain 3..=5 cards"
    );
    let mut seen = 0u64;
    let mut counts = [0u8; 13];
    let mut suits = [0u16; 4];
    let mut board_mask = 0u16;
    for (i, card) in board.iter().copied().chain(hole).enumerate() {
        let bit = 1u64 << card.index();
        assert_eq!(seen & bit, 0, "cards must be distinct");
        seen |= bit;
        counts[card.rank() as usize] += 1;
        suits[card.suit() as usize] |= 1 << card.rank();
        if i < board.len() {
            board_mask |= 1 << card.rank();
        }
    }
    let mask = counts
        .iter()
        .enumerate()
        .fold(0u16, |m, (r, &n)| m | (u16::from(n > 0) << r));
    let board_play = board.len() == 5
        && rank_of(board.iter().copied()) == rank_of(board.iter().copied().chain(hole));
    let flush = suits.iter().any(|m| m.count_ones() >= 5);
    let sf = suits.iter().any(|&m| straight(m));
    let quads = counts.contains(&4);
    let trips = counts.iter().filter(|&&n| n >= 3).count();
    let pairs = counts.iter().filter(|&&n| n >= 2).count();
    let made = if board_play {
        MadeHand::BoardPlay
    } else if sf {
        MadeHand::StraightFlush
    } else if quads {
        MadeHand::Quads
    } else if trips > 0 && pairs >= 2 {
        MadeHand::FullHouse
    } else if flush {
        MadeHand::Flush
    } else if straight(mask) {
        MadeHand::Straight
    } else if trips > 0 {
        let r = counts.iter().rposition(|&n| n >= 3).unwrap() as u8;
        if hole[0].rank() == r && hole[1].rank() == r {
            MadeHand::Set
        } else {
            MadeHand::Trips
        }
    } else if pairs >= 2 {
        MadeHand::TwoPair
    } else if pairs == 1 {
        let r = counts.iter().position(|&n| n == 2).unwrap() as u8;
        let pocket = hole[0].rank() == hole[1].rank();
        let above = (board_mask >> (r + 1)).count_ones();
        if !hole.iter().any(|card| card.rank() == r) {
            MadeHand::HighCard
        } else if pocket && above == 0 && board_mask & (1 << r) == 0 {
            MadeHand::Overpair
        } else if pocket && board_mask & ((1 << r) - 1) == 0 {
            MadeHand::Underpair
        } else if above == 0 {
            MadeHand::TopPair
        } else if above == 1 {
            MadeHand::SecondPair
        } else {
            MadeHand::ThirdPair
        }
    } else {
        MadeHand::HighCard
    };
    let mut draw = Draw::default();
    if board.len() < 5 {
        for (s, &ranks) in suits.iter().enumerate() {
            let involved = hole.iter().any(|c| c.suit() as usize == s);
            draw.flushdraw |= involved && ranks.count_ones() == 4;
            draw.backdoorflush |= board.len() == 3 && involved && ranks.count_ones() == 3;
        }
        if !straight(mask) {
            let mut outs = 0u16;
            for (strength, w) in windows().enumerate() {
                let missing = w & !mask;
                if missing.count_ones() == 1
                    && (w & !board_mask).count_ones() > 1
                    && !windows()
                        .skip(strength)
                        .any(|board_window| (board_mask | missing) & board_window == board_window)
                {
                    outs |= missing;
                }
                if board.len() == 3
                    && missing.count_ones() == 2
                    && (w & !board_mask).count_ones() > 2
                {
                    draw.backdoorstraight = true;
                }
            }
            draw.oesd = outs.count_ones() >= 2;
            draw.gutshot = outs.count_ones() == 1;
        }
    }
    Classification { made, draw }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    Name(String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}
/// Parsed category expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryExpr(Expr);
/// Category syntax error at a UTF-8 byte offset.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("category expression at byte {offset}: {message}")]
pub struct CategoryError {
    pub offset: usize,
    pub message: String,
}
const NAMES: &[&str] = &[
    "boardplay",
    "straightflush",
    "quads",
    "fullhouse",
    "flush",
    "straight",
    "set",
    "trips",
    "twopair",
    "overpair",
    "toppair",
    "secondpair",
    "thirdpair",
    "underpair",
    "highcard",
    "flushdraw",
    "oesd",
    "gutshot",
    "backdoorflush",
    "backdoorstraight",
    "nodraw",
    "draw",
    "combodraw",
    "pair",
    "twopairplus",
];
struct Parser<'a> {
    text: &'a str,
    pos: usize,
}
impl Parser<'_> {
    fn skip(&mut self) {
        while let Some(c) = self.text[self.pos..].chars().next() {
            if !c.is_whitespace() {
                break;
            }
            self.pos += c.len_utf8();
        }
    }
    fn take(&mut self, c: char) -> bool {
        self.skip();
        if self.text[self.pos..].starts_with(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }
    fn error(&self, message: &str) -> CategoryError {
        CategoryError {
            offset: self.pos,
            message: message.into(),
        }
    }
    fn expr(&mut self) -> Result<Expr, CategoryError> {
        let mut e = self.and()?;
        while self.take('|') {
            e = Expr::Or(Box::new(e), Box::new(self.and()?));
        }
        Ok(e)
    }
    fn and(&mut self) -> Result<Expr, CategoryError> {
        let mut e = self.unary()?;
        while self.take('&') {
            e = Expr::And(Box::new(e), Box::new(self.unary()?));
        }
        Ok(e)
    }
    fn unary(&mut self) -> Result<Expr, CategoryError> {
        if self.take('!') {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if self.take('(') {
            let e = self.expr()?;
            if !self.take(')') {
                return Err(self.error("expected ')'"));
            }
            return Ok(e);
        }
        self.skip();
        let start = self.pos;
        while self.pos < self.text.len()
            && (self.text.as_bytes()[self.pos].is_ascii_lowercase()
                || self.text.as_bytes()[self.pos] == b'_')
        {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(self.error("expected category name"));
        }
        let name = &self.text[start..self.pos];
        if !NAMES.contains(&name) {
            return Err(CategoryError {
                offset: start,
                message: format!("unknown category {name:?}"),
            });
        }
        Ok(Expr::Name(name.into()))
    }
}
impl CategoryExpr {
    pub fn parse(text: &str) -> Result<Self, CategoryError> {
        let mut p = Parser { text, pos: 0 };
        let e = p.expr()?;
        p.skip();
        if p.pos != text.len() {
            return Err(p.error("unexpected character"));
        }
        Ok(Self(e))
    }
    pub fn matches(&self, board: &[Card], hole: [Card; 2]) -> bool {
        self.0.matches(classify(board, hole))
    }
    pub fn canonical(&self) -> String {
        self.0.render()
    }
}
impl Expr {
    fn render(&self) -> String {
        match self {
            Self::Name(n) => n.clone(),
            Self::Not(e) => format!("!({})", e.render()),
            Self::And(a, b) => format!("({} & {})", a.render(), b.render()),
            Self::Or(a, b) => format!("({} | {})", a.render(), b.render()),
        }
    }
    fn matches(&self, c: Classification) -> bool {
        use MadeHand::*;
        match self {
            Self::Not(e) => !e.matches(c),
            Self::And(a, b) => a.matches(c) && b.matches(c),
            Self::Or(a, b) => a.matches(c) || b.matches(c),
            Self::Name(n) => match n.as_str() {
                "boardplay" => c.made == BoardPlay,
                "straightflush" => c.made == StraightFlush,
                "quads" => c.made == Quads,
                "fullhouse" => c.made == FullHouse,
                "flush" => c.made == Flush,
                "straight" => c.made == Straight,
                "set" => c.made == Set,
                "trips" => c.made == Trips,
                "twopair" => c.made == TwoPair,
                "overpair" => c.made == Overpair,
                "toppair" => c.made == TopPair,
                "secondpair" => c.made == SecondPair,
                "thirdpair" => c.made == ThirdPair,
                "underpair" => c.made == Underpair,
                "highcard" => c.made == HighCard,
                "flushdraw" => c.draw.flushdraw,
                "oesd" => c.draw.oesd,
                "gutshot" => c.draw.gutshot,
                "backdoorflush" => c.draw.backdoorflush,
                "backdoorstraight" => c.draw.backdoorstraight,
                "nodraw" => !c.draw.any(),
                "draw" => c.draw.any(),
                "combodraw" => c.draw.flushdraw && (c.draw.oesd || c.draw.gutshot),
                "pair" => matches!(
                    c.made,
                    Overpair | TopPair | SecondPair | ThirdPair | Underpair
                ),
                "twopairplus" => matches!(
                    c.made,
                    TwoPair | Trips | Set | Straight | Flush | FullHouse | Quads | StraightFlush
                ),
                _ => unreachable!(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ALL_CARDS,
        iso::{all_suit_perms, permute_card},
    };
    fn cards(s: &str) -> Vec<Card> {
        s.split_whitespace().map(|x| x.parse().unwrap()).collect()
    }
    fn example(board: &str, hole: &str, expected: MadeHand) {
        let b = cards(board);
        let h = cards(hole);
        assert_eq!(
            classify(&b, [h[0], h[1]]).made,
            expected,
            "{board} / {hole}"
        );
    }
    #[test]
    fn made_examples() {
        use MadeHand::*;
        for (b, h, m) in [
            ("2c 3c 4c", "5c Ac", StraightFlush),
            ("Ac Ad Ah", "As Kc", Quads),
            ("Ac Ad Kc", "Ah Kd", FullHouse),
            ("2c 5c 9c", "Kc Jc", Flush),
            ("2c 3d 4h", "5s Ac", Straight),
            ("Ac 7d 2h", "As Ah", Set),
            ("Ac Ad 7h", "As Kc", Trips),
            ("Ac Kd 2h", "As Kh", TwoPair),
            ("Kc 7d 2h", "As Ah", Overpair),
            ("Ac 7d 2h", "As Kh", TopPair),
            ("Ac 7d 2h", "7s Kh", SecondPair),
            ("Ac 7d 2h", "2s Kh", ThirdPair),
            ("Ac 7d 4h", "2s 2h", Underpair),
            ("Ac 7d 4h", "Ks Qh", HighCard),
            ("Ac Ad 7h", "Ks Qh", HighCard),
            ("Qc 7d 7h", "As Kh", HighCard),
            ("2c 3d 4h 5s 6c", "As Kc", BoardPlay),
            ("2c 3c 7c 9c Kc", "As Qh", BoardPlay),
            ("Ac Ad 7h", "7s Kh", TwoPair),
            ("Ac Ad 7h 7d 2c", "Ks Qh", TwoPair),
        ] {
            example(b, h, m);
        }
    }
    #[test]
    fn draw_examples_and_precedence() {
        let eval = |board: &str, hole: &str, expr: &str| {
            let b = cards(board);
            let h = cards(hole);
            CategoryExpr::parse(expr).unwrap().matches(&b, [h[0], h[1]])
        };
        assert!(eval("9c 8c 2d", "Tc Jc", "flushdraw & oesd & combodraw"));
        assert!(eval("9c 7d 2h", "Tc Jd", "gutshot & !oesd"));
        assert!(eval("9c 7d 2h", "Ac Kc", "backdoorflush"));
        assert!(eval("9c 7d 2h", "Tc Ad", "backdoorstraight"));
        assert!(eval("9c Td Jh Qs", "7c 2d", "nodraw & !gutshot & !oesd"));
        assert!(eval("5c 6d 7h 8s", "Tc 2d", "gutshot & !oesd"));
        // 7-J is already made; the only higher out (Q) completes a board-only straight.
        assert!(eval("8c 9d Th Js", "7c 2d", "straight & !gutshot & !oesd"));
        assert!(eval(
            "8c 9c Td Jh",
            "7c 2c",
            "straight & flushdraw & !gutshot & !combodraw"
        ));
        assert!(eval(
            "9c Tc Jd Qh",
            "7c 2c",
            "flushdraw & !combodraw & !gutshot"
        ));
        assert!(eval("Ac 7d 2h", "As Kc", "toppair | underpair & flushdraw"));
        assert!(!eval(
            "Ac 7d 2h",
            "As Kc",
            "(toppair | underpair) & flushdraw"
        ));
        assert!(eval("2c 3d 4h 5s 6c", "As Kc", "boardplay & nodraw"));
    }
    #[test]
    fn expressions() {
        let e = CategoryExpr::parse("toppair | pair & !flushdraw").unwrap();
        assert_eq!(CategoryExpr::parse(&e.canonical()).unwrap(), e);
        assert_eq!(CategoryExpr::parse("toppair & ?").unwrap_err().offset, 10);
        assert_eq!(CategoryExpr::parse("é").unwrap_err().offset, 0);
        assert_eq!(CategoryExpr::parse("(pair").unwrap_err().offset, 5);
    }
    #[test]
    fn suit_invariance_and_river_draws() {
        for b in [
            "Ac 7d 2h",
            "9c 8c 2d",
            "Ac Ad 7h 2s",
            "2c 3d 4h 5s 6c",
            "Ac Ad 7h 7d 2c",
        ] {
            let made_names: Vec<_> = NAMES[..15]
                .iter()
                .map(|n| CategoryExpr::parse(n).unwrap())
                .collect();
            let board = cards(b);
            for a in ALL_CARDS {
                for z in ALL_CARDS {
                    if a >= z || board.contains(&a) || board.contains(&z) {
                        continue;
                    }
                    let c = classify(&board, [a, z]);
                    assert_eq!(made_names.iter().filter(|e| e.0.matches(c)).count(), 1);
                    if board.len() == 5 {
                        assert_eq!(c.draw, Draw::default());
                    }
                    for p in all_suit_perms() {
                        let pb: Vec<_> = board.iter().map(|&x| permute_card(&p, x)).collect();
                        assert_eq!(c, classify(&pb, [permute_card(&p, a), permute_card(&p, z)]));
                    }
                }
            }
        }
    }
}
