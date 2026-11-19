//! Fixtures for the handler tests: one hand-built `ReceiveInput` per message, compared against
//! the expected `Response`. Conversations between actors are covered by the integration tests.

pub(crate) use borsh::BorshSerialize;
pub(crate) use clock_core::Reply;
pub(crate) use lee_core::{
    account::{AccountId, Actor, ActorState},
    program::{ReceiveInput, Response, SyncMessage},
};
pub(crate) use twap_oracle_core::{
    compute_oracle_price_account_pda, compute_oracle_price_account_pda_seed,
    compute_price_observations_pda, compute_price_observations_pda_seed,
    event::{CurrentTickAccountCreated, OracleEvent, PriceAccountsCreated, Published, Recorded},
    price_to_tick, tick_to_oracle_price, CurrentTickState, Internal, Message, ObservationEntry,
    OraclePrice, OracleState, Pending, PriceObservations, PriceReply, PriceSource, ReadPrice,
    MAX_TAIL_INTERVALS, MAX_TICK_DELTA, OBSERVATIONS_CAPACITY,
};

use crate::handle_message;

pub(crate) const ORACLE: AccountId = AccountId::new([0xAA; 32]);
pub(crate) const AMM: AccountId = AccountId::new([0xBB; 32]);
pub(crate) const OTHER_PROGRAM: AccountId = AccountId::new([0xCC; 32]);

pub(crate) const POOL: AccountId = AccountId::new([1; 32]);
pub(crate) const KEY: AccountId = AccountId::new([2; 32]);
pub(crate) const BASE: AccountId = AccountId::new([3; 32]);
pub(crate) const QUOTE: AccountId = AccountId::new([4; 32]);
pub(crate) const READER: AccountId = AccountId::new([5; 32]);

/// `1.0` as `Q64.64`, tick 0.
pub(crate) const ONE: u128 = 1 << 64;
/// A window whose sampling interval is 1000 ms.
pub(crate) const WINDOW: u64 = 2_048_000;
pub(crate) const MIN_INTERVAL: u64 = 1_000;

pub(crate) fn oracle(account_id: AccountId) -> Actor {
    Actor::new(account_id, ORACLE)
}

/// An actor of another program.
pub(crate) fn outsider(account_id: AccountId) -> Actor {
    Actor::new(account_id, OTHER_PROGRAM)
}

pub(crate) fn clock() -> Actor {
    clock_core::clock_actor()
}

/// The AMM pool's actor, the program source in these tests.
pub(crate) fn pool() -> Actor {
    Actor::new(POOL, AMM)
}

pub(crate) fn source() -> PriceSource {
    PriceSource::program(pool())
}

pub(crate) fn current_tick_id() -> AccountId {
    source().current_tick_account_id()
}

pub(crate) fn observations_id() -> AccountId {
    compute_price_observations_pda(ORACLE, current_tick_id(), WINDOW)
}

pub(crate) fn price_id() -> AccountId {
    compute_oracle_price_account_pda(ORACLE, current_tick_id(), WINDOW)
}

pub(crate) fn run(
    receiver: AccountId,
    from: Option<Actor>,
    is_authorized: bool,
    pre: Option<&OracleState>,
    message: &impl BorshSerialize,
) -> Response {
    let input = ReceiveInput {
        receiver: oracle(receiver),
        from,
        is_authorized,
        pre_state: ActorState::from(pre.map(OracleState::encode).unwrap_or_default()),
        message: borsh::to_vec(message).expect("test message encodes"),
    };
    handle_message(&input)
}

pub(crate) fn clock_reply(receiver: AccountId, pre: &OracleState, now: u64) -> Response {
    run(
        receiver,
        Some(clock()),
        false,
        Some(pre),
        &Reply::Timestamp(now),
    )
}

pub(crate) fn write(state: &OracleState) -> Response {
    Response::set_state(state.encode())
}

/// What a current-tick actor returns when it parks an operation: its new state and the read.
pub(crate) fn reads_clock(state: &OracleState) -> Response {
    write(state).send_sync(
        clock(),
        &clock_core::Message::ReadTimestamp { granularity: 1 },
    )
}

/// The state a response writes.
pub(crate) fn written(response: &Response) -> OracleState {
    let state = response
        .post_state
        .as_ref()
        .expect("response writes a state");
    OracleState::decode(state).expect("written state is not empty")
}

pub(crate) fn current_tick(tick: i32, last_updated: u64, pending: Option<Pending>) -> OracleState {
    current_tick_of(source(), tick, last_updated, pending)
}

pub(crate) fn current_tick_of(
    source: PriceSource,
    tick: i32,
    last_updated: u64,
    pending: Option<Pending>,
) -> OracleState {
    OracleState::CurrentTick(CurrentTickState {
        source,
        tick,
        last_updated,
        pending,
    })
}

/// A ring buffer holding `entries` (timestamp, cumulative) from slot 0.
pub(crate) fn buffer(entries: &[(u64, i64)], last_recorded_tick: i32) -> PriceObservations {
    let capacity = usize::try_from(OBSERVATIONS_CAPACITY).unwrap();
    let mut slots = vec![ObservationEntry::default(); capacity];
    for (slot, &(timestamp, tick_cumulative)) in slots.iter_mut().zip(entries) {
        *slot = ObservationEntry {
            timestamp,
            tick_cumulative,
        };
    }
    PriceObservations {
        price_source_id: POOL,
        current_tick: current_tick_id(),
        window_duration: WINDOW,
        write_index: u32::try_from(entries.len().checked_rem(capacity).unwrap()).unwrap(),
        total_entries: u64::try_from(entries.len()).unwrap(),
        last_recorded_tick,
        entries: slots,
    }
}

pub(crate) fn observations_at(entries: &[(u64, i64)], last_recorded_tick: i32) -> OracleState {
    OracleState::PriceObservations(buffer(entries, last_recorded_tick))
}

pub(crate) fn oracle_price(price: u128, timestamp: u64) -> OraclePrice {
    OraclePrice {
        base_asset: BASE,
        quote_asset: QUOTE,
        price,
        timestamp,
        source_id: POOL,
        confidence_interval: 0,
    }
}

pub(crate) fn price(price: u128, timestamp: u64) -> OracleState {
    OracleState::Price(oracle_price(price, timestamp))
}

/// `SetPrice` naming the test window.
pub(crate) fn set_price_message(price: u128, now: u64) -> Internal {
    Internal::SetPrice {
        current_tick: current_tick_id(),
        window_duration: WINDOW,
        price,
        now,
    }
}
