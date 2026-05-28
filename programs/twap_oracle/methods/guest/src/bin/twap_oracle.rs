#![cfg_attr(not(test), no_main)]

use nssa_core::account::AccountWithMetadata;
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
}
