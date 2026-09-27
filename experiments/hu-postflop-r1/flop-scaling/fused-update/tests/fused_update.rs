//! Research-only regression fixture. Copy into the candidate engine/tests/
//! in an isolated workspace before a separately bounded native test run.
//! This file has not been compiled or run during source preparation.
//! The reference calls the unchanged original storage methods; it is not
//! an independent CFR oracle and shares no code with crates/cfr-ref.

use engine::{
    CfrPlus, CfrUpdate, Dcfr, DiscountSchedule, Discounts, F32Storage, HsDcfr, I16Storage, Storage,
    StorageOps, StorageRef, StorageSpan, StorageState, StorageView, Vanilla, linear_cfr,
};

#[derive(Clone)]
struct Inputs {
    action_values: Vec<f32>,
    node_values: Vec<f32>,
    reach: Vec<f32>,
    strategy: Vec<f32>,
}

fn inputs(r: StorageRef) -> Inputs {
    let (actions, hands) = (r.num_actions as usize, r.num_hands as usize);
    let tiny = f32::from_bits(1);
    let values = [
        0.0,
        -0.0,
        tiny,
        -tiny,
        1.0 / 3.0,
        -2.25,
        f32::from_bits(0x3f80_0001),
    ];
    let reaches = [-0.0, 0.0, tiny, 0.3, 1.0];
    let mut result = Inputs {
        action_values: (0..r.len()).map(|i| values[i % values.len()]).collect(),
        node_values: vec![0.0; hands],
        reach: (0..hands).map(|h| reaches[h % reaches.len()]).collect(),
        strategy: vec![0.0; r.len()],
    };
    for a in 0..actions {
        let probability = match actions {
            1 => 1.0,
            3 => [0.2, 0.3, 0.5][a],
            _ => unreachable!("fixture uses zero, one or three actions"),
        };
        for h in 0..hands {
            let i = a * hands + h;
            result.strategy[i] = probability;
            result.node_values[h] += probability * result.action_values[i];
        }
    }
    result
}

fn original_update(ops: &mut impl StorageOps, r: StorageRef, x: &mut Inputs, d: &Discounts) {
    let (actions, hands) = (r.num_actions as usize, r.num_hands as usize);
    for a in 0..actions {
        for h in 0..hands {
            x.action_values[a * hands + h] -= x.node_values[h];
        }
    }
    ops.update_regrets(r, r.index, &x.action_values, d);
    for a in 0..actions {
        for h in 0..hands {
            x.action_values[a * hands + h] = x.reach[h] * x.strategy[a * hands + h];
        }
    }
    ops.accumulate_strategy(r, r.index, &x.action_values, d);
}

fn update(ops: &mut impl StorageOps, r: StorageRef, x: &mut Inputs, d: &Discounts, fused: bool) {
    if fused {
        ops.fused_update(
            r,
            r.index,
            CfrUpdate {
                action_values: &mut x.action_values,
                node_values: &x.node_values,
                reach: &x.reach,
                strategy: &x.strategy,
            },
            d,
        );
    } else {
        original_update(ops, r, x, d);
    }
}

fn apply<S: Storage>(
    s: &mut S,
    r: StorageRef,
    x: &mut Inputs,
    d: &Discounts,
    split: bool,
    fused: bool,
) {
    if split {
        let span = StorageSpan {
            // Nonzero global base and local offset, with sentinel neighbors
            // inside the borrowed view as well as outside its bounds.
            start: r.offset - 1,
            end: r.offset + r.len() + 1,
            sref_start: r.index,
            sref_end: r.index + 1,
        };
        let mut whole = s.view_mut();
        let mut views = whole.split(&[span]);
        update(&mut views[0], r, x, d, fused);
    } else {
        update(s, r, x, d, fused);
    }
}

fn seeded<S: Storage>(len: usize) -> S {
    let mut storage = S::new(len, 3);
    let state = match storage.state() {
        StorageState::F32 {
            mut regrets,
            mut strategy_sum,
        } => {
            let pattern = [0.0, -0.0, 2.0, -3.0, f32::from_bits(1), -f32::from_bits(1)];
            for i in 0..len {
                regrets[i] = pattern[i % pattern.len()];
                strategy_sum[i] = pattern[(i + 1) % pattern.len()];
            }
            StorageState::F32 {
                regrets,
                strategy_sum,
            }
        }
        StorageState::I16 {
            mut regrets,
            mut strategy_sum,
            mut regret_scales,
            mut strategy_scales,
        } => {
            let pattern = [0, 1, -1, 17, -29, 32_000];
            for i in 0..len {
                regrets[i] = pattern[i % pattern.len()];
                strategy_sum[i] = pattern[(i + 1) % pattern.len()];
            }
            regret_scales.copy_from_slice(&[0.125, 0.25, 0.5]);
            strategy_scales.copy_from_slice(&[0.5, 0.125, 0.25]);
            StorageState::I16 {
                regrets,
                strategy_sum,
                regret_scales,
                strategy_scales,
            }
        }
    };
    storage.restore_state(state).unwrap();
    storage
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}

fn state_bits(s: StorageState) -> Vec<Vec<u32>> {
    match s {
        StorageState::F32 {
            regrets,
            strategy_sum,
        } => vec![bits(&regrets), bits(&strategy_sum)],
        StorageState::I16 {
            regrets,
            strategy_sum,
            regret_scales,
            strategy_scales,
        } => vec![
            regrets.iter().map(|&v| v as u16 as u32).collect(),
            strategy_sum.iter().map(|&v| v as u16 as u32).collect(),
            bits(&regret_scales),
            bits(&strategy_scales),
        ],
    }
}

fn assert_sentinels(before: &[Vec<u32>], after: &[Vec<u32>], r: StorageRef) {
    for (old, new) in before[..2].iter().zip(&after[..2]) {
        assert_eq!(&old[..r.offset], &new[..r.offset]);
        assert_eq!(&old[r.offset + r.len()..], &new[r.offset + r.len()..]);
    }
    for (old, new) in before[2..].iter().zip(&after[2..]) {
        let i = r.index as usize;
        assert_eq!(&old[..i], &new[..i]);
        assert_eq!(&old[i + 1..], &new[i + 1..]);
    }
}

fn discounts() -> Vec<Discounts> {
    vec![
        Vanilla.at(1, None),
        CfrPlus.at(1, None),
        CfrPlus.at(7, None),
        Dcfr::default().at(2, None),
        Dcfr::default().at(4, None),
        Dcfr::default().at(16, None),
        HsDcfr { gamma0: 30.0 }.at(3, Some(16)),
        linear_cfr().at(7, None),
        Discounts {
            pos: 0.987654321,
            neg: 0.234567891,
            avg: 0.876543219,
            floor_neg: true,
            reset_avg: true,
        },
    ]
}

fn matrix<S: Storage>(split: bool) {
    for actions in [0, 1, 3] {
        for hands in [0, 1, 3, 5] {
            let r = StorageRef {
                offset: 3,
                num_actions: actions,
                num_hands: hands,
                index: 1,
            };
            let len = r.offset + r.len() + 4;
            let mut old = seeded::<S>(len);
            let mut fused = S::new(len, 3);
            fused.restore_state(old.state()).unwrap();
            let initial = state_bits(old.state());
            for d in discounts() {
                let x = inputs(r);
                apply(&mut old, r, &mut x.clone(), &d, split, false);
                apply(&mut fused, r, &mut x.clone(), &d, split, true);
                let actual = state_bits(fused.state());
                assert_eq!(
                    state_bits(old.state()),
                    actual,
                    "A={actions} H={hands} split={split} d={d:?}"
                );
                assert_sentinels(&initial, &actual, r);
            }
        }
    }
}

#[test]
fn f32_full_matches_original_operations() {
    matrix::<F32Storage>(false);
}

#[test]
fn f32_rebased_view_matches_original_operations() {
    matrix::<F32Storage>(true);
}

#[test]
fn i16_full_preserves_raw_values_and_scale_bits() {
    matrix::<I16Storage>(false);
}

#[test]
fn i16_rebased_view_preserves_raw_values_and_scale_bits() {
    matrix::<I16Storage>(true);
}

#[test]
fn f32_reset_overwrites_nonfinite_old_sum_and_preserves_weighted_signed_zero() {
    // A robustness case for the reset branch, not a legal-solver/NaN-payload
    // claim: no arithmetic on the old nonfinite sum is required by either path.
    for split in [false, true] {
        let r = StorageRef {
            offset: 3,
            num_actions: 1,
            num_hands: 3,
            index: 1,
        };
        let len = r.offset + r.len() + 4;
        let mut old = seeded::<F32Storage>(len);
        let StorageState::F32 {
            regrets,
            mut strategy_sum,
        } = old.state()
        else {
            unreachable!()
        };
        strategy_sum[r.offset..r.offset + r.len()].copy_from_slice(&[
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ]);
        old.restore_state(StorageState::F32 {
            regrets,
            strategy_sum,
        })
        .unwrap();
        let mut fused = F32Storage::new(len, 3);
        fused.restore_state(old.state()).unwrap();
        let mut x = inputs(r);
        x.reach = vec![-0.0, 0.0, f32::from_bits(1)];
        let d = Discounts {
            pos: 1.0,
            neg: 1.0,
            avg: 0.7,
            floor_neg: false,
            reset_avg: true,
        };
        apply(&mut old, r, &mut x.clone(), &d, split, false);
        apply(&mut fused, r, &mut x, &d, split, true);
        let actual = state_bits(fused.state());
        assert_eq!(state_bits(old.state()), actual);
        assert_eq!(
            &actual[1][r.offset..r.offset + r.len()],
            &[(-0.0_f32).to_bits(), 0, 1]
        );
    }
}
