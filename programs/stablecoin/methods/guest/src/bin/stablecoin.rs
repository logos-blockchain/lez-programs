#![cfg_attr(not(test), no_main)]

use nssa_core::account::AccountWithMetadata;
use spel_framework::context::ProgramContext;
use spel_framework::prelude::*;

#[cfg(not(test))]
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
    #[allow(
        clippy::too_many_arguments,
        reason = "account inputs + nonce + amount mirror the host function's ABI"
    )]
    pub fn open_position(
        ctx: ProgramContext,
        #[account(signer)]
        owner: AccountWithMetadata,
        #[account(init)]
        position: AccountWithMetadata,
        #[account(init)]
        vault: AccountWithMetadata,
        #[account(mut, signer)]
        user_holding: AccountWithMetadata,
        token_definition: AccountWithMetadata,
        position_nonce: u64,
        collateral_amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) = stablecoin_program::open_position::open_position(
            owner,
            position,
            vault,
            user_holding,
            token_definition,
            ctx.self_program_id,
            position_nonce,
            collateral_amount,
        );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }

    /// Withdraw `amount` collateral tokens from an existing position back to a
    /// user-controlled holding.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if any precondition
    /// fails (see
    /// [`stablecoin_program::withdraw_collateral::withdraw_collateral`] for the
    /// full list).
    #[instruction]
    pub fn withdraw_collateral(
        ctx: ProgramContext,
        #[account(signer)]
        owner: AccountWithMetadata,
        #[account(mut)]
        position: AccountWithMetadata,
        #[account(mut)]
        vault: AccountWithMetadata,
        #[account(mut)]
        destination: AccountWithMetadata,
        amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            stablecoin_program::withdraw_collateral::withdraw_collateral(
                owner,
                position,
                vault,
                destination,
                ctx.self_program_id,
                amount,
            );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }

    /// Repay `amount` of outstanding stablecoin debt against an existing position.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if any precondition
    /// fails (see [`stablecoin_program::repay_debt::repay_debt`] for the
    /// full list).
    #[instruction]
    pub fn repay_debt(
        ctx: ProgramContext,
        #[account(signer)]
        owner: AccountWithMetadata,
        #[account(mut)]
        position: AccountWithMetadata,
        #[account(mut)]
        stablecoin_definition: AccountWithMetadata,
        #[account(mut, signer)]
        user_stablecoin_holding: AccountWithMetadata,
        amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) = stablecoin_program::repay_debt::repay_debt(
            owner,
            position,
            stablecoin_definition,
            user_stablecoin_holding,
            ctx.self_program_id,
            amount,
        );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }
}
