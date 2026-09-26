use super::*;

fn set_bits(indices: &[usize]) -> Mass {
    let mut limbs = [0; 5];
    for &bit in indices {
        assert!(bit < 320);
        limbs[bit / 64] |= 1u64 << (bit % 64);
    }
    Mass(limbs)
}

fn shifted_u128(value: u128, shift: usize) -> Mass {
    let mut bits = Vec::new();
    for bit in 0..128 {
        if value & (1u128 << bit) != 0 {
            bits.push(bit + shift);
        }
    }
    set_bits(&bits)
}

fn bit_vector(value: Mass) -> [bool; 320] {
    std::array::from_fn(|bit| value.0[bit / 64] & (1u64 << (bit % 64)) != 0)
}

// An independent, one-bit-at-a-time integer sum; no limb arithmetic.
fn add_bits(left: [bool; 320], right: [bool; 320]) -> [bool; 320] {
    let mut result = [false; 320];
    let mut carry = 0u8;
    for bit in 0..320 {
        let sum = u8::from(left[bit]) + u8::from(right[bit]) + carry;
        result[bit] = !sum.is_multiple_of(2);
        carry = sum / 2;
    }
    assert_eq!(carry, 0);
    result
}

// Decimal conversion supplies an oracle independent of the production
// binary64 guard/sticky and limb extraction code. Integer * 2^-149 equals
// integer * 5^149 * 10^-149; Rust's decimal parser rounds that exact value.
fn decimal_reference(value: Mass) -> f64 {
    fn multiply_add(digits: &mut Vec<u8>, factor: u8, mut carry: u8) {
        for digit in digits.iter_mut() {
            let next = *digit * factor + carry;
            *digit = next % 10;
            carry = next / 10;
        }
        while carry != 0 {
            digits.push(carry % 10);
            carry /= 10;
        }
    }
    let mut digits = vec![0];
    for bit in bit_vector(value).into_iter().rev() {
        multiply_add(&mut digits, 2, u8::from(bit));
    }
    for _ in 0..149 {
        multiply_add(&mut digits, 5, 0);
    }
    let mut decimal: String = digits.iter().rev().map(|d| char::from(b'0' + d)).collect();
    decimal.push_str("e-149");
    decimal.parse().unwrap()
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn mass_decodes_every_f32_exponent_and_subnormal_boundary() {
    assert_eq!(Mass::default(), Mass::ZERO);
    assert_eq!(Mass::from_f32(-0.0), Mass::ZERO);
    assert_eq!(Mass::ZERO.to_f64().to_bits(), 0);
    for exponent in 0..255u32 {
        for fraction in [0, 1, 2, 0x3f_ffff, 0x40_0000, 0x7f_fffe, 0x7f_ffff] {
            let value = f32::from_bits((exponent << 23) | fraction);
            let mass = Mass::from_f32(value);
            assert_eq!(mass.to_f64().to_bits(), f64::from(value).to_bits());
            if value != 0.0 {
                // Scale the exact f64 value back by 2^149, within each
                // binary32 component's 24-bit integer window.
                let shift = exponent.saturating_sub(1) as usize;
                let expected = (f64::from(value) * 2.0f64.powi(149 - shift as i32)) as u128;
                assert_eq!(mass, shifted_u128(expected, shift));
            }
        }
    }
    assert_eq!(Mass::from_f32(f32::from_bits(1)), set_bits(&[0]));
    assert_eq!(Mass::from_f32(f32::MIN_POSITIVE), set_bits(&[23]));
}

#[test]
fn mass_add_sub_cross_every_limb_boundary() {
    let one = set_bits(&[0]);
    for boundary in [64, 128, 192, 256] {
        let power = set_bits(&[boundary]);
        let all_lower = set_bits(&(0..boundary).collect::<Vec<_>>());
        assert_eq!(all_lower.add(one), power);
        assert_eq!(power.sub(one), all_lower);
        assert_eq!(power.sub(all_lower), one);
        assert_eq!(power.sub(power), Mass::ZERO);
    }
    // Both a limb addition and its incoming carry can overflow on
    // different limbs; the borrow chain has the analogous boundary.
    let left = Mass([u64::MAX, 0, u64::MAX, 0, 0]);
    let right = Mass([1, u64::MAX, 0, u64::MAX, 0]);
    let sum = left.add(right);
    assert_eq!(
        bit_vector(sum),
        add_bits(bit_vector(left), bit_vector(right))
    );
    assert_eq!(sum.sub(left), right);
    assert_eq!(sum.sub(right), left);
}

#[test]
fn mass_random_limb_arithmetic_matches_bit_vector_reference() {
    let mut seed = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..256 {
        let mut left = std::array::from_fn(|_| next_random(&mut seed));
        let mut right = std::array::from_fn(|_| next_random(&mut seed));
        left[4] >>= 1;
        right[4] >>= 1;
        let (left, right) = (Mass(left), Mass(right));
        let sum = left.add(right);
        assert_eq!(
            bit_vector(sum),
            add_bits(bit_vector(left), bit_vector(right))
        );
        assert_eq!(sum.sub(left), right);
        assert_eq!(sum.sub(right), left);
    }
}

#[test]
fn mass_to_f64_matches_independent_u128_conversion_at_all_limb_offsets() {
    let mut seed = 0xd1b5_4a32_d192_ed03;
    for shift in 0..=192 {
        for value in [
            0,
            1,
            (1u128 << 53) - 1,
            (1u128 << 53) + 1,
            u128::MAX,
            (u128::from(next_random(&mut seed)) << 64) | u128::from(next_random(&mut seed)),
        ] {
            let actual = shifted_u128(value, shift).to_f64();
            let expected = (value as f64) * 2.0f64.powi(shift as i32 - 149);
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "shift={shift}, value={value}"
            );
        }
    }
}

#[test]
fn mass_to_f64_rounds_ties_even_sticky_and_significand_carry() {
    for high in 53..320usize {
        let guard = high - 53;
        let base = 2.0f64.powi(high as i32 - 149);
        let half_even = set_bits(&[high, guard]);
        assert_eq!(half_even.to_f64().to_bits(), base.to_bits());
        let half_odd = set_bits(&[high, high - 52, guard]);
        assert_eq!(half_odd.to_f64().to_bits(), base.to_bits() + 2);
        if guard > 0 {
            assert_eq!(
                set_bits(&[high, guard, 0]).to_f64().to_bits(),
                base.to_bits() + 1
            );
            assert_eq!(
                set_bits(&[high, guard - 1]).to_f64().to_bits(),
                base.to_bits()
            );
        }
        let mut carry_bits: Vec<_> = ((high - 52)..=high).collect();
        carry_bits.push(guard);
        assert_eq!(
            set_bits(&carry_bits).to_f64().to_bits(),
            2.0f64.powi(high as i32 + 1 - 149).to_bits()
        );
    }
}

#[test]
fn mass_to_f64_matches_exact_decimal_reference_for_full_320_bit_values() {
    let mut seed = 0x94d0_49bb_1331_11eb;
    for value in [Mass::ZERO, set_bits(&[0]), Mass([u64::MAX; 5])] {
        assert_eq!(value.to_f64().to_bits(), decimal_reference(value).to_bits());
    }
    for _ in 0..128 {
        let value = Mass(std::array::from_fn(|_| next_random(&mut seed)));
        assert_eq!(value.to_f64().to_bits(), decimal_reference(value).to_bits());
    }
}

#[test]
fn mass_holds_maximum_hand_sum_and_self_addback_without_losing_tiny_mass() {
    let largest = Mass::from_f32(f32::MAX);
    let tiny = Mass::from_f32(f32::from_bits(1));
    let mut sum = Mass::ZERO;
    let mut reference = [false; 320];
    for _ in 0..=MAX_HANDS {
        sum = sum.add(largest);
        reference = add_bits(reference, bit_vector(largest));
    }
    assert_eq!(bit_vector(sum), reference);
    assert!(sum.0[4] < (1 << 32));
    assert_eq!(
        sum.to_f64().to_bits(),
        (f64::from(f32::MAX) * 1327.0).to_bits()
    );
    assert_eq!(sum.to_f64().to_bits(), decimal_reference(sum).to_bits());
    assert_eq!(sum.add(tiny).sub(sum), tiny);
    assert_eq!(
        largest.add(tiny).sub(largest).to_f64(),
        f64::from(f32::from_bits(1))
    );
    assert_eq!(
        (f64::from(f32::MAX) + f64::from(f32::from_bits(1))) - f64::from(f32::MAX),
        0.0
    );
}

#[test]
fn mass_analysis_preserves_gate_width_and_scale_in_eight_bytes() {
    assert_eq!(std::mem::size_of::<MassAnalysis>(), 8);
    // Expected bounds are fixed examples, including a nonzero common quantum
    // in every width. They do not derive expectations through either wrapper.
    let cases = [
        (vec![], 0, 0, true, MassWidth::U64),
        (vec![0.0, -0.0], 0, 0, true, MassWidth::U64),
        (vec![0.0; MAX_HANDS], 0, 0, true, MassWidth::U64),
        (
            vec![f32::from_bits(1), f32::MIN_POSITIVE],
            26,
            0,
            true,
            MassWidth::U64,
        ),
        (vec![f32::MAX], 25, 253, true, MassWidth::U64),
        (vec![f32::MAX; MAX_HANDS], 35, 253, true, MassWidth::U64),
        (vec![1.0, 2.0_f32.powi(-30)], 56, 96, false, MassWidth::U64),
        (vec![1.0, 2.0_f32.powi(-70)], 96, 56, false, MassWidth::U128),
        (
            vec![f32::from_bits(1), f32::MAX],
            279,
            0,
            false,
            MassWidth::Wide,
        ),
        (
            vec![value_at_mass_shift(100, 1), value_at_mass_shift(240, 3)],
            166,
            100,
            false,
            MassWidth::Wide,
        ),
    ];
    for (reach, bound, min_shift, exact, width) in cases {
        let analysis = analyze_mass(&reach);
        assert_eq!((analysis.bound, analysis.min_shift), (bound, min_shift));
        assert_eq!(analysis.is_f64_exact(), exact);
        let (actual_width, scale) = analysis.integer();
        assert_eq!(actual_width, width);
        assert_eq!(scale.min_shift, min_shift);
        assert_eq!(
            scale.factor.to_bits(),
            2.0_f64.powi(min_shift as i32 - 149).to_bits()
        );
        if bound == 0 {
            assert_eq!(scale.min_shift, MassScale::WIDE.min_shift);
            assert_eq!(scale.factor.to_bits(), MassScale::WIDE.factor.to_bits());
        }
        assert_eq!(f64_mass_is_exact(&reach), exact);
        let (wrapper_width, wrapper_scale) = classify_integer_mass(&reach);
        assert_eq!(wrapper_width, width);
        assert_eq!(wrapper_scale.min_shift, scale.min_shift);
        assert_eq!(wrapper_scale.factor.to_bits(), scale.factor.to_bits());
    }
}

// Candidate03's branchy classification is kept only as a differential oracle
// for the raw-bit extrema scan. It neither calls nor shares helpers with it.
fn branchy_mass_reference(reach: &[f32]) -> (u32, u32, bool, MassWidth, u64) {
    assert!(reach.len() <= MAX_HANDS, "too many reach terms");
    let mut count = 0usize;
    let mut minimum = u32::MAX;
    let mut maximum = 0u32;
    for &value in reach {
        assert!(value.is_finite() && value >= 0.0, "invalid reach mass");
        if value == 0.0 {
            continue;
        }
        count += 1;
        let exponent = (value.to_bits() >> 23) & 0xff;
        let shift = exponent.saturating_sub(1);
        minimum = minimum.min(shift);
        maximum = maximum.max(shift);
    }
    if count == 0 {
        return (0, 0, true, MassWidth::U64, 874u64 << 52);
    }
    let count_bits = usize::BITS - count.leading_zeros();
    let bound = maximum - minimum + 24 + count_bits;
    let width = if bound <= 64 {
        MassWidth::U64
    } else if bound <= 128 {
        MassWidth::U128
    } else {
        MassWidth::Wide
    };
    (
        bound,
        minimum,
        bound <= 53,
        width,
        u64::from(minimum + 874) << 52,
    )
}

#[test]
fn mass_analysis_raw_bits_matches_branchy_reference() {
    fn check(reach: &[f32]) {
        let expected = branchy_mass_reference(reach);
        let analysis = analyze_mass(reach);
        let (width, scale) = analysis.integer();
        assert_eq!(
            (
                analysis.bound,
                analysis.min_shift,
                analysis.is_f64_exact(),
                width,
                scale.factor.to_bits()
            ),
            expected,
            "reach bits={:?}",
            reach
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
    }
    for exponent in 0..255u32 {
        for fraction in [0, 1, 0x3f_ffff, 0x40_0000, 0x7f_ffff] {
            let value = f32::from_bits((exponent << 23) | fraction);
            check(&[value]);
            check(&[0.0, value, -0.0, f32::from_bits(1)]);
            check(&[f32::MAX, -0.0, value, 0.0]);
        }
    }
    let mut seed = 0x510e_527f_ade6_82d1;
    for count in [0, 1, 2, 3, 4, 127, 128, 255, 256, 1023, 1024, MAX_HANDS] {
        for _ in 0..32 {
            let minimum = (next_random(&mut seed) % 254) as u32;
            let span = (next_random(&mut seed) % u64::from(254 - minimum)) as u32;
            let reach: Vec<_> = (0..count)
                .map(|index| {
                    if index % 17 == 0 {
                        return if index % 2 == 0 { 0.0 } else { -0.0 };
                    }
                    let shift = minimum + (next_random(&mut seed) % u64::from(span + 1)) as u32;
                    value_at_mass_shift(shift, next_random(&mut seed) as u32)
                })
                .collect();
            check(&reach);
        }
    }
    for invalid in [
        -1.0,
        -f32::from_bits(1),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::from_bits(0x7f80_0001),
        f32::NAN,
        f32::from_bits(0xff80_0001),
    ] {
        for position in 0..4 {
            let mut reach = [f32::from_bits(1), f32::MAX, -0.0, 1.0];
            reach[position] = invalid;
            assert!(std::panic::catch_unwind(|| branchy_mass_reference(&reach)).is_err());
            assert!(std::panic::catch_unwind(|| analyze_mass(&reach)).is_err());
        }
    }
}

#[test]
fn mass_gate_covers_zero_subnormal_and_term_count_boundaries() {
    assert!(f64_mass_is_exact(&[]));
    assert!(f64_mass_is_exact(&[0.0, -0.0]));
    assert!(f64_mass_is_exact(&vec![0.0; MAX_HANDS]));
    assert!(f64_mass_is_exact(&[f32::MAX]));
    assert!(f64_mass_is_exact(&[f32::from_bits(1), f32::MIN_POSITIVE]));
    assert!(!f64_mass_is_exact(&[f32::from_bits(1), f32::MAX]));
    // Explicit table avoids testing bit_length with another copy of itself.
    for (count, bits) in [
        (2, 2),
        (3, 2),
        (4, 3),
        (127, 7),
        (128, 8),
        (255, 8),
        (256, 9),
        (1023, 10),
        (1024, 11),
        (1326, 11),
    ] {
        for boundary in [52u32, 53, 54] {
            let difference = boundary - 24 - bits;
            let low = f32::from_bits(1 << 23); // shift=0, also tests subnormal/normal boundary.
            let high = f32::from_bits((difference + 1) << 23);
            let mut reach = vec![low; count];
            reach[count - 1] = high;
            assert_eq!(
                f64_mass_is_exact(&reach),
                boundary <= 53,
                "N={count}, B={boundary}"
            );
            if count < MAX_HANDS {
                reach.push(-0.0);
                assert_eq!(f64_mass_is_exact(&reach), boundary <= 53);
            }
        }
    }
}

#[test]
fn mass_gate_safe_sums_and_cancelling_card_expression_match_exact_mass() {
    // The positive overlap is deliberately tiny relative to the largest
    // weight while remaining inside the sufficient f64 bound.
    let reach = [
        f32::from_bits(1),
        f32::from_bits(23 << 23),
        0.0,
        f32::from_bits((22 << 23) | 0x7f_ffff),
    ];
    assert!(f64_mass_is_exact(&reach));
    let mut exact = Mass::ZERO;
    let mut floating = 0.0;
    for &value in &reach {
        exact = exact.add(Mass::from_f32(value));
        floating += f64::from(value);
        assert_eq!(floating.to_bits(), exact.to_f64().to_bits());
    }
    // Both blocked-card totals include the shared combo. The old left-
    // associative expression may be negative before adding self back.
    let overlap = f64::from(reach[0]);
    let cards_a = floating;
    let cards_b = overlap;
    assert_eq!(
        ((floating - cards_a) - cards_b).to_bits(),
        (-overlap).to_bits()
    );
    let compatible = ((floating - cards_a) - cards_b) + overlap;
    assert_eq!(compatible, 0.0);
    let exact_compatible = exact
        .add(Mass::from_f32(reach[0]))
        .sub(exact)
        .sub(Mass::from_f32(reach[0]));
    assert_eq!(compatible.to_bits(), exact_compatible.to_f64().to_bits());
}

#[test]
fn mass_rejects_invalid_inputs_even_after_an_unsafe_gate_prefix() {
    for value in [
        -1.0,
        -f32::from_bits(1),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ] {
        assert!(std::panic::catch_unwind(|| Mass::from_f32(value)).is_err());
        assert!(std::panic::catch_unwind(|| analyze_mass(&[value])).is_err());
        assert!(
            std::panic::catch_unwind(|| analyze_mass(&[f32::from_bits(1), f32::MAX, value]))
                .is_err()
        );
        assert!(std::panic::catch_unwind(|| f64_mass_is_exact(&[value])).is_err());
        assert!(
            std::panic::catch_unwind(|| f64_mass_is_exact(&[f32::from_bits(1), f32::MAX, value]))
                .is_err()
        );
    }
    assert!(std::panic::catch_unwind(|| f64_mass_is_exact(&vec![0.0; MAX_HANDS + 1])).is_err());
    assert!(std::panic::catch_unwind(|| analyze_mass(&vec![0.0; MAX_HANDS + 1])).is_err());
}

#[test]
#[should_panic(expected = "mass addition overflow")]
fn mass_rejects_addition_overflow() {
    Mass([u64::MAX; 5]).add(set_bits(&[0]));
}

#[test]
#[should_panic(expected = "mass subtraction underflow")]
fn mass_rejects_negative_subtraction() {
    Mass::ZERO.sub(set_bits(&[0]));
}

fn value_at_mass_shift(shift: u32, fraction: u32) -> f32 {
    assert!(shift <= 253);
    f32::from_bits(((shift + 1) << 23) | (fraction & 0x7f_ffff))
}

fn check_scaled_cancellation<M: ExactMass>(
    reach: &[f32],
    scale: MassScale,
    word: impl Fn(M) -> u128,
) {
    let minimum = reach
        .iter()
        .filter(|&&v| v != 0.0)
        .map(|v| (((v.to_bits() >> 23) & 0xff).saturating_sub(1)) as usize)
        .min()
        .unwrap();
    let masses: Vec<_> = reach
        .iter()
        .map(|&v| M::from_f32_scaled(v, scale))
        .collect();
    let mut total = M::ZERO;
    let mut card_a = M::ZERO;
    let mut card_b = M::ZERO;
    let mut expected_total = [false; 320];
    let mut expected_compatible = [false; 320];
    for (i, (&value, &mass)) in reach.iter().zip(&masses).enumerate() {
        let exact = bit_vector(Mass::from_f32(value));
        expected_total = add_bits(expected_total, exact);
        total = total.add(mass);
        // Index 0 is the unique self hand. Other hands share at most one
        // queried card; every third hand is compatible with both cards.
        if i == 0 || i % 3 == 1 {
            card_a = card_a.add(mass);
        }
        if i == 0 || i % 3 == 2 {
            card_b = card_b.add(mass);
        }
        if i != 0 && i % 3 == 0 {
            expected_compatible = add_bits(expected_compatible, exact);
        }
    }
    let unscaled_total = shifted_u128(word(total), minimum);
    assert_eq!(bit_vector(unscaled_total), expected_total);
    assert_eq!(
        total.to_f64_scaled(scale).to_bits(),
        decimal_reference(unscaled_total).to_bits()
    );
    let compatible = total.add(masses[0]).sub(card_a).sub(card_b);
    let unscaled_compatible = shifted_u128(word(compatible), minimum);
    assert_eq!(bit_vector(unscaled_compatible), expected_compatible);
    assert_eq!(
        compatible.to_f64_scaled(scale).to_bits(),
        decimal_reference(unscaled_compatible).to_bits()
    );
    // A rounded subtotal could hide low bits; raw unscaling above and
    // removing every original term below check the integer state itself.
    for &mass in masses.iter().rev() {
        total = total.sub(mass);
    }
    assert_eq!(word(total), 0);
}

#[test]
fn integer_mass_classifier_preserves_f64_gate_and_width_count_boundaries() {
    for reach in [vec![], vec![0.0, -0.0], vec![0.0; MAX_HANDS]] {
        assert_eq!(classify_integer_mass(&reach).0, MassWidth::U64);
    }
    for (count, count_bits) in [
        (2, 2),
        (3, 2),
        (4, 3),
        (127, 7),
        (128, 8),
        (1023, 10),
        (1024, 11),
        (1326, 11),
    ] {
        for bound in [53, 54, 64, 65, 128, 129] {
            let difference = bound - 24 - count_bits;
            let mut reach = vec![f32::MIN_POSITIVE; count];
            reach[count - 1] = value_at_mass_shift(difference, 0x7f_ffff);
            let expected = match bound {
                0..=64 => MassWidth::U64,
                65..=128 => MassWidth::U128,
                _ => MassWidth::Wide,
            };
            assert_eq!(
                classify_integer_mass(&reach).0,
                expected,
                "N={count}, B={bound}"
            );
            assert_eq!(f64_mass_is_exact(&reach), bound <= 53);
            if bound == 64 {
                check_scaled_cancellation::<u64>(
                    &reach,
                    classify_integer_mass(&reach).1,
                    u128::from,
                );
            } else if bound == 128 {
                check_scaled_cancellation::<u128>(&reach, classify_integer_mass(&reach).1, |v| v);
            }
            if count < MAX_HANDS {
                reach.push(-0.0);
                assert_eq!(classify_integer_mass(&reach).0, expected);
            }
        }
    }
}

#[test]
fn scaled_integer_mass_decodes_all_f32_exponents_and_subnormal_edges() {
    for exponent in 0..255 {
        for fraction in [0, 1, 2, 0x3f_ffff, 0x40_0000, 0x7f_ffff] {
            let value = f32::from_bits((exponent << 23) | fraction);
            let (width, scale) = classify_integer_mass(&[value]);
            assert_eq!(width, MassWidth::U64);
            let expected = f64::from(value).to_bits();
            assert_eq!(
                u64::from_f32_scaled(value, scale)
                    .to_f64_scaled(scale)
                    .to_bits(),
                expected
            );
            assert_eq!(
                u128::from_f32_scaled(value, scale)
                    .to_f64_scaled(scale)
                    .to_bits(),
                expected
            );
            assert_eq!(Mass::from_f32_scaled(value, scale), Mass::from_f32(value));
            assert_eq!(
                Mass::from_f32_scaled(value, scale)
                    .to_f64_scaled(scale)
                    .to_bits(),
                expected
            );
        }
    }
    let (_, high_scale) = classify_integer_mass(&[f32::MAX]);
    for zero in [0.0, -0.0] {
        assert_eq!(u64::from_f32_scaled(zero, high_scale), 0);
        assert_eq!(u128::from_f32_scaled(zero, high_scale), 0);
        assert_eq!(Mass::from_f32_scaled(zero, high_scale), Mass::ZERO);
    }
}

#[test]
fn scaled_integer_random_card_removal_matches_independent_bit_sums() {
    let mut seed = 0x7a32_819c_bca5_061d;
    for (difference, expected) in [(35, MassWidth::U64), (99, MassWidth::U128)] {
        for _ in 0..24 {
            let minimum = (next_random(&mut seed) % u64::from(254 - difference)) as u32;
            let mut reach: Vec<_> = (0..17)
                .map(|_| {
                    let shift =
                        minimum + (next_random(&mut seed) % u64::from(difference + 1)) as u32;
                    value_at_mass_shift(shift, next_random(&mut seed) as u32)
                })
                .collect();
            reach[0] = value_at_mass_shift(minimum, 1);
            reach[16] = value_at_mass_shift(minimum + difference, 0x7f_ffff);
            let (width, scale) = classify_integer_mass(&reach);
            assert_eq!(width, expected);
            assert!(!f64_mass_is_exact(&reach));
            match width {
                MassWidth::U64 => check_scaled_cancellation::<u64>(&reach, scale, u128::from),
                MassWidth::U128 => check_scaled_cancellation::<u128>(&reach, scale, |v| v),
                MassWidth::Wide => unreachable!(),
            }
        }
    }
    let reach = [f32::from_bits(1), f32::MAX];
    let (width, scale) = classify_integer_mass(&reach);
    assert_eq!(width, MassWidth::Wide);
    let tiny = Mass::from_f32_scaled(reach[0], scale);
    let large = Mass::from_f32_scaled(reach[1], scale);
    assert_eq!(large.add(tiny).sub(large), Mass::from_f32(reach[0]));
    assert_eq!(
        tiny.to_f64_scaled(scale).to_bits(),
        f64::from(reach[0]).to_bits()
    );
}

#[test]
fn scaled_integer_conversion_rounds_once_even_odd_sticky_and_carry() {
    for shift in [0, 1, 63, 126, 192] {
        let (_, scale) = classify_integer_mass(&[value_at_mass_shift(shift, 0)]);
        for high in [53, 54, 63, 64, 65, 95, 126, 127] {
            let base = 1u128 << high;
            let guard = 1u128 << (high - 53);
            let carry = (((1u128 << 53) - 1) << (high - 52)) | guard;
            for value in [
                base | guard,
                base | (1u128 << (high - 52)) | guard,
                base + guard - 1,
                base + guard + 1,
                carry,
            ] {
                let expected = decimal_reference(shifted_u128(value, shift as usize)).to_bits();
                assert_eq!(
                    value.to_f64_scaled(scale).to_bits(),
                    expected,
                    "high={high}, shift={shift}"
                );
                if let Ok(narrow) = u64::try_from(value) {
                    assert_eq!(narrow.to_f64_scaled(scale).to_bits(), expected);
                }
            }
        }
    }
}

#[test]
fn scaled_integer_operations_reject_invalid_inputs_and_overflow() {
    let (_, zero_shift) = classify_integer_mass(&[f32::MIN_POSITIVE]);
    let (_, high_shift) = classify_integer_mass(&[value_at_mass_shift(100, 0)]);
    for value in [
        -1.0,
        -f32::from_bits(1),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ] {
        assert!(std::panic::catch_unwind(|| classify_integer_mass(&[value])).is_err());
        assert!(
            std::panic::catch_unwind(|| classify_integer_mass(&[
                f32::from_bits(1),
                f32::MAX,
                value
            ]))
            .is_err()
        );
        assert!(std::panic::catch_unwind(|| u64::from_f32_scaled(value, zero_shift)).is_err());
        assert!(std::panic::catch_unwind(|| u128::from_f32_scaled(value, zero_shift)).is_err());
        assert!(std::panic::catch_unwind(|| Mass::from_f32_scaled(value, zero_shift)).is_err());
    }
    assert!(std::panic::catch_unwind(|| classify_integer_mass(&vec![0.0; MAX_HANDS + 1])).is_err());
    assert!(
        std::panic::catch_unwind(|| u64::from_f32_scaled(f32::MIN_POSITIVE, high_shift)).is_err()
    );
    assert!(
        std::panic::catch_unwind(|| u128::from_f32_scaled(f32::MIN_POSITIVE, high_shift)).is_err()
    );
    // checked_shl alone is insufficient: these shift amounts are in range,
    // but the 24-bit significand would lose its highest bit.
    assert!(
        std::panic::catch_unwind(|| u64::from_f32_scaled(value_at_mass_shift(41, 0), zero_shift))
            .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| u128::from_f32_scaled(value_at_mass_shift(105, 0), zero_shift))
            .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| u64::from_f32_scaled(value_at_mass_shift(64, 0), zero_shift))
            .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| u128::from_f32_scaled(value_at_mass_shift(128, 0), zero_shift))
            .is_err()
    );
    assert!(std::panic::catch_unwind(|| <u64 as ExactMass>::add(u64::MAX, 1)).is_err());
    assert!(std::panic::catch_unwind(|| <u128 as ExactMass>::add(u128::MAX, 1)).is_err());
    assert!(std::panic::catch_unwind(|| <u64 as ExactMass>::sub(0, 1)).is_err());
    assert!(std::panic::catch_unwind(|| <u128 as ExactMass>::sub(0, 1)).is_err());
}
