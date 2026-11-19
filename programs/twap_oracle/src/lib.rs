//! The TWAP oracle on LEZ's actor interface.
//!
//! A handler sees one actor: a current-tick actor, an observations actor, a price actor, or an
//! empty one. Recording and publishing are sync chains from the current-tick actor to the
//! observations actor (and on to the price actor) that carry the tick forward. The current-tick
//! actor reads the clock first, parking the operation in its state until the reply arrives.
//!
//! Trust rests on the sender:
//!
//! - the clock's reply is accepted only from `clock_actor()`;
//! - [`Internal`] messages only from oracle actors, and each receiver checks that the sender is
//!   its own current-tick or observations actor;
//! - source messages only from the [`PriceSource`] stored at creation.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
pub use twap_oracle_core as core;
use twap_oracle_core::{
    compute_price_observations_pda, CurrentTickState, Internal, Message, OraclePrice, OracleState,
    PriceObservations, PriceSource, ReadPrice, OBSERVATIONS_CAPACITY,
};

mod clock;
mod create_current_tick_account;
mod create_price_accounts;
mod publish_price;
mod read_price;
mod record_tick;
#[cfg(test)]
mod test_utils;
mod update_current_tick;

/// The guest entry point. Each kind of sender has one format: the clock's reply, [`Internal`]
/// from oracle actors, then [`ReadPrice`] or a [`Message`] from anyone else.
pub fn handle_message(input: &ReceiveInput) -> Response {
    if input.from == Some(clock_core::clock_actor()) {
        return clock::on_reply(input);
    }
    if input.from_own_program() {
        let from = input.from.expect("an internal message always has a sender");
        let message: Internal =
            borsh::from_slice(&input.message).expect("internal oracle message must decode");
        return match message {
            Internal::InitObservations {
                window_duration,
                price_source_id,
                tick,
                now,
            } => create_price_accounts::init_observations(
                input,
                from,
                window_duration,
                price_source_id,
                tick,
                now,
            ),
            Internal::InitPrice {
                window_duration,
                price_source_id,
                base_asset,
                quote_asset,
                price,
                now,
            } => create_price_accounts::init_price(
                input,
                from,
                create_price_accounts::InitPrice {
                    window_duration,
                    price_source_id,
                    base_asset,
                    quote_asset,
                    price,
                    now,
                },
            ),
            Internal::SampleTick { tick, now } => record_tick::sample_tick(input, from, tick, now),
            Internal::ComputePrice {
                tick,
                last_updated,
                now,
            } => publish_price::compute_price(input, from, tick, last_updated, now),
            Internal::SetPrice {
                current_tick,
                window_duration,
                price,
                now,
            } => publish_price::set_price(input, from, current_tick, window_duration, price, now),
        };
    }
    if ReadPrice::matches(&input.message) {
        return read_price::read_price(input);
    }
    let message: Message = borsh::from_slice(&input.message).expect("oracle message must decode");
    match message {
        Message::CreateCurrentTickAccount { initial_price } => {
            create_current_tick_account::create_current_tick_account(input, initial_price)
        }
        Message::UpdateCurrentTick { price } => {
            update_current_tick::update_current_tick(input, price)
        }
        Message::CreatePriceAccounts {
            window_duration,
            base_asset,
            quote_asset,
        } => create_price_accounts::create_price_accounts(
            input,
            window_duration,
            base_asset,
            quote_asset,
        ),
        Message::RecordTick { window_duration } => record_tick::record_tick(input, window_duration),
        Message::PublishPrice { window_duration } => {
            publish_price::publish_price(input, window_duration)
        }
    }
}

/// The oracle's actor on `account_id`.
fn oracle_actor(input: &ReceiveInput, account_id: AccountId) -> Actor {
    Actor::new(account_id, input.receiver.program_account_id)
}

/// From the current-tick actor: its observations actor for `window_duration`.
fn observations_actor(input: &ReceiveInput, window_duration: u64) -> Actor {
    oracle_actor(
        input,
        compute_price_observations_pda(
            input.receiver.program_account_id,
            input.receiver.account_id,
            window_duration,
        ),
    )
}

fn write(state: OracleState) -> Response {
    Response::set_state(state.encode())
}

fn current_tick_state(input: &ReceiveInput) -> CurrentTickState {
    match OracleState::decode(&input.pre_state) {
        Some(OracleState::CurrentTick(state)) => state,
        Some(OracleState::PriceObservations(_) | OracleState::Price(_)) | None => {
            panic!("receiver is not a current-tick actor")
        }
    }
}

fn observations_state(input: &ReceiveInput) -> PriceObservations {
    match OracleState::decode(&input.pre_state) {
        Some(OracleState::PriceObservations(state)) => state,
        Some(OracleState::CurrentTick(_) | OracleState::Price(_)) | None => {
            panic!("receiver is not an observations actor")
        }
    }
}

fn price_state(input: &ReceiveInput) -> OraclePrice {
    match OracleState::decode(&input.pre_state) {
        Some(OracleState::Price(state)) => state,
        Some(OracleState::CurrentTick(_) | OracleState::PriceObservations(_)) | None => {
            panic!("receiver is not a price actor")
        }
    }
}

/// Panics unless the sender is `source`: its actor for a program, or a root message signed for
/// the key's own account. A key's signature reaching a nested call doesn't count.
fn assert_source(input: &ReceiveInput, source: &PriceSource) {
    match source.program_actor() {
        Some(actor) => assert!(input.from == Some(actor), "sender is not the price source"),
        None => assert!(
            input.from.is_none()
                && input.is_authorized
                && input.receiver.account_id == source.account_id(),
            "the price source must sign the transaction's root message"
        ),
    }
}

/// `RecordTick` and `PublishPrice` run only as the transaction's root message.
fn assert_root(input: &ReceiveInput, operation: &str) {
    assert!(
        input.from.is_none(),
        "{operation} must be the transaction's root message"
    );
}

/// The minimum time between two observations. Floored to 1 so a degenerate window can't let
/// same-timestamp writes through.
fn min_interval(window_duration: u64) -> u64 {
    window_duration
        .checked_div(u64::from(OBSERVATIONS_CAPACITY))
        .expect("OBSERVATIONS_CAPACITY is non-zero")
        .max(1)
}

fn capacity() -> usize {
    usize::try_from(OBSERVATIONS_CAPACITY).expect("OBSERVATIONS_CAPACITY fits in usize")
}

/// The index of the newest entry, the one before `write_index`.
fn newest_index(observations: &PriceObservations) -> usize {
    let write_index = usize::try_from(observations.write_index).expect("write_index fits in usize");
    write_index
        .checked_sub(1)
        .unwrap_or_else(|| capacity().checked_sub(1).expect("capacity is non-zero"))
}
