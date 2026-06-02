#![cfg_attr(not(test), no_main)]

use nssa_core::account::{AccountId, AccountWithMetadata};
use spel_framework::context::ProgramContext;
use spel_framework::prelude::*;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "twap_oracle_core::Instruction")]
mod twap_oracle {
    #[allow(unused_imports)]
    use super::*;

    /// Creates and initialises a price observations account for a price source and time window.
    ///
    /// Expected accounts:
    /// 1. `price_observations` — uninitialized PDA owned by this oracle program.
    /// 2. `price_source` — account the caller controls (proven via `is_authorized = true`);
    ///    its ID is used as the observations identifier and to derive the price observations PDA.
    /// 3. `clock` — read-only LEZ clock account (any cadence).
    #[instruction]
    pub fn create_price_observations(
        ctx: ProgramContext,
        price_observations: AccountWithMetadata,
        price_source: AccountWithMetadata,
        clock: AccountWithMetadata,
        initial_tick: i32,
        window_duration: u64,
    ) -> SpelResult {
        let post_states =
            twap_oracle_program::create_price_observations::create_price_observations(
                price_observations,
                price_source,
                clock,
                initial_tick,
                window_duration,
                ctx.self_program_id,
            );
        Ok(spel_framework::SpelOutput::execute(post_states, vec![]))
    }

    /// Creates and initialises a canonical oracle price account for a price source and time
    /// window.
    ///
    /// Expected accounts:
    /// 1. `oracle_price_account` — uninitialized PDA owned by this oracle program.
    /// 2. `price_source` — account the caller controls (proven via `is_authorized = true`);
    ///    its ID ties this price account to the same source as the corresponding
    ///    `PriceObservations` account for the same window.
    /// 3. `clock` — canonical 1-block LEZ clock account; supplies the initial timestamp.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit price account, source, and clock accounts alongside the asset pair, initial price, and window"
    )]
    #[instruction]
    pub fn create_oracle_price_account(
        ctx: ProgramContext,
        oracle_price_account: AccountWithMetadata,
        price_source: AccountWithMetadata,
        clock: AccountWithMetadata,
        base_asset: AccountId,
        quote_asset: AccountId,
        initial_price: u128,
        window_duration: u64,
    ) -> SpelResult {
        let post_states =
            twap_oracle_program::create_oracle_price_account::create_oracle_price_account(
                oracle_price_account,
                price_source,
                clock,
                base_asset,
                quote_asset,
                initial_price,
                window_duration,
                ctx.self_program_id,
            );
        Ok(spel_framework::SpelOutput::execute(post_states, vec![]))
    }

    /// Creates and initialises a current tick account for a price source.
    ///
    /// Expected accounts:
    /// 1. `current_tick_account` — uninitialized PDA owned by this oracle program.
    /// 2. `price_source` — account the caller controls (proven via `is_authorized = true`).
    /// 3. `clock` — read-only LEZ clock account.
    ///
    /// `initial_price` is a `Q64.64` spot price; the oracle converts it to a tick.
    #[instruction]
    pub fn create_current_tick_account(
        ctx: ProgramContext,
        current_tick_account: AccountWithMetadata,
        price_source: AccountWithMetadata,
        clock: AccountWithMetadata,
        initial_price: u128,
    ) -> SpelResult {
        let post_states =
            twap_oracle_program::create_current_tick_account::create_current_tick_account(
                current_tick_account,
                price_source,
                clock,
                initial_price,
                ctx.self_program_id,
            );
        Ok(spel_framework::SpelOutput::execute(post_states, vec![]))
    }

    /// Records the current tick into a price observations ring buffer.
    ///
    /// Expected accounts:
    /// 1. `price_observations` — initialized PDA owned by this oracle program.
    /// 2. `current_tick_account` — initialized PDA owned by this oracle program.
    /// 3. `clock` — read-only LEZ clock account.
    #[instruction]
    pub fn record_tick(
        ctx: ProgramContext,
        price_observations: AccountWithMetadata,
        current_tick_account: AccountWithMetadata,
        clock: AccountWithMetadata,
        price_source_id: AccountId,
        window_duration: u64,
    ) -> SpelResult {
        let post_states = twap_oracle_program::record_tick::record_tick(
            price_observations,
            current_tick_account,
            clock,
            price_source_id,
            window_duration,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, vec![]))
    }

    /// Updates the tick stored in an existing current tick account.
    ///
    /// Expected accounts:
    /// 1. `current_tick_account` — initialized PDA owned by this oracle program.
    /// 2. `price_source` — account the caller controls (proven via `is_authorized = true`).
    /// 3. `clock` — read-only LEZ clock account.
    ///
    /// `price` is a `Q64.64` spot price; the oracle converts it to a tick.
    #[instruction]
    pub fn update_current_tick(
        ctx: ProgramContext,
        current_tick_account: AccountWithMetadata,
        price_source: AccountWithMetadata,
        clock: AccountWithMetadata,
        price: u128,
    ) -> SpelResult {
        let post_states = twap_oracle_program::update_current_tick::update_current_tick(
            current_tick_account,
            price_source,
            clock,
            price,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, vec![]))
    }
}
