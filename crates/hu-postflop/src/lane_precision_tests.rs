//! Every lane width and terminal kind on actual turn-to-river rank tables.
use super::*;
use rayon::prelude::*;

#[test]
fn t26_all_widths_kinds_and_orders_match_f64_and_singletons() {
    let mut maximum = 0.0f64;
    for wide in [false, true] {
        let mut cfg = config();
        cfg.ranges = if wide {
            PerPlayer::new(nlh::Range::full(), nlh::Range::full())
        } else {
            PerPlayer::new(
                "22+,A2s+,K2s+,Q5s+,J7s+,T7s+,97s+,86s+,75s+,65s,54s,A5o+,K9o+,Q9o+,J9o+,T9o".parse().unwrap(),
                "99-22,AJs-A2s,KJs-K2s,Q4s+,J6s+,T6s+,96s+,85s+,74s+,64s+,53s+,43s,AJo-A2o,K8o+,Q9o+,J9o+,T8o+,98o".parse().unwrap(),
            )
        };
        let mut built = build_postflop_game(
            &cfg,
            PayoffPipeline {
                rake: &NoRake,
                utility: &ChipEv,
            },
        );
        let e = &mut built.game.evaluator;
        e.set_cfr_precision(CfrPrecision::F32);
        // Multiple real river boards create dead own rows inside the root
        // supports, testing overwritten worker buffers and chunk remainders.
        // Include the timed board and rivers that kill the final support
        // entry on each realistic seat, exercising dead rows in copy tails.
        let tables = ["9s", "Ah", "Js"].map(|card| {
            let river: nlh::Card = card.parse().unwrap();
            e.terminals
                .iter()
                .enumerate()
                .find_map(|(id, t)| {
                    (t.board_mask.count_ones() == 5
                        && t.board_mask & (1 << river.index()) != 0
                        && e.batch_tables[id] != u32::MAX)
                        .then_some(e.batch_tables[id])
                })
                .unwrap()
        });
        for table in tables {
            let candidates: [Vec<u32>; 2] = std::array::from_fn(|fold| {
                e.terminals
                    .iter()
                    .enumerate()
                    .filter_map(|(id, t)| {
                        (e.batch_tables[id] == table
                            && usize::from(matches!(
                                t.kind,
                                super::super::TerminalKind::Fold { .. }
                            )) == fold)
                            .then_some(id as u32)
                    })
                    .collect()
            });
            assert!(candidates.iter().all(|ids| !ids.is_empty()));
            for p in Player::BOTH {
                assert!(!e.rank_tables[table as usize][p].dead.is_empty());
                let ids: Vec<_> = candidates
                    .iter()
                    .flat_map(|ids| (0..8).map(|lane| ids[lane % ids.len()]))
                    .collect();
                for pattern in 0..5 {
                    let reaches: Vec<Vec<f32>> = ids
                        .iter()
                        .enumerate()
                        .map(|(slot, &id)| {
                            let board = e.terminals[id as usize].board_mask;
                            e.hands
                                .combos(p.opponent())
                                .iter()
                                .enumerate()
                                .map(|(h, &combo)| {
                                    let (a, b) = combo_cards(combo as usize);
                                    if board & ((1 << a.index()) | (1 << b.index())) != 0
                                        || pattern == 3
                                    {
                                        0.0
                                    } else if pattern == 4 || (h + slot) % 7 == 0 {
                                        -0.0
                                    } else {
                                        ((h * 37 + slot * 19) % 101 + 1) as f32 / 103.0
                                            * [1.0, 0.001, 100.0][pattern]
                                    }
                                })
                                .collect()
                        })
                        .collect();
                    let run = |order: &[usize], width: usize| {
                        let mut results = vec![vec![0.0; e.hands.len(p)]; ids.len()];
                        for slots in order.chunks(width) {
                            let ts: Vec<_> = slots.iter().map(|&i| ids[i]).collect();
                            let rs: Vec<_> = slots.iter().map(|&i| reaches[i].as_slice()).collect();
                            let mut rows = vec![vec![0.0; e.hands.len(p)]; slots.len()];
                            e.eval_cfr_batch(
                                &ts,
                                p,
                                &rs,
                                &mut rows.iter_mut().map(Vec::as_mut_slice).collect::<Vec<_>>(),
                            );
                            for (&slot, row) in slots.iter().zip(rows) {
                                results[slot] = row;
                            }
                        }
                        results
                    };
                    let order: Vec<_> = (0..ids.len()).collect();
                    let singleton = run(&order, 1);
                    for (slot, (&id, row)) in ids.iter().zip(&singleton).enumerate() {
                        let mut exact = vec![0.0; row.len()];
                        e.eval(id, p, &reaches[slot], &mut exact);
                        maximum = maximum.max(relative_error(&exact, row));
                        assert!(
                            maximum < 1e-5,
                            "wide={wide} p={p:?} slot={slot} pattern={pattern}: {maximum:e}"
                        );
                        for &dead in &e.rank_tables[table as usize][p].dead {
                            assert_eq!(row[dead as usize].to_bits(), 0);
                        }
                    }
                    let expected: Vec<_> = singleton.iter().map(|r| bits(r)).collect();
                    // Homogeneous calls guarantee every width1..8 is tested
                    // for both kernels; mixed calls test kind partitioning,
                    // reversed lane order, caller grouping and thread count.
                    for width in 1..=8 {
                        for start in [0, 8] {
                            let slots: Vec<_> = (start..start + width).collect();
                            let actual = run(&slots, width);
                            for &slot in &slots {
                                assert_eq!(bits(&actual[slot]), expected[slot]);
                            }
                        }
                    }
                    let mixed: Vec<_> = (0..8).flat_map(|i| [i, i + 8]).collect();
                    let mut reversed = mixed.clone();
                    reversed.reverse();
                    let mut rotated = mixed.clone();
                    rotated.rotate_left(5);
                    for threads in [1, 4] {
                        let pool = rayon::ThreadPoolBuilder::new()
                            .num_threads(threads)
                            .build()
                            .unwrap();
                        let variants = pool.install(|| {
                            [(&mixed, 16), (&reversed, 7), (&rotated, 5), (&order, 8)]
                                .par_iter()
                                .map(|(order, width)| {
                                    run(order, *width)
                                        .iter()
                                        .map(|r| bits(r))
                                        .collect::<Vec<_>>()
                                })
                                .collect::<Vec<_>>()
                        });
                        for actual in variants {
                            assert_eq!(actual, expected);
                        }
                    }
                }
            }
        }
        println!(
            "T26 wide={wide} supports={}/{} maximum relative infinity error={maximum:e}",
            e.hands.len(Player::P0),
            e.hands.len(Player::P1)
        );
    }
}
