#![cfg_attr(not(test), no_main)]

use nssa_core::account::{AccountId, AccountWithMetadata};
use spel_framework::{context::ProgramContext, prelude::*};

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

    /// Initialize redemption-rate feedback controller state for one stablecoin/feed pair.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if any precondition
    /// fails (see
    /// [`stablecoin_program::redemption_controller::initialize_redemption_controller`]
    /// for the full list).
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface exposes controller configuration explicitly"
    )]
    #[instruction]
    pub fn initialize_redemption_controller(
        ctx: ProgramContext,
        #[account(init)]
        controller: AccountWithMetadata,
        #[account(signer)]
        stablecoin_definition: AccountWithMetadata,
        price_feed: AccountWithMetadata,
        collateral_definition_id: AccountId,
        initial_redemption_price: u128,
        proportional_gain: u128,
        integral_gain: u128,
        max_integral_error: u128,
        max_redemption_rate: u128,
        max_price_feed_age: u64,
        current_timestamp: u64,
    ) -> SpelResult {
        let post_states =
            stablecoin_program::redemption_controller::initialize_redemption_controller(
                controller,
                stablecoin_definition,
                price_feed,
                ctx.self_program_id,
                collateral_definition_id,
                initial_redemption_price,
                proportional_gain,
                integral_gain,
                max_integral_error,
                max_redemption_rate,
                max_price_feed_age,
                current_timestamp,
            );
        let validity_end = current_timestamp
            .checked_add(1)
            .expect("current_timestamp must allow an exact validity window");
        Ok(spel_framework::SpelOutput::execute(post_states, vec![])
            .try_with_timestamp_validity_window(current_timestamp..validity_end)
            .expect("exact timestamp validity window must be non-empty"))
    }

    /// Update redemption price and redemption rate from the configured price feed.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if controller state
    /// validation fails. Stale or unavailable price feeds pause updates by
    /// emitting the controller state unchanged.
    #[instruction]
    pub fn update_redemption_controller(
        ctx: ProgramContext,
        #[account(mut)]
        controller: AccountWithMetadata,
        price_feed: AccountWithMetadata,
        current_timestamp: u64,
    ) -> SpelResult {
        let post_states = stablecoin_program::redemption_controller::update_redemption_controller(
            controller,
            price_feed,
            ctx.self_program_id,
            current_timestamp,
        );
        let validity_end = current_timestamp
            .checked_add(1)
            .expect("current_timestamp must allow an exact validity window");
        Ok(spel_framework::SpelOutput::execute(post_states, vec![])
            .try_with_timestamp_validity_window(current_timestamp..validity_end)
            .expect("exact timestamp validity window must be non-empty"))
    }
}
