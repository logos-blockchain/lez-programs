#![no_main]

use nssa_core::account::AccountWithMetadata;
use spel_framework::context::ProgramContext;
use spel_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "stablecoin_core::Instruction")]
mod stablecoin {
    #[allow(unused_imports)]
    use super::*;

    /// Open a new collateral-only position for the calling owner.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if any precondition fails (see
    /// [`stablecoin_program::open_position::open_position`] for the full list).
    #[instruction]
    pub fn open_position(
        ctx: ProgramContext,
        owner: AccountWithMetadata,
        position: AccountWithMetadata,
        vault: AccountWithMetadata,
        user_holding: AccountWithMetadata,
        token_definition: AccountWithMetadata,
        collateral_amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) = stablecoin_program::open_position::open_position(
            owner,
            position,
            vault,
            user_holding,
            token_definition,
            ctx.self_program_id,
            collateral_amount,
        );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }
}
