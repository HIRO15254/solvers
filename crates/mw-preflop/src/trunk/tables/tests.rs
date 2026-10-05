use super::*;
use crate::card_abstraction::buckets::canonical_river_set;
use nlh::iso::{all_suit_perms, permute_card};
use std::collections::BTreeSet;
use std::time::Instant;

fn workspace_cache() -> std::path::PathBuf {
    // Cargo runs library tests from the package directory. Keep regenerable
    // tables under the workspace's ignored cache, not a crate-local directory.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/p2-trunk")
}

fn named(name: &str) -> usize {
    (0..169).find(|&c| Classes::get().name(c) == name).unwrap()
}

#[test]
fn trunk_classes_and_weights() {
    let classes = Classes::get();
    assert_eq!((0..169).map(|c| classes.n(c)).sum::<usize>(), 1326);
    for (size, count) in [(6, 13), (4, 78), (12, 78)] {
        assert_eq!((0..169).filter(|&c| classes.n(c) == size).count(), count);
    }
    for c in 0..169 {
        assert_eq!(
            (0..169)
                .map(|d| usize::from(classes.k(c, d)))
                .sum::<usize>(),
            1225
        );
        assert_eq!(
            classes.representative(c),
            *classes.combos(c).iter().min().unwrap()
        );
        for &v in classes.combos(c) {
            let [a, b] = classes.cards(v);
            assert_eq!(class(v), c);
            assert_eq!(
                class(v),
                nlh::class_index(
                    a.rank().max(b.rank()),
                    a.rank().min(b.rank()),
                    a.suit() == b.suit()
                )
            );
        }
        for d in 0..169 {
            assert!(classes.k(c, d) >= 1);
            assert_eq!(
                classes.n(c) * usize::from(classes.k(c, d)),
                classes.n(d) * usize::from(classes.k(d, c))
            );
        }
    }
    assert_eq!(classes.k(named("AA"), named("AA")), 1);
    assert_eq!(classes.k(named("AKs"), named("AKo")), 6);
    assert_eq!(classes.k(named("AKo"), named("AKs")), 2);
    let random = classes.weights(&"random".parse().unwrap());
    assert_eq!(random.weights, [1.0; 169]);
    assert!(random.warnings.is_empty());
    let asymmetry = classes.weights(&"AhKh".parse().unwrap());
    assert_eq!(asymmetry.weights[named("AKs")], 0.25);
    assert_eq!(asymmetry.warnings.len(), 1);
    let warning = &asymmetry.warnings[0];
    assert_eq!(
        (
            warning.class,
            warning.name.as_str(),
            warning.min,
            warning.max
        ),
        (named("AKs"), "AKs", 0.0, 1.0)
    );
    assert!(
        classes
            .weights(&"AA:0.5,AKs:0.25,72o:0.1".parse().unwrap())
            .warnings
            .is_empty()
    );
}

#[test]
fn trunk_weak_orderings() {
    let mut seen = BTreeSet::new();
    // All 27 input triples over three levels, including every realizable
    // comparison-sign triple. Nontransitive sign triples cannot be ranks.
    for h in 0..3 {
        for a in 0..3 {
            for b in 0..3 {
                let o = Ordering3::from_ranks(h, a, b);
                let [rh, ra, rb] = o.ranks();
                assert_eq!(
                    (h.cmp(&a), h.cmp(&b), a.cmp(&b)),
                    (rh.cmp(&ra), rh.cmp(&rb), ra.cmp(&rb))
                );
                assert_eq!(Ordering3::from_ranks(h, b, a), o.swap_opponents());
                seen.insert(o.index());
            }
        }
    }
    assert_eq!(seen.len(), 13);
    for o in Ordering3::ALL {
        assert_eq!(o.swap_opponents().swap_opponents(), o);
        let [h, a, b] = o.ranks();
        assert_eq!(Ordering3::from_ranks(h, a, b), o);
        assert_eq!(Ordering3::from_index(o.index()), Some(o));
    }
    assert!(Ordering3::from_index(13).is_none());
}

fn sampled_canonical_boards() -> Vec<([Card; 5], u32)> {
    let mut rng = entry_rng(123, 0, 0, 0);
    (0..137)
        .map(|_| {
            let raw = sample_board(&mut rng, 0);
            let (key, _) = canonical_river_set(raw);
            let board = key.map(Card::from_index);
            let orbit: BTreeSet<_> = all_suit_perms()
                .into_iter()
                .map(|perm| {
                    let mut key = board.map(|c| permute_card(&perm, c).index());
                    key.sort_unstable();
                    key
                })
                .collect();
            (board, orbit.len() as u32)
        })
        .collect()
}

fn brute_board(c: usize, d: usize, board: [Card; 5]) -> [u64; 3] {
    let catalog = Classes::get();
    let dead = board.iter().fold(0_u64, |bits, c| bits | (1 << c.index()));
    let mut result = [0; 3];
    for &h in catalog.combos(c) {
        for &v in catalog.combos(d) {
            if (catalog.combo_mask(h) | catalog.combo_mask(v)) & dead != 0
                || catalog.combo_mask(h) & catalog.combo_mask(v) != 0
            {
                continue;
            }
            let h = rank_of(board.into_iter().chain(catalog.cards(h)));
            let v = rank_of(board.into_iter().chain(catalog.cards(v)));
            result[if h > v {
                0
            } else if h == v {
                1
            } else {
                2
            }] += 1;
        }
    }
    result
}

#[test]
fn trunk_t2_subset_and_brute_boards() {
    let classes = ["AA", "KK", "AKs", "72o", "T9s"].map(named);
    let boards = sampled_canonical_boards();
    let table = HuShowdownTable::build_for_boards(&classes, &boards).unwrap();
    table.validate().unwrap();
    assert!(table.counts(named("QQ"), classes[0]).is_err());
    assert!(table.t2(classes[0], classes[1]).is_err());
    for &c in &classes {
        for &d in &classes {
            let expected = boards.iter().fold([0; 3], |mut total, (board, m)| {
                let counts = brute_board(c, d, *board);
                for o in 0..3 {
                    total[o] += counts[o] * u64::from(*m);
                }
                total
            });
            assert_eq!(table.counts(c, d).unwrap(), expected);
        }
    }
    for &(board, m) in &boards[..3] {
        let single = HuShowdownTable::build_for_boards(&classes, &[(board, m)]).unwrap();
        for &c in &classes {
            for &d in &classes {
                assert_eq!(
                    single.counts(c, d).unwrap(),
                    brute_board(c, d, board).map(|n| n * u64::from(m))
                );
            }
        }
    }
    let single = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| HuShowdownTable::build_for_boards(&classes, &boards).unwrap());
    assert_eq!(table.w, single.w);
}

#[test]
fn trunk_t3_subset_deterministic() {
    let classes = ["AA", "KK", "72o", "32o"].map(named);
    let pool = |n| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .unwrap()
    };
    let one = pool(1).install(|| ThreeWayTable::build_subset(&classes, 512, 0).unwrap());
    let two = pool(2).install(|| ThreeWayTable::build_subset(&classes, 512, 0).unwrap());
    assert_eq!(one.payload(), two.payload());
    for &c in &classes {
        for &d in &classes {
            for &e in &classes {
                let p = one.p3(c, d, e).unwrap();
                assert!((p.iter().sum::<f32>() - 1.0).abs() < 1e-6);
                assert_eq!(p, swap(one.p3(c, e, d).unwrap()));
                assert_eq!(p, ThreeWayTable::build_entry(c, d, e, 512, 0).unwrap());
                if d == e {
                    assert_eq!(p, swap(p));
                }
            }
        }
    }
    let sanity = one.p3(named("AA"), named("72o"), named("32o")).unwrap();
    // H>A>B is about 0.475, not >0.5: A=B is appreciable on shared boards.
    // The exhaustive ignored model check independently verifies this entry.
    assert!((0.40..0.55).contains(&sanity[0]));
    assert!(sanity[0] + sanity[1] + sanity[9] > 0.5);
    assert!(one.p3(named("QQ"), classes[0], classes[1]).is_err());
    assert!(ThreeWayTable::build_subset(&classes, 0, 0).is_err());
}

#[test]
fn trunk_cache_roundtrip_and_reject_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let classes = [named("AA"), named("72o")];
    let t2 = HuShowdownTable::build_for_boards(&classes, &sampled_canonical_boards()[..2]).unwrap();
    let path2 = dir.path().join("t2.bin");
    t2.save(&path2).unwrap();
    assert_eq!(
        t2.payload(),
        HuShowdownTable::load(&path2).unwrap().payload()
    );
    let t3 = ThreeWayTable::build_subset(&classes, 32, 7).unwrap();
    let path3 = dir.path().join("t3.bin");
    t3.save(&path3).unwrap();
    assert_eq!(
        t3.payload(),
        ThreeWayTable::load(&path3, 32, 7).unwrap().payload()
    );
    assert!(ThreeWayTable::load(&path3, 31, 7).is_err());
    assert!(ThreeWayTable::load(&path3, 32, 8).is_err());
    assert!(HuShowdownTable::load(&path3).is_err());
    for path in [&path2, &path3] {
        let original = std::fs::read(path).unwrap();
        for offset in [0, 8, 12, 16, 24, 32, 40, 72, 241, HEADER_LEN] {
            let mut corrupt = original.clone();
            corrupt[offset] ^= 0x80;
            std::fs::write(path, &corrupt).unwrap();
            if path == &path2 {
                assert!(HuShowdownTable::load(path).is_err(), "offset {offset}");
            } else {
                assert!(ThreeWayTable::load(path, 32, 7).is_err(), "offset {offset}");
            }
        }
        std::fs::write(path, &original[..original.len() - 1]).unwrap();
        if path == &path2 {
            assert!(HuShowdownTable::load(path).is_err());
        } else {
            assert!(ThreeWayTable::load(path, 32, 7).is_err());
        }
    }
}

/// Independent explicit board enumeration outside a physical union of holes.
fn enumerate_boards(dead: u64, mut visit: impl FnMut([Card; 5])) {
    let deck: Vec<_> = (0..52_u8)
        .filter(|&i| dead & (1_u64 << i) == 0)
        .map(Card::from_index)
        .collect();
    for i in 0..deck.len() {
        for j in i + 1..deck.len() {
            for k in j + 1..deck.len() {
                for l in k + 1..deck.len() {
                    for m in l + 1..deck.len() {
                        visit([deck[i], deck[j], deck[k], deck[l], deck[m]]);
                    }
                }
            }
        }
    }
}

fn brute_representative(c: usize, d: usize) -> [u64; 3] {
    let catalog = Classes::get();
    let h = catalog.representative(c);
    catalog
        .combos(d)
        .par_iter()
        .map(|&v| {
            let mut counts = [0; 3];
            if catalog.combo_mask(h) & catalog.combo_mask(v) != 0 {
                return counts;
            }
            enumerate_boards(catalog.combo_mask(h) | catalog.combo_mask(v), |board| {
                let hr = rank_of(board.into_iter().chain(catalog.cards(h)));
                let vr = rank_of(board.into_iter().chain(catalog.cards(v)));
                counts[if hr > vr {
                    0
                } else if hr == vr {
                    1
                } else {
                    2
                }] += 1;
            });
            counts
        })
        .reduce(|| [0; 3], |a, b| std::array::from_fn(|i| a[i] + b[i]))
}

#[test]
#[ignore = "release-only complete T2 build and independent exhaustive checks"]
fn trunk_full_t2() {
    if cfg!(debug_assertions) {
        panic!("run trunk acceptance tests in release mode");
    }
    let start = Instant::now();
    let table = HuShowdownTable::build().unwrap();
    let elapsed = start.elapsed();
    table.validate().unwrap();
    let dir = workspace_cache();
    table.save(&dir.join("t2-v1.bin")).unwrap();
    eprintln!(
        "T2 full build {elapsed:?}; payload {} bytes; BLAKE3 {}",
        table.payload().len(),
        table.payload_hash()
    );
    for (c, d) in [("AA", "KK"), ("AKs", "QQ"), ("72o", "32o")] {
        let start = Instant::now();
        let (c, d) = (named(c), named(d));
        let counts = brute_representative(c, d);
        assert_eq!(
            table.counts(c, d).unwrap(),
            counts.map(|w| w * Classes::get().n(c) as u64)
        );
        eprintln!("T2 brute ({c},{d}) {:?}", start.elapsed());
    }
    let [w, t, _] = table.counts(named("AA"), named("KK")).unwrap();
    let denominator = Classes::get().n(named("AA")) as f64
        * f64::from(Classes::get().k(named("AA"), named("KK")))
        * BOARDS_48 as f64;
    let equity = (w as f64 + t as f64 / 2.0) / denominator;
    eprintln!("AA vs KK equity {equity:.12}");
    assert!((0.81..0.83).contains(&equity));
}

#[test]
#[ignore = "release-only exhaustive three-way model versus large-N Monte Carlo"]
fn trunk_t3_exact_model() {
    if cfg!(debug_assertions) {
        panic!("run trunk acceptance tests in release mode");
    }
    check_exact_model("AA", "KK", "QQ");
    check_exact_model("AA", "72o", "32o");
}

fn check_exact_model(hero: &str, a: &str, b: &str) {
    let catalog = Classes::get();
    let (c, d, e) = (named(hero), named(a), named(b));
    let h = catalog.representative(c);
    let dead_h = catalog.combo_mask(h);
    let pairs: Vec<_> = catalog
        .combos(d)
        .iter()
        .flat_map(|&a| catalog.combos(e).iter().map(move |&b| (a, b)))
        .filter(|&(a, b)| dead_h & (catalog.combo_mask(a) | catalog.combo_mask(b)) == 0)
        .collect();
    let start = Instant::now();
    // Average each A,B world's normalized board distribution. In general
    // world sizes differ when opponents overlap; do not pool raw counts.
    let exact = pairs
        .par_iter()
        .map(|&(a, b)| {
            let mut counts = [0_u64; 13];
            enumerate_boards(
                dead_h | catalog.combo_mask(a) | catalog.combo_mask(b),
                |board| {
                    let rank = |v| rank_of(board.into_iter().chain(catalog.cards(v))).0;
                    counts[Ordering3::from_ranks(rank(h), rank(a), rank(b)).index()] += 1;
                },
            );
            let total = counts.iter().sum::<u64>() as f64;
            counts.map(|v| v as f64 / total)
        })
        .reduce(|| [0.0; 13], |a, b| std::array::from_fn(|i| a[i] + b[i]))
        .map(|v| v / pairs.len() as f64);
    let n = 2_000_000;
    let p = ThreeWayTable::build_entry(c, d, e, n, 0).unwrap();
    for i in 0..13 {
        let se = (exact[i] * (1.0 - exact[i]) / n as f64).sqrt();
        assert!(
            (f64::from(p[i]) - exact[i]).abs() <= 5.0 * se + 1e-7,
            "ordering {i}: MC {} exact {} se {se}",
            p[i],
            exact[i]
        );
    }
    eprintln!(
        "T3 exact {hero};{a},{b} versus N={n}: {:?}; exact {exact:?}; MC {p:?}",
        start.elapsed()
    );
    if (hero, a, b) == ("AA", "72o", "32o") {
        assert!((0.47..0.48).contains(&exact[0]));
    }
}

#[test]
#[ignore = "release-only full T3 N=4096 timing and cache"]
fn trunk_full_t3() {
    if cfg!(debug_assertions) {
        panic!("run trunk acceptance tests in release mode");
    }
    let start = Instant::now();
    let table = ThreeWayTable::build(4096, 0).unwrap();
    let elapsed = start.elapsed();
    let path = workspace_cache().join("t3-v1-n4096-seed0.bin");
    table.save(&path).unwrap();
    assert_eq!(table.payload_len(), 2_427_685 * 13 * 4);
    let loaded = ThreeWayTable::load(&path, 4096, 0).unwrap();
    assert_eq!(table.payload_hash(), loaded.payload_hash());
    eprintln!(
        "T3 full build {elapsed:?}; payload {} bytes; file {} bytes; BLAKE3 {}",
        table.payload_len(),
        std::fs::metadata(&path).unwrap().len(),
        table.payload_hash()
    );
}
