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
        /// enforced by `RecordPrice`: `min_interval = window_duration / OBSERVATIONS_CAPACITY`.
        /// It is also part of the PDA seed, so each window gets a distinct account.
        window_duration: u64,
    },
    /// Creates and initialises a canonical [`OraclePriceAccount`] for a price source and time
    /// window.
    ///
    /// The price and timestamp start at zero and are populated later by a `PublishPrice`
    /// instruction. Consumers must reject accounts whose `timestamp` is zero or stale.
    ///
    /// Required accounts (in order):
    /// 1. Oracle price account — uninitialized PDA derived from
    ///    `compute_oracle_price_account_pda(self_program_id, price_source.account_id,
    ///    window_duration)`.
    /// 2. Price source account — must be passed with `is_authorized = true` to prove the caller
    ///    controls it. Its ID ties this price account to the same source as the corresponding
    ///    [`PriceObservations`] account for the same window.
    CreateOraclePriceAccount {
        /// Canonical identifier of the base asset being priced.
        base_asset: AccountId,
        /// Canonical identifier of the quote asset that denominates `price`.
        quote_asset: AccountId,
        /// Duration of the TWAP window this price account serves, in milliseconds.
        ///
        /// Part of the PDA seed, so each `(price_source, window)` pair maps to a distinct
        /// oracle price account.
        window_duration: u64,
    },
    /// Creates and initialises a [`CurrentTickAccount`] for a price source.
    ///
    /// Called once per price source (not per window). The account holds the latest raw tick
    /// written by the price source and serves as the input to `RecordTick`.
    ///
    /// Required accounts (in order):
    /// 1. Current tick account — uninitialized PDA derived from
    ///    `compute_current_tick_account_pda(self_program_id, price_source.account_id)`.
    /// 2. Price source account — must be passed with `is_authorized = true`.
    /// 3. Clock account — read-only; supplies the initial timestamp.
    CreateCurrentTickAccount {
        /// Opening tick: `floor(log_{1.0001}(reserve_b / reserve_a))` at creation time.
        initial_tick: i32,
    },
    /// Updates the tick stored in an existing [`CurrentTickAccount`].
    ///
    /// Called by the price source (e.g. AMM) after each price-changing operation. Anyone may
    /// subsequently call `RecordTick` to advance the [`PriceObservations`] accumulator using
    /// the new tick.
    ///
    /// Required accounts (in order):
    /// 1. Current tick account — initialized PDA derived from
    ///    `compute_current_tick_account_pda(self_program_id, price_source.account_id)`.
    /// 2. Price source account — must be passed with `is_authorized = true`.
    /// 3. Clock account — read-only; supplies the updated timestamp.
    UpdateCurrentTick {
        /// New raw tick from the price source.
        tick: i32,
    },
    /// Computes the TWAP over `window_duration` from the [`PriceObservations`] ring buffer and
    /// writes the result to the [`OraclePriceAccount`].
    ///
    /// Permissionless — anyone may call this. Returns all accounts unchanged (no-op) if the
    /// ring buffer holds fewer than two observations. Once at least two observations exist the
    /// TWAP is computed over the available history, which may be shorter than `window_duration`
    /// while the buffer is young.
    ///
    /// The resulting TWAP tick is stored in [`OraclePriceAccount::price`] via
    /// [`tick_to_oracle_price`]. Consumers decode with [`oracle_price_to_tick`].
    ///
    /// Required accounts (in order):
    /// 1. Price observations account — initialized PDA derived from
    ///    `compute_price_observations_pda(self_program_id, price_source_id, window_duration)`.
    /// 2. Oracle price account — initialized PDA derived from
    ///    `compute_oracle_price_account_pda(self_program_id, price_source_id, window_duration)`.
    /// 3. Clock account — read-only; supplies the publication timestamp.
    PublishPrice {
        /// ID of the price source; used to verify both PDAs.
        price_source_id: AccountId,
        /// Duration of the TWAP window in milliseconds; used to verify both PDAs and to
        /// locate the boundary observation in the ring buffer.
        window_duration: u64,
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
    /// Running sum of `tick × elapsed_milliseconds` up to this entry.
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
/// time) may append new entries via `RecordPrice`.
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
// TWAP price encoding
// ──────────────────────────────────────────────────────────────────────────────

/// Number of fractional bits in the [`OraclePriceAccount::price`] fixed-point value.
///
/// The price is stored as a `Q64.64` ratio: `OraclePriceAccount::price / 2^PRICE_FRACTIONAL_BITS`
/// is the amount of `quote_asset` one unit of `base_asset` is worth. A consumer multiplies a
/// token amount by the price with `(amount * price) >> PRICE_FRACTIONAL_BITS`.
pub const PRICE_FRACTIONAL_BITS: u32 = 64;

/// Converts a TWAP tick into the `Q64.64` fixed-point price stored in
/// [`OraclePriceAccount::price`].
///
/// The price is `1.0001^tick`, computed via the Uniswap v3 `sqrtPriceX96` representation
/// (pure-integer, no floating point) and then squared back to a plain ratio:
///
/// ```text
/// sqrtPriceX96 = sqrt(1.0001^tick) * 2^96
/// price        = sqrtPriceX96^2 / 2^128 = 1.0001^tick * 2^64   (Q64.64)
/// ```
///
/// `sqrtPriceX96^2` is computed with [`full_math::mul_div`] using a 512-bit intermediate, so it
/// never overflows for any valid tick. The tick is clamped to `[MIN_TICK, MAX_TICK]` and the
/// result saturates at `u128::MAX` for the (practically unreachable) ticks above ~443 636 whose
/// ratio would exceed `2^64`.
///
/// See `docs/twap-oracle-tick-to-price-conversion.md` for the full derivation.
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

    /// `1.0` in Q64.64 is `2^64`.
    const ONE_Q64_64: u128 = 1u128 << PRICE_FRACTIONAL_BITS;

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
