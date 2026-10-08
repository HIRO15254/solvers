// Microbenchmark for P1-T8a: bit-identical single-scan variants of the showdown kernel.
use std::hint::black_box;
use std::time::Instant;

#[derive(Clone, Copy)]
struct Hand {
    local: u16,
    cards: [u8; 2],
}
struct Ranked {
    hands: Vec<Hand>,
    groups: Vec<(u32, usize)>,
}
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}
fn make(n: usize, ngroups: usize, rng: &mut Rng) -> Ranked {
    let mut pairs = Vec::new();
    for a in 0..47u8 {
        for b in (a + 1)..47u8 {
            pairs.push([a, b]);
        }
    }
    for i in (1..pairs.len()).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        pairs.swap(i, j);
    }
    let hands: Vec<Hand> = pairs[..n].iter().enumerate().map(|(i, &c)| Hand { local: i as u16, cards: c }).collect();
    let ranks: Vec<u32> = (0..n).map(|_| (rng.next() % ngroups as u64) as u32).collect();
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by_key(|&i| ranks[i]);
    let hands: Vec<Hand> = idx.iter().map(|&i| hands[i]).collect();
    let ranks: Vec<u32> = idx.iter().map(|&i| ranks[i]).collect();
    let mut groups = Vec::new();
    for i in 0..n {
        if i + 1 == n || ranks[i + 1] != ranks[i] {
            groups.push((ranks[i], i + 1));
        }
    }
    Ranked { hands, groups }
}

#[inline(always)]
fn add_branch(hands: &[Hand], reach: &[f32], total: &mut f64, card: &mut [f64; 52]) {
    for h in hands {
        let r = reach[h.local as usize] as f64;
        if r != 0.0 {
            *total += r;
            card[h.cards[0] as usize] += r;
            card[h.cards[1] as usize] += r;
        }
    }
}
#[inline(always)]
fn add_nb(hands: &[Hand], reach: &[f32], total: &mut f64, card: &mut [f64; 52]) {
    for h in hands {
        let r = reach[h.local as usize] as f64;
        *total += r;
        card[h.cards[0] as usize] += r;
        card[h.cards[1] as usize] += r;
    }
}
const ZERO: [f64; 52] = [0.0; 52];

#[inline(always)]
fn emit(h: Hand, u: [f64; 3], win: f64, gt: f64, gc: &[f64; 52], all_total: f64, all_card: &[f64; 52], out: &mut [f32]) {
    let [a, b] = h.cards.map(usize::from);
    let tie = gt - gc[a] - gc[b];
    let compat = all_total - all_card[a] - all_card[b];
    let lose = compat - win - tie;
    out[h.local as usize] = (u[0] * win + u[1] * tie + u[2] * lose) as f32;
}

/// V: 0 old (branch, rescan), 1 branchless rescan, 2 codex (copy every group), 3 tied-only snapshot,
/// 4 win-first (stack buffer, snapshot fallback), 5 f64fold (not bit-identical), 6 old+lazy zero.
fn kernel<const V: u8>(own: &Ranked, opp: &Ranked, u: [f64; 3], reach: &[f32], out: &mut [f32]) {
    let add = if V == 0 || V == 6 { add_branch } else { add_nb };
    let mut all_total = 0.0;
    let mut all_card = [0.0; 52];
    add(&opp.hands, reach, &mut all_total, &mut all_card);
    let mut below_total = 0.0;
    let mut below_card = [0.0; 52];
    let (mut oi, mut os, mut start) = (0, 0, 0);
    let mut wins = [0.0f64; 64];
    for &(rank, end) in &own.groups {
        while oi < opp.groups.len() && opp.groups[oi].0 < rank {
            let oe = opp.groups[oi].1;
            let mut gt = 0.0;
            add(&opp.hands[os..oe], reach, &mut gt, &mut below_card);
            below_total += gt;
            os = oe;
            oi += 1;
        }
        let tied = oi < opp.groups.len() && opp.groups[oi].0 == rank;
        let oe = if tied { opp.groups[oi].1 } else { os };
        let mine = &own.hands[start..end];
        match V {
            0 | 1 | 5 => {
                let mut gt = 0.0;
                let mut gc = [0.0; 52];
                add(&opp.hands[os..oe], reach, &mut gt, &mut gc);
                for &h in mine {
                    let [a, b] = h.cards.map(usize::from);
                    emit(h, u, below_total - below_card[a] - below_card[b], gt, &gc, all_total, &all_card, out);
                }
                if tied {
                    below_total += gt;
                    if V == 5 {
                        for (x, g) in below_card.iter_mut().zip(gc) {
                            *x += g;
                        }
                    } else {
                        let mut un = 0.0;
                        add(&opp.hands[os..oe], reach, &mut un, &mut below_card);
                    }
                }
            }
            6 => {
                if tied {
                    let mut gt = 0.0;
                    let mut gc = [0.0; 52];
                    add(&opp.hands[os..oe], reach, &mut gt, &mut gc);
                    for &h in mine {
                        let [a, b] = h.cards.map(usize::from);
                        emit(h, u, below_total - below_card[a] - below_card[b], gt, &gc, all_total, &all_card, out);
                    }
                    below_total += gt;
                    let mut un = 0.0;
                    add(&opp.hands[os..oe], reach, &mut un, &mut below_card);
                } else {
                    for &h in mine {
                        let [a, b] = h.cards.map(usize::from);
                        emit(h, u, below_total - below_card[a] - below_card[b], 0.0, &ZERO, all_total, &all_card, out);
                    }
                }
            }
            2 => {
                let mut gt = 0.0;
                let mut gc = [0.0; 52];
                let mut next = below_card;
                for h in &opp.hands[os..oe] {
                    let r = reach[h.local as usize] as f64;
                    gt += r;
                    for c in h.cards.map(usize::from) {
                        gc[c] += r;
                        next[c] += r;
                    }
                }
                for &h in mine {
                    let [a, b] = h.cards.map(usize::from);
                    emit(h, u, below_total - below_card[a] - below_card[b], gt, &gc, all_total, &all_card, out);
                }
                if tied {
                    below_total += gt;
                    below_card = next;
                }
            }
            3 | 4 => {
                if !tied {
                    for &h in mine {
                        let [a, b] = h.cards.map(usize::from);
                        emit(h, u, below_total - below_card[a] - below_card[b], 0.0, &ZERO, all_total, &all_card, out);
                    }
                } else if V == 4 && mine.len() <= wins.len() {
                    for (w, h) in wins.iter_mut().zip(mine) {
                        let [a, b] = h.cards.map(usize::from);
                        *w = below_total - below_card[a] - below_card[b];
                    }
                    let mut gt = 0.0;
                    let mut gc = [0.0; 52];
                    for h in &opp.hands[os..oe] {
                        let r = reach[h.local as usize] as f64;
                        gt += r;
                        let [a, b] = h.cards.map(usize::from);
                        gc[a] += r;
                        gc[b] += r;
                        below_card[a] += r;
                        below_card[b] += r;
                    }
                    for (&w, &h) in wins.iter().zip(mine) {
                        emit(h, u, w, gt, &gc, all_total, &all_card, out);
                    }
                    below_total += gt;
                } else {
                    let snap = below_card;
                    let mut gt = 0.0;
                    let mut gc = [0.0; 52];
                    for h in &opp.hands[os..oe] {
                        let r = reach[h.local as usize] as f64;
                        gt += r;
                        let [a, b] = h.cards.map(usize::from);
                        gc[a] += r;
                        gc[b] += r;
                        below_card[a] += r;
                        below_card[b] += r;
                    }
                    for &h in mine {
                        let [a, b] = h.cards.map(usize::from);
                        emit(h, u, below_total - snap[a] - snap[b], gt, &gc, all_total, &all_card, out);
                    }
                    below_total += gt;
                }
            }
            _ => unreachable!(),
        }
        if tied {
            os = oe;
            oi += 1;
        }
        start = end;
    }
}

fn main() {
    let mut rng = Rng(0x9e3779b97f4a7c15);
    let names = ["old", "nobranch", "codex", "tiedsnap", "winfirst", "f64fold*", "lazyzero"];
    let fns: [fn(&Ranked, &Ranked, [f64; 3], &[f32], &mut [f32]); 7] =
        [kernel::<0>, kernel::<1>, kernel::<2>, kernel::<3>, kernel::<4>, kernel::<5>, kernel::<6>];
    for &(n, g, zero_pct) in &[(300usize, 120usize, 30u64), (700, 250, 30), (1000, 400, 50), (1000, 150, 10), (1000, 900, 60)] {
        let own = make(n, g, &mut rng);
        let opp = make(n, g, &mut rng);
        let reach: Vec<f32> = (0..n)
            .map(|_| {
                let x = rng.next() % 100;
                if x < zero_pct { 0.0 } else if x < zero_pct + 5 { 1e-9 * (rng.next() % 1000) as f32 } else { (rng.next() % 1000) as f32 / 1000.0 }
            })
            .collect();
        let mut base = vec![0.0f32; n];
        fns[0](&own, &opp, [1.0, 0.5, -1.0], &reach, &mut base);
        let mut out = vec![0.0f32; n];
        let reps = 20000;
        let mut res = vec![Vec::new(); fns.len()];
        for _round in 0..3 {
            for (i, f) in fns.iter().enumerate() {
                for _ in 0..200 {
                    f(&own, &opp, [1.0, 0.5, -1.0], &reach, &mut out);
                }
                let s = Instant::now();
                for _ in 0..reps {
                    f(black_box(&own), &opp, [1.0, 0.5, -1.0], black_box(&reach), black_box(&mut out));
                }
                res[i].push(s.elapsed().as_nanos() as f64 / reps as f64);
            }
        }
        for (i, f) in fns.iter().enumerate() {
            f(&own, &opp, [1.0, 0.5, -1.0], &reach, &mut out);
            let eq = out.iter().zip(&base).all(|(x, y)| x.to_bits() == y.to_bits());
            let best = res[i].iter().cloned().fold(f64::INFINITY, f64::min);
            println!("n={n:5} groups={g:4} zero={zero_pct:2}%  {:9} best {best:8.0} ns  all {:?}  bit-equal {eq}", names[i],
                res[i].iter().map(|x| x.round() as i64).collect::<Vec<_>>());
        }
    }
}
