use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, ShardData},
    program::PdaSeed,
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// TWAP Oracle Program Instruction.
///
/// Borsh is the instruction wire format LEZ reads, and deliberately the *only* encoding
/// this type derives.
///
/// `#[lez_program]` generates its guest-side counterpart with serde derives alongside
/// Borsh, "for IDL/tooling". Nothing uses them: `idl-gen` parses the guest source, the
/// framework's runtime IDL path deserializes its own types, and `spel-cli` encodes Borsh
/// straight from the IDL. Carrying them here would only mean a type that accepts either
/// encoder, which is how the v0.2.5 port silently rotted every instruction-encoding site in
/// `modules/*/ffi` -- `risc0_zkvm::serde::to_vec` kept compiling and kept producing bytes
/// the runtime rejects. Without a serde impl that is a compile error instead.
///
/// Account-data types below keep serde; the FFI renders them as JSON. This applies to the
/// instruction enum alone.
///
/// Borsh encodes the variant as a leading tag byte, so variants are append-only:
/// inserting one shifts the encoding of every variant after it.
#[derive(Debug, BorshSerialize, BorshDeserialize)]
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
        /// Wall-clock milliseconds, pinned **exactly** against `CLOCK_01`.
        ///
        /// Exact, not bounded, unlike the faucet's cooldown: an oracle timestamp
        /// is data consumers act on, and neither direction of slack is safe.
        /// Understating it would let a caller stretch `RecordTick`'s sampling
        /// interval and over-weight a segment; overstating it would make a stale
        /// price look fresh. The cost is that a publish races the next block and
        /// must be retried, which is the keeper's problem, not a correctness one.
        now: u64,
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
        /// Wall-clock milliseconds, pinned **exactly** against `CLOCK_01`.
        ///
        /// Exact, not bounded, unlike the faucet's cooldown: an oracle timestamp
        /// is data consumers act on, and neither direction of slack is safe.
        /// Understating it would let a caller stretch `RecordTick`'s sampling
        /// interval and over-weight a segment; overstating it would make a stale
        /// price look fresh. The cost is that a publish races the next block and
        /// must be retried, which is the keeper's problem, not a correctness one.
        now: u64,
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
        /// Wall-clock milliseconds, pinned **exactly** against `CLOCK_01`.
        ///
        /// Exact, not bounded, unlike the faucet's cooldown: an oracle timestamp
        /// is data consumers act on, and neither direction of slack is safe.
        /// Understating it would let a caller stretch `RecordTick`'s sampling
        /// interval and over-weight a segment; overstating it would make a stale
        /// price look fresh. The cost is that a publish races the next block and
        /// must be retried, which is the keeper's problem, not a correctness one.
        now: u64,
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
        /// Wall-clock milliseconds, pinned **exactly** against `CLOCK_01`.
        ///
        /// Exact, not bounded, unlike the faucet's cooldown: an oracle timestamp
        /// is data consumers act on, and neither direction of slack is safe.
        /// Understating it would let a caller stretch `RecordTick`'s sampling
        /// interval and over-weight a segment; overstating it would make a stale
        /// price look fresh. The cost is that a publish races the next block and
        /// must be retried, which is the keeper's problem, not a correctness one.
        now: u64,
    },
    /// Computes the TWAP over `window_duration` from the [`PriceObservations`] ring buffer and
    /// writes the result to the [`OraclePriceAccount`].
    ///
    /// Permissionless — anyone may call this. Returns all accounts unchanged (no-op) if the
    /// ring buffer holds fewer than two observations. Once at least two observations exist the
    /// TWAP is computed over the available history, which may be shorter than `window_duration`
    /// while the buffer is young.
    ///
    /// The resulting TWAP tick is converted to a `Q64.64` price ratio via [`tick_to_oracle_price`]
    /// and stored in [`OraclePriceAccount::price`]. The stored value is a plain price ratio, not a
    /// tick encoding, so consumers read it directly (`price / 2^PRICE_FRACTIONAL_BITS`) and the
    /// account stays source-agnostic — a non-tick source (e.g. RedStone, Pyth) writes the same
    /// field.
    ///
    /// Required accounts (in order):
    /// 1. Price observations account — initialized PDA derived from
    ///    `compute_price_observations_pda(self_program_id, price_source_id, window_duration)`.
    /// 2. Oracle price account — initialized PDA derived from
    ///    `compute_oracle_price_account_pda(self_program_id, price_source_id, window_duration)`.
    /// 3. Current tick account — initialized PDA derived from
    ///    `compute_current_tick_account_pda(self_program_id, price_source_id)`. Supplies the live
    ///    tick used to extend the average from the newest stored observation up to `now`.
    /// 4. Clock account — read-only; supplies the publication timestamp.
    PublishPrice {
        /// ID of the price source; used to verify all three PDAs.
        price_source_id: AccountId,
        /// Duration of the TWAP window in milliseconds. Used only to verify the price
        /// observations and oracle price account PDAs (the current tick account PDA does not
        /// depend on it). No boundary search is performed: the oldest valid observation is the
        /// ring buffer's natural start (entry 0 while partially filled, else the slot at
        /// `write_index`), because each observations account is calibrated to one window via its
        /// sampling guard.
        window_duration: u64,
        /// Wall-clock milliseconds, pinned **exactly** against `CLOCK_01`.
        ///
        /// Exact, not bounded, unlike the faucet's cooldown: an oracle timestamp
        /// is data consumers act on, and neither direction of slack is safe.
        /// Understating it would let a caller stretch `RecordTick`'s sampling
        /// interval and over-weight a segment; overstating it would make a stale
        /// price look fresh. The cost is that a publish races the next block and
        /// must be retried, which is the keeper's problem, not a correctness one.
        now: u64,
        /// The live tick and when it was written, each pinned against the
        /// `CurrentTickAccount` shard by its own guard.
        current_tick: i32,
        current_tick_last_updated: u64,
        /// The TWAP the caller computed, or `None` to claim there are fewer than
        /// two observations and so nothing to publish.
        ///
        /// Neither is taken on trust. The guard on the observations shard
        /// recomputes the average from the ring buffer it can see plus the three
        /// pinned scalars above and asserts it matches. That is the whole reason
        /// those scalars are declared: no single apply sees the buffer, the live
        /// tick and the clock at once, so the computation runs where the buffer
        /// is and every input it lacks is pinned elsewhere.
        twap_price: Option<u128>,
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
        /// Wall-clock milliseconds, pinned **exactly** against `CLOCK_01`.
        ///
        /// Exact, not bounded, unlike the faucet's cooldown: an oracle timestamp
        /// is data consumers act on, and neither direction of slack is safe.
        /// Understating it would let a caller stretch `RecordTick`'s sampling
        /// interval and over-weight a segment; overstating it would make a stale
        /// price look fresh. The cost is that a publish races the next block and
        /// must be retried, which is the keeper's problem, not a correctness one.
        now: u64,
        /// The tick to record, pinned against the `CurrentTickAccount` shard.
        ///
        /// The observations shard's apply is what integrates it, and that apply
        /// cannot see the tick account — so the tick travels in the instruction
        /// and a guard makes the claim true.
        current_tick: i32,
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
/// Bounded by the zkVM cycle budget, not storage: `RecordTick` commits this owned account on both
/// read and write, so cost scales with size and a full 100 KiB buffer exceeds the public-execution
/// limit. See `programs/benchmark/README.md` and `programs/benchmark/tests/twap_cycle_bench.rs`.
///
/// Capacity primarily affects resolution: `min_interval = window_duration / OBSERVATIONS_CAPACITY`
/// (integer division), so full-buffer coverage is approximately `window_duration` (up to
/// `< OBSERVATIONS_CAPACITY` ms shorter) — only resolution changes meaningfully.
pub const OBSERVATIONS_CAPACITY: u32 = 2048;

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

impl TryFrom<&ShardData> for PriceObservations {
    type Error = std::io::Error;

    fn try_from(data: &ShardData) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&PriceObservations> for ShardData {
    fn from(feed: &PriceObservations) -> Self {
        let serialized_len =
            borsh::object_length(feed).expect("PriceObservations length must be known");
        let mut data = Vec::with_capacity(serialized_len);
        BorshSerialize::serialize(feed, &mut data).expect("Serialization to Vec should not fail");
        Self::try_from(data).expect("PriceObservations encoded data should fit into ShardData")
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// PDA helpers
// ──────────────────────────────────────────────────────────────────────────────

const PRICE_OBSERVATIONS_PDA_SEED: &[u8] = b"PRICE_OBSERVATIONS";

/// Derives the [`AccountId`] for a price source's [`PriceObservations`] PDA.
///
/// The `window_duration` is included in the seed so that each `(price_source, window)` pair
/// maps to a distinct account.
#[must_use]
pub fn compute_price_observations_pda(
    oracle_program_id: AccountId,
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
/// PRICE_OBSERVATIONS_PDA_SEED`.
#[must_use]
pub fn compute_price_observations_pda_seed(
    price_source_id: AccountId,
    window_duration: u64,
) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&price_source_id.to_bytes());
    bytes.extend_from_slice(&window_duration.to_le_bytes());
    bytes.extend_from_slice(PRICE_OBSERVATIONS_PDA_SEED);

    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

const ORACLE_PRICE_ACCOUNT_PDA_SEED: &[u8] = b"ORACLE_PRICE_ACCOUNT";

/// Derives the [`AccountId`] for a price source's [`OraclePriceAccount`] PDA.
///
/// The `window_duration` is included in the seed so that each `(price_source, window)` pair
/// maps to a distinct account, mirroring the [`PriceObservations`] PDA derivation.
#[must_use]
pub fn compute_oracle_price_account_pda(
    oracle_program_id: AccountId,
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
/// ORACLE_PRICE_ACCOUNT_PDA_SEED`.
#[must_use]
pub fn compute_oracle_price_account_pda_seed(
    price_source_id: AccountId,
    window_duration: u64,
) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&price_source_id.to_bytes());
    bytes.extend_from_slice(&window_duration.to_le_bytes());
    bytes.extend_from_slice(ORACLE_PRICE_ACCOUNT_PDA_SEED);

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

impl TryFrom<&ShardData> for OraclePriceAccount {
    type Error = std::io::Error;

    fn try_from(data: &ShardData) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&OraclePriceAccount> for ShardData {
    fn from(price_account: &OraclePriceAccount) -> Self {
        let serialized_len =
            borsh::object_length(price_account).expect("Oracle price account length must be known");
        let mut data = Vec::with_capacity(serialized_len);
        BorshSerialize::serialize(price_account, &mut data)
            .expect("Serialization to Vec should not fail");
        Self::try_from(data).expect("Oracle price account encoded data should fit into ShardData")
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Price → tick conversion
// ──────────────────────────────────────────────────────────────────────────────
/// Guard payload: the clock must read exactly the declared time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CheckClock {
    pub now: u64,
}

/// Guard payload: the `CurrentTickAccount` must hold exactly these values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CheckCurrentTick {
    pub tick: i32,
    /// `None` where only the tick matters (`RecordTick`); `Some` where the
    /// publish tail is split at it.
    pub last_updated: Option<u64>,
}

/// Effect payload: write a whole record into an empty shard.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Create {
    pub data: ShardData,
}

/// Effect payload: overwrite the live tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct WriteCurrentTick {
    pub tick: i32,
    pub last_updated: u64,
}

/// Effect payload: append an observation, subject to the sampling guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct AppendObservation {
    pub tick: i32,
    pub now: u64,
    pub window_duration: u64,
}

/// Guard payload: the declared TWAP must be what this ring buffer yields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CheckTwap {
    pub tick: i32,
    pub last_updated: u64,
    pub now: u64,
    pub twap_price: Option<u128>,
}

/// Effect payload: publish a price that a [`CheckTwap`] guard has already pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PublishTwap {
    pub price: u128,
    pub now: u64,
}

/// The TWAP over `[oldest stored observation, now]`, as a `Q64.64` price.
///
/// Returns `None` when fewer than two observations exist — there is nothing to
/// average yet, and the price account must stay at its `timestamp = 0`
/// "no valid price" sentinel.
///
/// Split out of `PublishPrice`'s handler so it can run inside an *apply* half.
/// The computation needs the ring buffer, the live tick and the wall clock, and
/// no single apply sees more than one shard — so it runs where the buffer is, in
/// a guard on the observations shard, with `tick`/`last_updated`/`now` arriving
/// as declared values that their own guards pin to the shards they came from.
///
/// The *body* of the average comes from the ring buffer (oldest valid entry `t1`
/// through newest stored entry `t2`). The *final segment* from `t2` to `now` is
/// extrapolated from the current tick, mirroring Uniswap's
/// `OracleLibrary.consult`, which always projects the accumulator to the present
/// rather than reading only stored points.
///
/// The tail is split at `last_updated`: the live tick is only known to have held
/// since then, so it is integrated only over `[last_updated, now]`, while
/// `[t2, last_updated]` carries the tick stored at t2 (`last_recorded_tick`).
/// Without this split a tick written moments before publish would be smeared
/// across the whole unrecorded gap, letting a late spot move dominate a long
/// stale tail. The live-tick segment is clamped against `last_recorded_tick` by
/// [`MAX_TICK_DELTA`] — the same anti-manipulation bound `RecordTick` applies.
///
/// # Panics
/// If any intermediate exceeds `i64`, or `now` is before the oldest observation.
#[must_use]
pub fn compute_twap_price(
    observations: &PriceObservations,
    tick: i32,
    last_updated: u64,
    now: u64,
) -> Option<u128> {
    // Need at least two observations to compute a TWAP.
    if observations.total_entries < 2 {
        return None;
    }

    let capacity =
        usize::try_from(OBSERVATIONS_CAPACITY).expect("OBSERVATIONS_CAPACITY fits in usize");

    // t2: the most recent observation.
    let t2_index = if observations.write_index == 0 {
        capacity
            .checked_sub(1)
            .expect("OBSERVATIONS_CAPACITY is non-zero")
    } else {
        usize::try_from(
            observations
                .write_index
                .checked_sub(1)
                .expect("write_index > 0"),
        )
        .expect("write_index - 1 fits in usize")
    };

    // t1: the oldest valid observation. Once the buffer is full, the oldest entry sits at
    // write_index (the slot about to be overwritten next). Before that, entries start at 0.
    let is_full = observations.total_entries >= u64::from(OBSERVATIONS_CAPACITY);
    let t1_index = if is_full {
        usize::try_from(observations.write_index).expect("write_index fits in usize")
    } else {
        0
    };

    let t1 = observations
        .entries
        .get(t1_index)
        .expect("t1_index is within bounds");
    let t2 = observations
        .entries
        .get(t2_index)
        .expect("t2_index is within bounds");

    let delta = tick.saturating_sub(observations.last_recorded_tick);
    let clamped_delta = delta.clamp(-MAX_TICK_DELTA, MAX_TICK_DELTA);
    let clamped_tick = observations
        .last_recorded_tick
        .saturating_add(clamped_delta);

    // `now` comes from the canonical clock and is monotonic, so it is never before the newest
    // stored observation. The boundary is clamped into `[t2.timestamp, now]`: a `last_updated` at
    // or before t2 means the live tick provably held for the whole tail (pre-boundary segment
    // empty); one at or after `now` means the live tick has accrued no elapsed time yet
    // (post-boundary segment empty). The clamp also keeps a degenerate clock from underflowing the
    // saturating subtractions into a huge tail.
    let boundary = last_updated.clamp(t2.timestamp, now);
    let pre_ms = boundary.saturating_sub(t2.timestamp);
    let post_ms = now.saturating_sub(boundary);
    let pre_ms_i64 = i64::try_from(pre_ms).expect("pre_ms fits in i64");
    let post_ms_i64 = i64::try_from(post_ms).expect("post_ms fits in i64");

    let pre_cumulative = i64::from(observations.last_recorded_tick)
        .checked_mul(pre_ms_i64)
        .expect("pre-boundary tail cumulative fits in i64");
    let post_cumulative = i64::from(clamped_tick)
        .checked_mul(post_ms_i64)
        .expect("post-boundary tail cumulative fits in i64");
    let cum_now = t2
        .tick_cumulative
        .checked_add(pre_cumulative)
        .and_then(|acc| acc.checked_add(post_cumulative))
        .expect("extrapolated tick_cumulative fits in i64");

    // Average over [t1, now]. `now > t1.timestamp` because `now >= t2.timestamp > t1.timestamp`
    // (the sampling guard makes stored timestamps strictly increasing), so the divisor is positive.
    let elapsed_ms = now.checked_sub(t1.timestamp).expect("now >= t1.timestamp");
    let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
    let cumulative_diff = cum_now
        .checked_sub(t1.tick_cumulative)
        .expect("tick_cumulative difference fits in i64");
    // Floor division (round toward -inf), matching Uniswap's `OracleLibrary.consult`. The divisor
    // is always positive, so `div_euclid` is exactly the floor. Plain truncating division would
    // round toward zero, biasing negative TWAPs upward by one tick.
    let twap_tick_i64 = cumulative_diff
        .checked_div_euclid(elapsed_ms_i64)
        .expect("elapsed_ms is non-zero");
    let twap_tick = i32::try_from(twap_tick_i64).expect("TWAP tick fits in i32");

    Some(tick_to_oracle_price(twap_tick))
}

/// Append `tick` to the ring buffer at `now`, or `None` if the sampling guard
/// says it is too soon.
///
/// `None` is how "too soon" is expressed now: an apply returning `None` leaves
/// its shard untouched, which is exactly what the old handler did by returning
/// every account unchanged. Callers may still invoke `RecordTick` every block.
///
/// # Panics
/// If the accumulator would exceed `i64`.
#[must_use]
pub fn record_observation(
    observations: &PriceObservations,
    tick: i32,
    now: u64,
    window_duration: u64,
) -> Option<PriceObservations> {
    let mut observations = observations.clone();
    let capacity =
        usize::try_from(OBSERVATIONS_CAPACITY).expect("OBSERVATIONS_CAPACITY fits in usize");

    // Sampling guard: enforce minimum interval between observations. Floored to 1 so a degenerate
    // `window_duration < OBSERVATIONS_CAPACITY` (rejected at creation, but guarded here too) cannot
    // zero out the interval and let same-timestamp writes through.
    let min_interval = window_duration
        .checked_div(u64::from(OBSERVATIONS_CAPACITY))
        .expect("OBSERVATIONS_CAPACITY is non-zero")
        .max(1);
    let last_index = if observations.write_index == 0 {
        capacity
            .checked_sub(1)
            .expect("OBSERVATIONS_CAPACITY is non-zero")
    } else {
        usize::try_from(
            observations
                .write_index
                .checked_sub(1)
                .expect("write_index > 0"),
        )
        .expect("write_index - 1 fits in usize")
    };
    let last_entry = observations
        .entries
        .get(last_index)
        .expect("last_index is within bounds");
    let last_timestamp = last_entry.timestamp;
    let last_cumulative = last_entry.tick_cumulative;
    let elapsed_ms = now.saturating_sub(last_timestamp);

    if elapsed_ms < min_interval {
        return None;
    }

    // Cap the per-observation tick move (anti-manipulation), then integrate the resulting *tick*
    // — not the delta — so a constant tick still accumulates `tick * dt` per the TWAP formula.
    let delta = tick.saturating_sub(observations.last_recorded_tick);
    let clamped_delta = delta.clamp(-MAX_TICK_DELTA, MAX_TICK_DELTA);
    let clamped_tick = observations
        .last_recorded_tick
        .saturating_add(clamped_delta);

    let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
    let new_cumulative = i64::from(clamped_tick)
        .checked_mul(elapsed_ms_i64)
        .and_then(|product| last_cumulative.checked_add(product))
        .expect("tick_cumulative fits in i64");

    let write_index = usize::try_from(observations.write_index).expect("write_index fits in usize");
    *observations
        .entries
        .get_mut(write_index)
        .expect("write_index is within bounds") = ObservationEntry {
        timestamp: now,
        tick_cumulative: new_cumulative,
    };
    let next_index = write_index
        .checked_add(1)
        .expect("write_index + 1 fits in usize")
        .checked_rem(capacity)
        .expect("capacity is non-zero");
    observations.write_index = u32::try_from(next_index).expect("next write_index fits in u32");
    observations.total_entries = observations
        .total_entries
        .checked_add(1)
        .expect("total_entries does not overflow");
    observations.last_recorded_tick = tick;

    Some(observations)
}

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
/// `sqrtPriceX96^2` is computed with `full_math::mul_div` using a 512-bit intermediate, so it
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

impl TryFrom<&ShardData> for CurrentTickAccount {
    type Error = std::io::Error;

    fn try_from(data: &ShardData) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&CurrentTickAccount> for ShardData {
    fn from(account: &CurrentTickAccount) -> Self {
        let serialized_len =
            borsh::object_length(account).expect("CurrentTickAccount length must be known");
        let mut data = Vec::with_capacity(serialized_len);
        BorshSerialize::serialize(account, &mut data)
            .expect("Serialization to Vec should not fail");
        Self::try_from(data).expect("CurrentTickAccount encoded data should fit into ShardData")
    }
}

const CURRENT_TICK_ACCOUNT_PDA_SEED: &[u8] = b"CURRENT_TICK_ACCOUNT";

/// Derives the [`AccountId`] for a price source's [`CurrentTickAccount`] PDA.
#[must_use]
pub fn compute_current_tick_account_pda(
    oracle_program_id: AccountId,
    price_source_id: AccountId,
) -> AccountId {
    AccountId::for_public_pda(
        &oracle_program_id,
        &compute_current_tick_account_pda_seed(price_source_id),
    )
}

/// Derives the [`PdaSeed`] for a price source's [`CurrentTickAccount`].
///
/// Hash input: `price_source_id (32 bytes) || CURRENT_TICK_ACCOUNT_PDA_SEED`.
#[must_use]
pub fn compute_current_tick_account_pda_seed(price_source_id: AccountId) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&price_source_id.to_bytes());
    bytes.extend_from_slice(CURRENT_TICK_ACCOUNT_PDA_SEED);

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
