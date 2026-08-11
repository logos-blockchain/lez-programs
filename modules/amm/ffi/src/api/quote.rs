//! Shared opening-deposit math for pool creation.
//!
//! Reused by `liquidity::create_pool_quote` to size the smallest deposit that clears
//! `MINIMUM_LIQUIDITY` for a given opening price.

use alloy_primitives::U256;
use amm_core::MINIMUM_LIQUIDITY;

/// `Q64.64` scaling factor (`2^64`).
pub(super) const Q64: u128 = 1_u128 << 64;

/// Smallest `(amount_a, amount_b)` deposit whose geometric-mean LP clears
/// `MINIMUM_LIQUIDITY`, holding the canonical opening `price` (token B per token A,
/// `Q64.64`). Binary-searches the smaller side, then derives the paired amount by ceil.
pub(super) fn minimum_opening_pair(price: u128) -> Result<(u128, u128), String> {
    let minimum_initial_lp = U256::from(MINIMUM_LIQUIDITY + 1);
    let target_product = minimum_initial_lp
        .checked_mul(minimum_initial_lp)
        .ok_or_else(|| String::from("minimum liquidity product overflow"))?;
    if price >= Q64 {
        let amount_a = binary_search_min(1, MINIMUM_LIQUIDITY + 1, |amount_a| {
            let amount_b = div_ceil_u256(U256::from(amount_a) * U256::from(price), U256::from(Q64));
            U256::from(amount_a) * amount_b >= target_product
        });
        let amount_b = div_ceil_u256(U256::from(amount_a) * U256::from(price), U256::from(Q64));
        Ok((
            amount_a,
            u128::try_from(amount_b).map_err(|_| String::from("opening amount overflow"))?,
        ))
    } else {
        let amount_b = binary_search_min(1, MINIMUM_LIQUIDITY + 1, |amount_b| {
            let amount_a = div_ceil_u256(U256::from(amount_b) * U256::from(Q64), U256::from(price));
            amount_a * U256::from(amount_b) >= target_product
        });
        let amount_a = div_ceil_u256(U256::from(amount_b) * U256::from(Q64), U256::from(price));
        Ok((
            u128::try_from(amount_a).map_err(|_| String::from("opening amount overflow"))?,
            amount_b,
        ))
    }
}

fn binary_search_min(mut low: u128, mut high: u128, predicate: impl Fn(u128) -> bool) -> u128 {
    while low < high {
        let mid = low + (high - low) / 2;
        if predicate(mid) {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}

pub(super) fn div_ceil_u256(numerator: U256, denominator: U256) -> U256 {
    numerator.div_ceil(denominator)
}
