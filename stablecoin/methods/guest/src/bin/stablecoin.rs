//! RISC Zero guest binary for the Stablecoin Program.
//!
//! Wires the host-side `stablecoin_program` instruction handlers to the LEZ framework via
//! `#[lez_program]` so the entry points can be invoked from a deployed program.

#![no_main]
#![allow(
    missing_docs,
    reason = "lez_program / instruction proc macros emit module, function, and constant items \
              that do not carry doc strings; user-written handlers document inline"
)]

use nssa_core::{account::AccountWithMetadata, program::ProgramId};
use spel_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "stablecoin_core::Instruction")]
mod stablecoin {
    #[allow(
        unused_imports,
        reason = "lez_program expansion may or may not reference every super:: import"
    )]
    use super::*;

    /// Heartbeat instruction that returns the input account unchanged.
    ///
    /// # Errors
    /// Currently never; reserved for future validation paths.
    #[instruction]
    #[allow(
        deprecated,
        reason = "SpelOutput::states_only: lez_program macro only rewrites vec![a, b, ...] \
                  literals into execute_with_claims; this handler delegates to a host function \
                  that returns Vec<AccountPostState>, so migration requires restructuring the \
                  handler shape — workspace-wide follow-up across token/amm/ata"
    )]
    pub fn noop(account: AccountWithMetadata) -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(stablecoin_program::noop::noop(
            account,
        ), vec![]))
    }

    /// Open a new collateral-only position for the calling owner.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if any precondition fails (see
    /// [`stablecoin_program::open_position::open_position`] for the full list).
    #[instruction]
    #[allow(
        deprecated,
        reason = "SpelOutput::with_chained_calls: same reason as noop above — migration to \
                  SpelOutput::execute requires the macro's vec![...] literal shape, which \
                  conflicts with delegating to host helpers"
    )]
    pub fn open_position(
        owner: AccountWithMetadata,
        position: AccountWithMetadata,
        vault: AccountWithMetadata,
        user_holding: AccountWithMetadata,
        token_definition: AccountWithMetadata,
        stablecoin_program_id: ProgramId,
        collateral_amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) = stablecoin_program::open_position::open_position(
            owner,
            position,
            vault,
            user_holding,
            token_definition,
            stablecoin_program_id,
            collateral_amount,
        );
        Ok(SpelOutput::with_chained_calls(post_states, chained_calls))
    }
}
