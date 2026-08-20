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

    /// Bootstrap the protocol (see spec §10.1 and host-function
    /// `stablecoin_program::initialize_program::initialize_program`).
    ///
    /// Wall-clock time is read from the system `CLOCK_01` account passed as the
    /// 9th input — the pinned `ProgramContext` exposes no clock.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if any precondition
    /// fails — see the host fn for the full list.
    #[instruction]
    #[allow(
        clippy::too_many_arguments,
        reason = "nine account inputs + the numerical params mirror the host function's ABI"
    )]
    pub fn initialize_program(
        ctx: ProgramContext,
        #[account(signer)]
        admin: AccountWithMetadata,
        #[account(init)]
        protocol_parameters: AccountWithMetadata,
        #[account(init)]
        stability_fee_accumulator: AccountWithMetadata,
        #[account(init)]
        redemption_price_state: AccountWithMetadata,
        #[account(init)]
        stablecoin_definition: AccountWithMetadata,
        #[account(init)]
        stablecoin_master_holding: AccountWithMetadata,
        collateral_definition: AccountWithMetadata,
        market_price_oracle: AccountWithMetadata,
        clock: AccountWithMetadata,
        freeze_authority_account_id: nssa_core::account::AccountId,
        initial_stability_fee_per_millisecond: u128,
        initial_controller_proportional_gain: i128,
        initial_controller_integral_gain: i128,
        initial_minimum_collateralization_ratio: u128,
        minimum_milliseconds_between_rate_updates: u64,
        maximum_oracle_price_age_milliseconds: u64,
        initial_redemption_price: u128,
        stablecoin_name: String,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            stablecoin_program::initialize_program::initialize_program(
                admin,
                protocol_parameters,
                stability_fee_accumulator,
                redemption_price_state,
                stablecoin_definition,
                stablecoin_master_holding,
                collateral_definition,
                market_price_oracle,
                clock,
                ctx.self_program_id,
                stablecoin_program::initialize_program::InitializeProgramParams {
                    freeze_authority_account_id,
                    initial_stability_fee_per_millisecond,
                    initial_controller_proportional_gain,
                    initial_controller_integral_gain,
                    initial_minimum_collateralization_ratio,
                    minimum_milliseconds_between_rate_updates,
                    maximum_oracle_price_age_milliseconds,
                    initial_redemption_price,
                    stablecoin_name: &stablecoin_name,
                },
            );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }

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
