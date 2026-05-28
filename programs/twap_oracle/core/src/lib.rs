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
}

// ──────────────────────────────────────────────────────────────────────────────
// Price feed
// ──────────────────────────────────────────────────────────────────────────────

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
    /// The TWAP over any window `[t1, t2]` is computed as
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
