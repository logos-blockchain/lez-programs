//! Core types for the TWAP oracle: actor state, messages, the standard price read, addresses
//! and events.
//!
//! Three actors serve a price source: its current-tick actor, and per window an observations
//! actor and a price actor.
//!
//! State and messages outlive code (state stays on accounts, async messages in mailboxes),
//! and Borsh encodes an enum variant as a leading tag byte. Every enum here is therefore
//! append-only: a changed layout is a new variant, never an edit of an existing one.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Actor},
    program::PdaSeed,
};
use risc0_zkvm::sha::{Impl, Sha256 as _};
use serde::{Deserialize, Serialize};

pub mod event;
mod price;

pub use price::{price_to_tick, tick_to_oracle_price, PRICE_FRACTIONAL_BITS};

/// Maximum tick delta injected into the accumulator per observation.
///
/// Matches the Uniswap v4 truncated oracle hook reference value (~2.39× price move per block).
/// An attacker who moves the pool by more than this in one block still only injects
/// `MAX_TICK_DELTA` ticks into the cumulative — they must sustain the manipulation across
/// many blocks while arbitrage erodes their position.
pub const MAX_TICK_DELTA: i32 = 9_116;

/// Number of entries in each observations ring buffer.
///
/// Bounded by cost, not storage: every `SampleTick` and `ComputePrice` loads the observations
/// actor, so cost scales with its size. See the cycle benchmark (`programs/benchmark`).
///
/// Sets the resolution: `min_interval = window_duration / OBSERVATIONS_CAPACITY`.
pub const OBSERVATIONS_CAPACITY: u32 = 2048;

/// Maximum number of sampling intervals `PublishPrice` extrapolates past the newest stored
/// observation before refusing to publish.
///
/// The published timestamp is `now`, the only freshness signal consumers have, so without a
/// bound an unattended feed would publish mostly extrapolation that still looks fresh. In
/// intervals rather than milliseconds, so the bound scales with the window.
pub const MAX_TAIL_INTERVALS: u64 = 4;

// ──────────────────────────────────────────────────────────────────────────────
// State
// ──────────────────────────────────────────────────────────────────────────────

/// The oracle's state on one account.
///
/// An empty actor state means "nothing here yet" and is not an `OracleState`.
#[spel_attrs::state]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum OracleState {
    CurrentTick(CurrentTickState),
    PriceObservations(PriceObservations),
    /// The price actor holds the standard price and nothing else.
    Price(OraclePrice),
}

impl OracleState {
    /// Decodes an actor state; `None` for an empty one.
    ///
    /// # Panics
    /// If the bytes are not an `OracleState`. Only oracle code writes oracle actor state.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }
        Some(borsh::from_slice(bytes).expect("oracle actor state must decode"))
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("borsh serialization is infallible")
    }
}

/// Who may create and update a current-tick actor.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub enum PriceSource {
    /// A program's actor, e.g. `(pool, amm)`, accepted as the sender. The current-tick actor
    /// is that program's PDA, `current_tick_seed(account_id)`.
    ///
    /// The actor's two ids, not an `Actor`: the IDL generator knows `AccountId` only.
    Program {
        account_id: AccountId,
        program_id: AccountId,
    },
    /// A key-owned account, accepted on a root message it signed. The current-tick actor lives
    /// on this account.
    Key(AccountId),
}

impl PriceSource {
    #[must_use]
    pub const fn program(actor: Actor) -> Self {
        Self::Program {
            account_id: actor.account_id,
            program_id: actor.program_account_id,
        }
    }

    /// The source's account: the pool or the key.
    #[must_use]
    pub const fn account_id(&self) -> AccountId {
        match self {
            Self::Program { account_id, .. } | Self::Key(account_id) => *account_id,
        }
    }

    /// The program source's actor, the sender its current-tick actor accepts.
    #[must_use]
    pub const fn program_actor(&self) -> Option<Actor> {
        match self {
            Self::Program {
                account_id,
                program_id,
            } => Some(Actor::new(*account_id, *program_id)),
            Self::Key(_) => None,
        }
    }

    /// The account of this source's current-tick actor.
    #[must_use]
    pub fn current_tick_account_id(&self) -> AccountId {
        match self {
            Self::Program {
                account_id,
                program_id,
            } => AccountId::for_public_pda(
                program_id,
                &compute_current_tick_account_pda_seed(*account_id),
            ),
            Self::Key(account_id) => *account_id,
        }
    }
}

/// A price source's live tick, shared by all its windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct CurrentTickState {
    pub source: PriceSource,
    /// The tick of the source's latest price, `price_to_tick(price)`.
    pub tick: i32,
    /// Clock time of the latest `UpdateCurrentTick`; 0 until the first.
    pub last_updated: u64,
    /// The operation waiting for the clock's reply; `None` between transactions.
    pub pending: Option<Pending>,
}

/// An operation the current-tick actor parked while it reads the clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum Pending {
    UpdateCurrentTick {
        price: u128,
    },
    CreatePriceAccounts {
        window_duration: u64,
        base_asset: AccountId,
        quote_asset: AccountId,
    },
    RecordTick {
        window_duration: u64,
    },
    PublishPrice {
        window_duration: u64,
    },
}

/// A single entry in a [`PriceObservations`] ring buffer.
#[derive(
    Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct ObservationEntry {
    /// Clock time (milliseconds) when this entry was recorded.
    pub timestamp: u64,
    /// Running sum of `tick × elapsed_ms` up to this entry.
    ///
    /// The TWAP over `[t1, t2]` is `(tick_cumulative[t2] - tick_cumulative[t1]) / (t2 - t1)`.
    pub tick_cumulative: i64,
}

/// The observations ring buffer of one price source and window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PriceObservations {
    /// The source's account: the pool or the key.
    pub price_source_id: AccountId,
    /// The current-tick actor's account: the only sender of `SampleTick` and `ComputePrice`.
    pub current_tick: AccountId,
    /// The TWAP window in milliseconds; sets the sampling interval.
    pub window_duration: u64,
    /// Index of the *next* slot to write (wraps at `OBSERVATIONS_CAPACITY`).
    pub write_index: u32,
    /// Total entries ever appended (never resets; tells a partial buffer from a full one).
    pub total_entries: u64,
    /// The tick most recently *recorded*: the clamped value, not the source's raw tick.
    ///
    /// Storing the clamped value makes [`MAX_TICK_DELTA`] a bound on the accumulator: the
    /// recorded tick walks toward the true price at `MAX_TICK_DELTA` per observation.
    pub last_recorded_tick: i32,
    /// Always exactly `OBSERVATIONS_CAPACITY` entries.
    pub entries: Vec<ObservationEntry>,
}

/// LEZ's standard price shape: a price actor's state, and what consumers decode from a
/// [`PriceReply`].
///
/// Generic so that any producer (TWAP, external adaptor, aggregator) can serve it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct OraclePrice {
    /// Canonical identifier for the priced asset.
    pub base_asset: AccountId,
    /// Canonical identifier for the quote asset that denominates `price`.
    pub quote_asset: AccountId,
    /// Amount of `quote_asset` one unit of `base_asset` is worth, as `Q64.64`
    /// (`price / 2^PRICE_FRACTIONAL_BITS`). Zero is rejected on read.
    pub price: u128,
    /// Price observation timestamp. Consumers choose the time unit by matching this with
    /// `max_age`.
    pub timestamp: u64,
    /// Identifier of the source that populated this price; for the TWAP, the price source's
    /// account.
    pub source_id: AccountId,
    /// Source-provided confidence interval, or zero when the source does not provide one.
    pub confidence_interval: u128,
}

// ──────────────────────────────────────────────────────────────────────────────
// Messages
// ──────────────────────────────────────────────────────────────────────────────

/// Messages anyone may send: a transaction author or another program. Each is refused from
/// anyone but the sender it names.
#[spel_attrs::message]
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Message {
    /// Create the current-tick actor of a price source, at tick `price_to_tick(initial_price)`.
    ///
    /// Sent to the empty actor by the source: a program's actor (with the current-tick seed,
    /// which admits the account), or a key at root, signing for its own account.
    CreateCurrentTickAccount {
        /// Opening spot price as `Q64.64` (`quote_asset` per `base_asset`).
        initial_price: u128,
    },

    /// From the source: set the tick to `price_to_tick(price)`, stamped with the clock.
    UpdateCurrentTick {
        /// New spot price as `Q64.64`.
        price: u128,
    },

    /// From the source: create the observations and price actors of a window, both seeded
    /// from the current tick.
    CreatePriceAccounts {
        /// The TWAP window in milliseconds; at least `OBSERVATIONS_CAPACITY`.
        window_duration: u64,
        base_asset: AccountId,
        quote_asset: AccountId,
    },

    /// At root, to the current-tick actor: append the current tick to the window's ring
    /// buffer, if a sampling interval has passed since the last entry.
    RecordTick { window_duration: u64 },

    /// At root, to the current-tick actor: publish the window's TWAP to its price actor.
    PublishPrice { window_duration: u64 },
}

/// Messages only oracle actors send each other. The guest decodes these only when the sender
/// is the oracle program itself, so each is trusted because only oracle code can have
/// produced it. `now` is the clock time the current-tick actor read.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Internal {
    /// To an empty observations actor, from its current-tick actor: entry 0 at `now`, and
    /// `last_recorded_tick = tick`.
    InitObservations {
        window_duration: u64,
        price_source_id: AccountId,
        tick: i32,
        now: u64,
    },
    /// To an empty price actor, from its current-tick actor: the first price.
    InitPrice {
        window_duration: u64,
        price_source_id: AccountId,
        base_asset: AccountId,
        quote_asset: AccountId,
        price: u128,
        now: u64,
    },
    /// To the observations actor, from its current-tick actor: record `tick` at `now`.
    SampleTick { tick: i32, now: u64 },
    /// To the observations actor, from its current-tick actor: compute the TWAP up to `now`
    /// and send it to the price actor.
    ComputePrice {
        tick: i32,
        last_updated: u64,
        now: u64,
    },
    /// To the price actor, from its observations actor: the new price. `current_tick` and
    /// `window_duration` derive both actors' addresses, which the price actor checks.
    SetPrice {
        current_tick: AccountId,
        window_duration: u64,
        price: u128,
        now: u64,
    },
}

// ──────────────────────────────────────────────────────────────────────────────
// The standard price read
// ──────────────────────────────────────────────────────────────────────────────

/// Asks a price actor for its price; it replies with a [`PriceReply`].
///
/// Fixed bytes, [`ReadPrice::BYTES`], independent of any producer's own messages, so a
/// consumer can read any producer that answers it. No oracle `Message` encodes to 8 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadPrice;

impl ReadPrice {
    /// `sha256("lez_oracle::ReadPrice")[..8]`.
    pub const BYTES: [u8; 8] = [79, 122, 236, 94, 108, 240, 101, 249];

    /// Whether a message is `ReadPrice`.
    #[must_use]
    pub fn matches(message: &[u8]) -> bool {
        message == Self::BYTES
    }
}

impl BorshSerialize for ReadPrice {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&Self::BYTES)
    }
}

impl BorshDeserialize for ReadPrice {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut bytes = [0_u8; 8];
        reader.read_exact(&mut bytes)?;
        if bytes == Self::BYTES {
            Ok(Self)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "not a ReadPrice",
            ))
        }
    }
}

/// A price actor's sync reply to [`ReadPrice`]. Accept it only from the price actor you
/// pinned, account and program.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PriceReply {
    pub price: OraclePrice,
}

// ──────────────────────────────────────────────────────────────────────────────
// Addresses
// ──────────────────────────────────────────────────────────────────────────────

const CURRENT_TICK_ACCOUNT_PDA_SEED: &[u8] = b"CURRENT_TICK_ACCOUNT";
const PRICE_OBSERVATIONS_PDA_SEED: &[u8] = b"PRICE_OBSERVATIONS";
const ORACLE_PRICE_ACCOUNT_PDA_SEED: &[u8] = b"ORACLE_PRICE_ACCOUNT";

/// The seed a program source derives its current-tick actor's account from, as its own PDA.
///
/// Hash input: `price_source_id (32 bytes) || CURRENT_TICK_ACCOUNT_PDA_SEED`.
#[must_use]
pub fn compute_current_tick_account_pda_seed(price_source_id: AccountId) -> PdaSeed {
    hash_seed(&[&price_source_id.to_bytes(), CURRENT_TICK_ACCOUNT_PDA_SEED])
}

/// The observations actor's account, an oracle PDA of the current-tick actor and the window.
#[must_use]
pub fn compute_price_observations_pda(
    oracle_program_id: AccountId,
    current_tick_id: AccountId,
    window_duration: u64,
) -> AccountId {
    AccountId::for_public_pda(
        &oracle_program_id,
        &compute_price_observations_pda_seed(current_tick_id, window_duration),
    )
}

/// Hash input: `current_tick_id (32 bytes) || window_duration_le (8 bytes) ||
/// PRICE_OBSERVATIONS_PDA_SEED`.
#[must_use]
pub fn compute_price_observations_pda_seed(
    current_tick_id: AccountId,
    window_duration: u64,
) -> PdaSeed {
    hash_seed(&[
        &current_tick_id.to_bytes(),
        &window_duration.to_le_bytes(),
        PRICE_OBSERVATIONS_PDA_SEED,
    ])
}

/// The price actor's account, an oracle PDA of the current-tick actor and the window.
#[must_use]
pub fn compute_oracle_price_account_pda(
    oracle_program_id: AccountId,
    current_tick_id: AccountId,
    window_duration: u64,
) -> AccountId {
    AccountId::for_public_pda(
        &oracle_program_id,
        &compute_oracle_price_account_pda_seed(current_tick_id, window_duration),
    )
}

/// Hash input: `current_tick_id (32 bytes) || window_duration_le (8 bytes) ||
/// ORACLE_PRICE_ACCOUNT_PDA_SEED`.
#[must_use]
pub fn compute_oracle_price_account_pda_seed(
    current_tick_id: AccountId,
    window_duration: u64,
) -> PdaSeed {
    hash_seed(&[
        &current_tick_id.to_bytes(),
        &window_duration.to_le_bytes(),
        ORACLE_PRICE_ACCOUNT_PDA_SEED,
    ])
}

fn hash_seed(parts: &[&[u8]]) -> PdaSeed {
    let bytes = parts.concat();
    PdaSeed::new(
        *Impl::hash_bytes(&bytes)
            .as_bytes()
            .first_chunk::<32>()
            .expect("a sha256 digest has 32 bytes"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const POOL: AccountId = AccountId::new([1; 32]);
    const AMM: AccountId = AccountId::new([2; 32]);
    const KEY: AccountId = AccountId::new([3; 32]);
    const ORACLE: AccountId = AccountId::new([4; 32]);

    #[test]
    fn read_price_bytes_are_the_documented_hash() {
        let digest = Impl::hash_bytes(b"lez_oracle::ReadPrice");
        assert_eq!(
            digest.as_bytes().first_chunk::<8>(),
            Some(&ReadPrice::BYTES)
        );
    }

    #[test]
    fn read_price_round_trips_and_rejects_other_bytes() {
        let bytes = borsh::to_vec(&ReadPrice).unwrap();
        assert!(ReadPrice::matches(&bytes));
        assert_eq!(borsh::from_slice::<ReadPrice>(&bytes).unwrap(), ReadPrice);
        assert!(borsh::from_slice::<ReadPrice>(&[0; 8]).is_err());
    }

    #[test]
    fn no_message_is_mistaken_for_read_price() {
        let messages = [
            Message::CreateCurrentTickAccount { initial_price: 0 },
            Message::UpdateCurrentTick { price: 0 },
            Message::CreatePriceAccounts {
                window_duration: 0,
                base_asset: POOL,
                quote_asset: POOL,
            },
            Message::RecordTick { window_duration: 0 },
            Message::PublishPrice { window_duration: 0 },
        ];
        for message in messages {
            let bytes = borsh::to_vec(&message).unwrap();
            assert_ne!(bytes.len(), ReadPrice::BYTES.len(), "{message:?}");
        }
    }

    #[test]
    fn a_program_source_derives_its_current_tick_actor_from_its_own_program() {
        let source = PriceSource::program(Actor::new(POOL, AMM));
        let expected =
            AccountId::for_public_pda(&AMM, &compute_current_tick_account_pda_seed(POOL));
        assert_eq!(source.current_tick_account_id(), expected);
        assert_eq!(source.account_id(), POOL);
        assert_eq!(source.program_actor(), Some(Actor::new(POOL, AMM)));
    }

    #[test]
    fn a_program_source_encodes_like_its_actor() {
        let actor = Actor::new(POOL, AMM);
        let source = borsh::to_vec(&PriceSource::program(actor)).unwrap();
        assert_eq!(source.first(), Some(&0));
        assert_eq!(
            source.get(1..),
            Some(borsh::to_vec(&actor).unwrap().as_slice())
        );
    }

    #[test]
    fn a_key_source_hosts_its_current_tick_actor() {
        let source = PriceSource::Key(KEY);
        assert_eq!(source.current_tick_account_id(), KEY);
        assert_eq!(source.account_id(), KEY);
        assert_eq!(source.program_actor(), None);
    }

    #[test]
    fn window_actors_are_distinct_per_window_and_kind() {
        let current_tick = PriceSource::program(Actor::new(POOL, AMM)).current_tick_account_id();
        let observations = compute_price_observations_pda(ORACLE, current_tick, 3_600_000);
        let price = compute_oracle_price_account_pda(ORACLE, current_tick, 3_600_000);
        assert_ne!(observations, price);
        assert_ne!(
            observations,
            compute_price_observations_pda(ORACLE, current_tick, 86_400_000)
        );
        assert_ne!(
            observations,
            compute_price_observations_pda(ORACLE, KEY, 3_600_000)
        );
    }

    #[test]
    fn state_round_trips_and_empty_is_none() {
        let state = OracleState::CurrentTick(CurrentTickState {
            source: PriceSource::Key(KEY),
            tick: -5,
            last_updated: 0,
            pending: Some(Pending::RecordTick {
                window_duration: 3_600_000,
            }),
        });
        assert_eq!(OracleState::decode(&state.encode()), Some(state));
        assert_eq!(OracleState::decode(&[]), None);
    }
}
