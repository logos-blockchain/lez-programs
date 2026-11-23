//! Price math: spot price → tick, and tick → the `Q64.64` price consumers read.

/// Converts a `Q64.64` fixed-point spot price into the nearest TWAP tick.
///
/// Price sources report a spot price (e.g. an AMM's `reserve_b / reserve_a` as a `Q64.64`
/// ratio); the oracle owns the conversion to its internal tick representation so producers never
/// need to know about ticks. The price is mapped through the Uniswap `sqrtPriceX96`
/// representation:
///
/// ```text
/// price        = ratio * 2^64                 (Q64.64 input)
/// sqrtPriceX96 = sqrt(ratio) * 2^96 = isqrt(price << 128)
/// tick         = get_tick_at_sqrt_ratio(sqrtPriceX96)
/// ```
///
/// `isqrt` is a pure-integer square root (no floating point — deterministic in the zkVM). The
/// `sqrtPriceX96` is clamped to at least `MIN_SQRT_RATIO` so a zero or dust price maps to
/// `MIN_TICK` rather than erroring; the upper bound cannot be reached from a `u128` price (its
/// max ratio `~2^64` corresponds to a tick well inside `MAX_TICK`).
#[must_use]
pub fn price_to_tick(price: u128) -> i32 {
    use alloy_primitives::U256;
    use uniswap_v3_math::tick_math::{get_tick_at_sqrt_ratio, MIN_SQRT_RATIO};

    // sqrtPriceX96 = sqrt(price / 2^64) * 2^96 = sqrt(price) * 2^64 = isqrt(price << 128).
    // price < 2^128, so price << 128 < 2^256 and the shift never overflows U256.
    let shifted = U256::from(price)
        .checked_shl(128)
        .expect("price < 2^128, so price << 128 fits in U256");
    let sqrt_price_x96 = integer_sqrt(shifted).max(MIN_SQRT_RATIO);

    get_tick_at_sqrt_ratio(sqrt_price_x96)
        .expect("sqrt_price_x96 is clamped into [MIN_SQRT_RATIO, MAX_SQRT_RATIO)")
}

/// Floor of the integer square root of a 256-bit value, computed bit-by-bit with no floating
/// point (`ruint`'s own `root` is gated behind its `std` feature and uses an `f64` seed, neither
/// of which is available in the guest). Deterministic — suitable for the zkVM.
fn integer_sqrt(n: alloy_primitives::U256) -> alloy_primitives::U256 {
    use alloy_primitives::U256;

    if n.is_zero() {
        return U256::ZERO;
    }

    // Largest power of four not exceeding `n`: `1 << (msb rounded down to an even bit index)`.
    let msb = 255usize
        .checked_sub(n.leading_zeros())
        .expect("n is non-zero, so leading_zeros <= 255");
    let start = msb & !1usize;
    let mut d = U256::ONE.checked_shl(start).expect("start < 256");
    let mut c = U256::ZERO;
    let mut x = n;

    // `c` and `d` are shifted with `wrapping_shr`, which logically drops the low bits (the
    // algorithm intends to). `checked_shr` would return `None` whenever a set bit falls off
    // (e.g. `d = 1 >> 2`), which is not what we want here.
    while !d.is_zero() {
        let cd = c
            .checked_add(d)
            .expect("c + d stays below the root, no overflow");
        if x >= cd {
            x = x.checked_sub(cd).expect("guarded by x >= cd");
            c = c
                .wrapping_shr(1)
                .checked_add(d)
                .expect("c/2 + d stays below the root, no overflow");
        } else {
            c = c.wrapping_shr(1);
        }
        d = d.wrapping_shr(2);
    }

    c
}

/// Number of fractional bits in the fixed-point
/// [`OraclePrice::price`](crate::OraclePrice::price).
///
/// The price is stored as a `Q64.64` ratio: `OraclePrice::price / 2^PRICE_FRACTIONAL_BITS`
/// is the amount of `quote_asset` one unit of `base_asset` is worth. A consumer multiplies a
/// token amount by the price with `(amount * price) >> PRICE_FRACTIONAL_BITS`.
pub const PRICE_FRACTIONAL_BITS: u32 = 64;

/// Converts a TWAP tick into the `Q64.64` fixed-point price stored in
/// [`OraclePrice::price`](crate::OraclePrice::price).
///
/// The price is `1.0001^tick`, computed via the Uniswap v3 `sqrtPriceX96` representation
/// (pure-integer, no floating point) and then squared back to a plain ratio:
///
/// ```text
/// sqrtPriceX96 = sqrt(1.0001^tick) * 2^96
/// price        = sqrtPriceX96^2 / 2^128 = 1.0001^tick * 2^64   (Q64.64)
/// ```
///
/// `sqrtPriceX96^2` is computed with `full_math::mul_div` using a 512-bit intermediate, so it
/// never overflows for any valid tick. The tick is clamped to `[MIN_TICK, MAX_TICK]` and the
/// result saturates at `u128::MAX` for the (practically unreachable) ticks above ~443 636 whose
/// ratio would exceed `2^64`.
#[must_use]
pub fn tick_to_oracle_price(tick: i32) -> u128 {
    use alloy_primitives::U256;
    use uniswap_v3_math::tick_math::{MAX_TICK, MIN_TICK};

    // 2^128, used to bring sqrtPriceX96^2 (a Q128.128 square) down to Q64.64.
    // Built from limbs (little-endian u64 words) to avoid arithmetic operators on U256.
    const TWO_POW_128: U256 = U256::from_limbs([0, 0, 1, 0]);

    let clamped_tick = tick.clamp(MIN_TICK, MAX_TICK);
    let sqrt_price_x96 = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(clamped_tick)
        .expect("clamped tick is within [MIN_TICK, MAX_TICK]");
    let price_q64_64 =
        uniswap_v3_math::full_math::mul_div(sqrt_price_x96, sqrt_price_x96, TWO_POW_128)
            .expect("1.0001^tick * 2^64 fits in U256 for any valid tick");

    u128::try_from(price_q64_64).unwrap_or(u128::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `1.0` in Q64.64 is `2^64`; its square root is `2^96 = sqrtPriceX96` at tick 0.
    const ONE_Q64_64: u128 = 1u128 << 64;

    // ── price_to_tick ───────────────────────────────────────────────────────────

    #[test]
    fn unit_price_maps_to_tick_zero() {
        assert_eq!(price_to_tick(ONE_Q64_64), 0);
    }

    #[test]
    fn zero_price_maps_to_min_tick() {
        // A zero/dust price clamps to MIN_SQRT_RATIO → MIN_TICK, never panics.
        assert_eq!(price_to_tick(0), uniswap_v3_math::tick_math::MIN_TICK);
    }

    #[test]
    fn prices_above_one_are_positive_below_one_negative() {
        assert!(
            price_to_tick(ONE_Q64_64 << 1) > 0,
            "2.0 should be a positive tick"
        );
        assert!(
            price_to_tick(ONE_Q64_64 >> 1) < 0,
            "0.5 should be a negative tick"
        );
    }

    #[test]
    fn price_to_tick_is_monotonic_in_price() {
        // Strictly increasing Q64.64 prices spanning well below and above 1.0 must produce
        // non-decreasing ticks. Built without the forward conversion, so this is self-contained.
        let prices = [
            1u128,
            ONE_Q64_64 >> 40,
            ONE_Q64_64 >> 20,
            ONE_Q64_64 >> 10,
            ONE_Q64_64 >> 1,
            ONE_Q64_64,
            ONE_Q64_64 << 1,
            ONE_Q64_64 << 10,
            ONE_Q64_64 << 20,
            ONE_Q64_64 << 40,
            u128::MAX,
        ];
        let mut prev = price_to_tick(prices[0]);
        for &price in &prices[1..] {
            let cur = price_to_tick(price);
            assert!(cur >= prev, "tick must not decrease as price increases");
            prev = cur;
        }
    }

    #[test]
    fn max_price_maps_near_saturation_tick_without_panicking() {
        // The largest representable Q64.64 price maps to a large positive tick near the +443,636
        // saturation edge. Exercises checked_shl(128) and the isqrt at their maximum input — the
        // high-end counterpart to `zero_price_maps_to_min_tick`.
        let tick = price_to_tick(u128::MAX);
        assert!(
            tick > 440_000,
            "max price should map near the saturation tick, got {tick}"
        );
        assert!(tick <= uniswap_v3_math::tick_math::MAX_TICK);
    }

    // ── integer_sqrt ────────────────────────────────────────────────────────────

    #[test]
    fn integer_sqrt_matches_known_squares() {
        use alloy_primitives::U256;
        for v in [
            0u128,
            1,
            4,
            9,
            2,
            3,
            15,
            16,
            17,
            1_000_000,
            u128::from(u64::MAX),
        ] {
            let n = U256::from(v);
            let root = integer_sqrt(n);
            // root^2 <= v < (root+1)^2
            let root_sq = root.checked_mul(root).expect("root^2 fits");
            let next = root.checked_add(U256::from(1u8)).expect("root+1 fits");
            let next_sq = next.checked_mul(next).expect("(root+1)^2 fits");
            assert!(root_sq <= n && n < next_sq, "isqrt({v}) = {root} is wrong");
        }
    }

    #[test]
    fn integer_sqrt_on_large_values() {
        use alloy_primitives::U256;
        // Exact roots of large perfect squares (s <= 2^127 so s^2 fits in U256). This is the
        // magnitude range price_to_tick actually exercises, well beyond u64::MAX.
        for shift in [64usize, 100, 127] {
            let s = U256::ONE.checked_shl(shift).expect("shift < 256");
            let n = s.checked_mul(s).expect("s^2 fits for s <= 2^127");
            assert_eq!(integer_sqrt(n), s, "isqrt((2^{shift})^2) should be exact");
            // One below a perfect square floors to s - 1.
            let n_minus = n.checked_sub(U256::ONE).expect("n > 0");
            let s_minus = s.checked_sub(U256::ONE).expect("s > 0");
            assert_eq!(
                integer_sqrt(n_minus),
                s_minus,
                "isqrt((2^{shift})^2 - 1) should floor to 2^{shift} - 1"
            );
        }

        // The largest input price_to_tick can feed: u128::MAX << 128 (~2^256). Must not panic and
        // satisfy root^2 <= n. The upper bound (root+1)^2 is omitted — it would overflow U256.
        let max_input = U256::from(u128::MAX)
            .checked_shl(128)
            .expect("u128::MAX << 128 fits in U256");
        let root = integer_sqrt(max_input);
        let root_sq = root.checked_mul(root).expect("root^2 < 2^256 fits");
        assert!(root_sq <= max_input, "root^2 must not exceed n");
        assert!(
            root >= U256::ONE.checked_shl(127).expect("shift < 256"),
            "root of a ~2^256 value should be ~2^128"
        );
    }

    // ── tick_to_oracle_price ──────────────────────────────────────────────────────

    #[test]
    fn tick_zero_is_unit_price() {
        // 1.0001^0 = 1.0 → exactly 2^64 in Q64.64.
        assert_eq!(tick_to_oracle_price(0), ONE_Q64_64);
    }

    #[test]
    fn positive_tick_is_above_unit() {
        assert!(tick_to_oracle_price(1) > ONE_Q64_64);
        assert!(tick_to_oracle_price(10_000) > ONE_Q64_64);
    }

    #[test]
    fn negative_tick_is_below_unit() {
        assert!(tick_to_oracle_price(-1) < ONE_Q64_64);
        assert!(tick_to_oracle_price(-10_000) < ONE_Q64_64);
    }

    #[test]
    fn price_is_monotonic_in_tick() {
        let mut prev = tick_to_oracle_price(-50_000);
        for tick in (-49_000..=50_000).step_by(1_000) {
            let cur = tick_to_oracle_price(tick);
            assert!(cur > prev, "price must increase with tick at {tick}");
            prev = cur;
        }
    }

    #[test]
    fn tick_10000_matches_known_ratio() {
        // 1.0001^10000 ≈ 2.71814. Check the ratio in milli-units (× 1000) lands in [2717, 2719]
        // using integer math only — `price * 1000 / 2^64` ≈ 2718.
        let price = tick_to_oracle_price(10_000);
        let ratio_milli = price
            .checked_mul(1_000)
            .and_then(|scaled| scaled.checked_div(ONE_Q64_64))
            .expect("price * 1000 fits in u128");
        assert!(
            (2_717..=2_719).contains(&ratio_milli),
            "got {ratio_milli} / 1000"
        );
    }

    #[test]
    fn extreme_positive_tick_saturates() {
        // 1.0001^MAX_TICK far exceeds 2^64, so the Q64.64 value saturates at u128::MAX.
        let price = tick_to_oracle_price(uniswap_v3_math::tick_math::MAX_TICK);
        assert_eq!(price, u128::MAX);
    }

    #[test]
    fn ticks_beyond_bounds_are_clamped() {
        // Ticks outside [MIN_TICK, MAX_TICK] must not panic; they clamp to the bound.
        assert_eq!(
            tick_to_oracle_price(i32::MAX),
            tick_to_oracle_price(uniswap_v3_math::tick_math::MAX_TICK)
        );
        assert_eq!(
            tick_to_oracle_price(i32::MIN),
            tick_to_oracle_price(uniswap_v3_math::tick_math::MIN_TICK)
        );
    }
}
