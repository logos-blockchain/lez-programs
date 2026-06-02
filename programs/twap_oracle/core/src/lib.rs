use borsh::{BorshDeserialize, BorshSerialize};
use nssa_core::{
    account::{AccountId, Data},
    program::{PdaSeed, ProgramId},
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// TWAP Oracle Program Instruction.
#[derive(Debug, Serialize, Deserialize)]
pub enum Instruction {
    /// Creates and initialises a price observations account for a price source and time window.
    ///
    /// Required accounts (in order):
    /// 1. Price observations account — uninitialized PDA derived from
    ///    `compute_price_observations_pda(self_program_id, price_source.account_id,
    ///    window_duration)`.
    /// 2. Price source account — the account whose ID acts as the feed identifier (e.g. an AMM
    ///    pool account); must be passed with `is_authorized = true` to prove the caller controls
    ///    it.
    /// 3. Clock account — read-only; supplies the initial observation timestamp.
    CreatePriceObservations {
        /// Initial price tick: `floor(log_{1.0001}(reserve_b / reserve_a))`.
        initial_tick: i32,
        /// Duration of the TWAP window this feed serves, in milliseconds.
        ///
        /// Together with `OBSERVATIONS_CAPACITY` this determines the minimum sampling interval
        /// enforced by `RecordTick`: `min_interval = window_duration / OBSERVATIONS_CAPACITY`.
        /// It is also part of the PDA seed, so each window gets a distinct account.
        window_duration: u64,
    },
    /// Creates and initialises a canonical [`OraclePriceAccount`] for a price source and time
    /// window.
    ///
    /// The account is initialised with the non-zero `initial_price` and the timestamp read from
    /// the canonical 1-block clock. A zero price or zero timestamp is the "no valid price"
    /// sentinel consumers reject, so an account is never created in that state.
    ///
    /// Required accounts (in order):
    /// 1. Oracle price account — uninitialized PDA derived from
    ///    `compute_oracle_price_account_pda(self_program_id, price_source.account_id,
    ///    window_duration)`.
    /// 2. Price source account — must be passed with `is_authorized = true` to prove the caller
    ///    controls it. Its ID ties this price account to the same source as the corresponding
    ///    [`PriceObservations`] account for the same window.
    /// 3. Clock account — the canonical 1-block LEZ clock; supplies the initial timestamp.
    CreateOraclePriceAccount {
        /// Canonical identifier of the base asset being priced.
        base_asset: AccountId,
        /// Canonical identifier of the quote asset that denominates `price`.
        quote_asset: AccountId,
        /// Initial price as a `Q64.64` fixed-point value (real price = `initial_price / 2^64`).
        ///
        /// Must be non-zero; the caller is responsible for supplying a correctly-scaled
        /// fixed-point value rather than a plain integer.
        initial_price: u128,
        /// Duration of the TWAP window this price account serves, in milliseconds.
        ///
        /// Part of the PDA seed, so each `(price_source, window)` pair maps to a distinct
        /// oracle price account.
        window_duration: u64,
    },
    /// Creates and initialises a [`CurrentTickAccount`] for a price source.
    ///
    /// Called once per price source (not per window). The account holds the latest tick written
    /// by the price source and serves as the input to `RecordTick`. The price source reports a
    /// spot **price**; the oracle converts it to a tick via [`price_to_tick`], so the source
    /// never needs to know about ticks.
    ///
    /// Required accounts (in order):
    /// 1. Current tick account — uninitialized PDA derived from
    ///    `compute_current_tick_account_pda(self_program_id, price_source.account_id)`.
    /// 2. Price source account — must be passed with `is_authorized = true`.
    /// 3. Clock account — read-only; supplies the initial timestamp.
    CreateCurrentTickAccount {
        /// Opening spot price as a `Q64.64` ratio (`quote_asset` per `base_asset`), e.g. an
        /// AMM's `reserve_b / reserve_a`. Converted to a tick on-chain.
        initial_price: u128,
    },
    /// Updates the tick stored in an existing [`CurrentTickAccount`].
    ///
    /// Called by the price source (e.g. AMM) after each price-changing operation. Anyone may
    /// subsequently call `RecordTick` to advance the [`PriceObservations`] accumulator using
    /// the new tick. The price source reports a spot **price**; the oracle converts it to a tick
    /// via [`price_to_tick`].
    ///
    /// Required accounts (in order):
    /// 1. Current tick account — initialized PDA derived from
    ///    `compute_current_tick_account_pda(self_program_id, price_source.account_id)`.
    /// 2. Price source account — must be passed with `is_authorized = true`.
    /// 3. Clock account — read-only; supplies the updated timestamp.
    UpdateCurrentTick {
        /// New spot price as a `Q64.64` ratio (`quote_asset` per `base_asset`). Converted to a
        /// tick on-chain.
        price: u128,
    },
    /// Records the current tick from a [`CurrentTickAccount`] into a [`PriceObservations`]
    /// ring buffer.
    ///
    /// Permissionless — anyone may call this. Both PDAs are verified against `price_source_id`,
    /// so the tick can only have been written by whoever controls that price source.
    ///
    /// A sampling guard silently skips the write if less than
    /// `window_duration / OBSERVATIONS_CAPACITY` milliseconds have elapsed since the last
    /// observation. Callers may call this on every block without concern — the guard handles
    /// downsampling on-chain.
    ///
    /// Required accounts (in order):
    /// 1. Price observations account — initialized PDA derived from
    ///    `compute_price_observations_pda(self_program_id, price_source_id, window_duration)`.
    /// 2. Current tick account — initialized PDA derived from
    ///    `compute_current_tick_account_pda(self_program_id, price_source_id)`.
    /// 3. Clock account — read-only; supplies the current timestamp.
    RecordTick {
        /// ID of the price source; used to verify both PDAs.
        price_source_id: AccountId,
        /// Duration of the TWAP window in milliseconds; used to verify the
        /// [`PriceObservations`] PDA and to compute the sampling guard interval.
        window_duration: u64,
    },
}

// ──────────────────────────────────────────────────────────────────────────────
// Price feed
// ──────────────────────────────────────────────────────────────────────────────

/// Maximum tick delta injected into the accumulator per observation.
///
/// Matches the Uniswap v4 truncated oracle hook reference value (~2.39× price move per block).
/// An attacker who moves the pool by more than this in one block still only injects
/// `MAX_TICK_DELTA` ticks into the cumulative — they must sustain the manipulation across
/// many blocks while arbitrage erodes their position.
pub const MAX_TICK_DELTA: i32 = 9_116;

/// Number of entries in each price feed.
///
/// 6 396 is the maximum that fits within the `DATA_MAX_LENGTH = 100 KiB` runtime ceiling.
/// Each [`ObservationEntry`] is 16 bytes (`timestamp` 8 + `tick_cumulative` 8); fixed overhead
/// is 52 bytes (`price_source_id` 32 + `write_index` 4 + `total_entries` 8 +
/// `last_recorded_tick` 4 + Borsh `Vec` length prefix 4), leaving 102 348 bytes for entries:
/// `floor(102 348 / 16) = 6 396`.
///
/// The effective history window depends on the `window_duration` used to derive the feed PDA
/// and the sampling guard: `min_interval = window_duration / OBSERVATIONS_CAPACITY`. A 24 h feed
/// samples every ~13 s; a 7 d feed every ~94 s; a 30 d feed every ~7 min.
pub const OBSERVATIONS_CAPACITY: u32 = 6396;

/// A single price entry written to a [`PriceObservations`].
#[derive(
    Debug, Default, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct ObservationEntry {
    /// Block timestamp (milliseconds) when this entry was recorded.
    pub timestamp: u64,
    /// Running sum of `tick × elapsed_ms` up to this entry.
    ///
    /// Grows without bound over time, which is why this is `i64` rather than `i32`.
    /// The TWAP over any window `[t1, t2]` (timestamps in milliseconds) is computed as
    /// `(tick_cumulative[t2] - tick_cumulative[t1]) / (t2 - t1)`.
    pub tick_cumulative: i64,
}

/// Circular price feed of tick observations for a price source and time window.
///
/// Owned by the TWAP oracle as a PDA derived from
/// `compute_price_observations_pda(oracle_program_id, price_source_id, window_duration)`.
/// The window duration is not stored here — it is implicit in the PDA address. Any caller
/// that locates this account already knows the window duration used to derive it.
/// Only the account that controls `price_source_id` (proven via `is_authorized = true` at call
/// time) may append new entries via `RecordTick`.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PriceObservations {
    /// ID of the price source account this feed is associated with (e.g. an AMM pool).
    /// The price feed PDA is derived from this ID and the window duration, so authorization is
    /// implicit: whoever controls `price_source_id` is authorized to record prices.
    pub price_source_id: AccountId,
    /// Index of the *next* slot to write (wraps at `OBSERVATIONS_CAPACITY`).
    pub write_index: u32,
    /// Total entries ever appended (never resets; used to detect empty/partial-fill state).
    pub total_entries: u64,
    /// The raw (untruncated) tick from the most recent `RecordTick` call.
    ///
    /// Used by `RecordTick` to compute the tick delta for the next observation:
    /// `delta = current_tick - last_recorded_tick`. Stored as the actual tick, not the
    /// clamped value, so that each successive delta is computed from the true price position.
    pub last_recorded_tick: i32,
    /// Circular price entries; always exactly `OBSERVATIONS_CAPACITY` elements.
    pub entries: Vec<ObservationEntry>,
}

impl TryFrom<&Data> for PriceObservations {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&PriceObservations> for Data {
    fn from(feed: &PriceObservations) -> Self {
        let serialized_len =
            borsh::object_length(feed).expect("PriceObservations length must be known");
        let mut data = Vec::with_capacity(serialized_len);
        BorshSerialize::serialize(feed, &mut data).expect("Serialization to Vec should not fail");
        Self::try_from(data).expect("PriceObservations encoded data should fit into Data")
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// PDA helpers
// ──────────────────────────────────────────────────────────────────────────────

const PRICE_OBSERVATIONS_PDA_SEED: [u8; 32] = [2; 32];

/// Derives the [`AccountId`] for a price source's [`PriceObservations`] PDA.
///
/// The `window_duration` is included in the seed so that each `(price_source, window)` pair
/// maps to a distinct account.
#[must_use]
pub fn compute_price_observations_pda(
    oracle_program_id: ProgramId,
    price_source_id: AccountId,
    window_duration: u64,
) -> AccountId {
    AccountId::for_public_pda(
        &oracle_program_id,
        &compute_price_observations_pda_seed(price_source_id, window_duration),
    )
}

/// Derives the [`PdaSeed`] for a price source's [`PriceObservations`].
///
/// Hash input: `price_source_id (32 bytes) || window_duration_le (8 bytes) ||
/// PRICE_OBSERVATIONS_PDA_SEED (32 bytes)`.
#[must_use]
pub fn compute_price_observations_pda_seed(
    price_source_id: AccountId,
    window_duration: u64,
) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = [0u8; 72];
    bytes[..32].copy_from_slice(&price_source_id.to_bytes());
    bytes[32..40].copy_from_slice(&window_duration.to_le_bytes());
    bytes[40..72].copy_from_slice(&PRICE_OBSERVATIONS_PDA_SEED);

    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

const ORACLE_PRICE_ACCOUNT_PDA_SEED: [u8; 32] = [3; 32];

/// Derives the [`AccountId`] for a price source's [`OraclePriceAccount`] PDA.
///
/// The `window_duration` is included in the seed so that each `(price_source, window)` pair
/// maps to a distinct account, mirroring the [`PriceObservations`] PDA derivation.
#[must_use]
pub fn compute_oracle_price_account_pda(
    oracle_program_id: ProgramId,
    price_source_id: AccountId,
    window_duration: u64,
) -> AccountId {
    AccountId::for_public_pda(
        &oracle_program_id,
        &compute_oracle_price_account_pda_seed(price_source_id, window_duration),
    )
}

/// Derives the [`PdaSeed`] for a price source's [`OraclePriceAccount`].
///
/// Hash input: `price_source_id (32 bytes) || window_duration_le (8 bytes) ||
/// ORACLE_PRICE_ACCOUNT_PDA_SEED (32 bytes)`.
#[must_use]
pub fn compute_oracle_price_account_pda_seed(
    price_source_id: AccountId,
    window_duration: u64,
) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = [0u8; 72];
    bytes[..32].copy_from_slice(&price_source_id.to_bytes());
    bytes[32..40].copy_from_slice(&window_duration.to_le_bytes());
    bytes[40..72].copy_from_slice(&ORACLE_PRICE_ACCOUNT_PDA_SEED);

    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

/// Canonical oracle price account consumed by LEZ programs.
///
/// Oracle producers own how this account is written; consumers only read and
/// validate it. The account shape is intentionally generic so that any oracle
/// type (TWAP, external adaptor, aggregator) can use the same interface.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct OraclePriceAccount {
    /// Canonical identifier for the priced asset.
    pub base_asset: AccountId,
    /// Canonical identifier for the quote asset that denominates `price`.
    pub quote_asset: AccountId,
    /// Amount of `quote_asset` one unit of `base_asset` is worth.
    ///
    /// `u128` keeps the consumer-side interface non-negative; zero is rejected on read.
    pub price: u128,
    /// Price observation timestamp. Consumers choose the time unit by matching this with
    /// `max_age`.
    pub timestamp: u64,
    /// Identifier of the source account that populated this account, such as a TWAP program or
    /// external adaptor.
    pub source_id: AccountId,
    /// Source-provided confidence interval, or zero when the source does not provide one.
    pub confidence_interval: u128,
}

impl TryFrom<&Data> for OraclePriceAccount {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&OraclePriceAccount> for Data {
    fn from(price_account: &OraclePriceAccount) -> Self {
        let serialized_len =
            borsh::object_length(price_account).expect("Oracle price account length must be known");
        let mut data = Vec::with_capacity(serialized_len);
        BorshSerialize::serialize(price_account, &mut data)
            .expect("Serialization to Vec should not fail");
        Self::try_from(data).expect("Oracle price account encoded data should fit into Data")
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Price → tick conversion
// ──────────────────────────────────────────────────────────────────────────────

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

// ──────────────────────────────────────────────────────────────────────────────
// Current tick account
// ──────────────────────────────────────────────────────────────────────────────

/// Live price tick for a price source, written by the price source on every price-changing
/// operation.
///
/// Owned by the TWAP oracle as a PDA derived from
/// `compute_current_tick_account_pda(oracle_program_id, price_source_id)`.
/// One account exists per price source; it is shared across all time windows for that source.
/// Anyone may call `RecordTick` to advance a [`PriceObservations`] accumulator using the tick
/// stored here.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct CurrentTickAccount {
    /// Most recent raw tick written by the price source:
    /// `floor(log_{1.0001}(reserve_b / reserve_a))`.
    pub tick: i32,
    /// Block timestamp (milliseconds) when `tick` was last written.
    pub last_updated: u64,
}

impl TryFrom<&Data> for CurrentTickAccount {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&CurrentTickAccount> for Data {
    fn from(account: &CurrentTickAccount) -> Self {
        let serialized_len =
            borsh::object_length(account).expect("CurrentTickAccount length must be known");
        let mut data = Vec::with_capacity(serialized_len);
        BorshSerialize::serialize(account, &mut data)
            .expect("Serialization to Vec should not fail");
        Self::try_from(data).expect("CurrentTickAccount encoded data should fit into Data")
    }
}

const CURRENT_TICK_ACCOUNT_PDA_SEED: [u8; 32] = [4; 32];

/// Derives the [`AccountId`] for a price source's [`CurrentTickAccount`] PDA.
#[must_use]
pub fn compute_current_tick_account_pda(
    oracle_program_id: ProgramId,
    price_source_id: AccountId,
) -> AccountId {
    AccountId::for_public_pda(
        &oracle_program_id,
        &compute_current_tick_account_pda_seed(price_source_id),
    )
}

/// Derives the [`PdaSeed`] for a price source's [`CurrentTickAccount`].
///
/// Hash input: `price_source_id (32 bytes) || CURRENT_TICK_ACCOUNT_PDA_SEED (32 bytes)`.
#[must_use]
pub fn compute_current_tick_account_pda_seed(price_source_id: AccountId) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = [0u8; 64];
    bytes[..32].copy_from_slice(&price_source_id.to_bytes());
    bytes[32..64].copy_from_slice(&CURRENT_TICK_ACCOUNT_PDA_SEED);

    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
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
}
