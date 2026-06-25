#![cfg_attr(not(test), no_main)]

use nssa_core::account::{AccountId, AccountWithMetadata};
use spel_framework::context::ProgramContext;
use spel_framework::prelude::*;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "stablecoin_core::Instruction")]
mod stablecoin {
    #[allow(unused_imports)]
    use super::*;

    /// Initialize protocol globals and the stablecoin token definition.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface initializes all stablecoin singleton accounts"
    )]
    #[instruction]
    pub fn initialize_program(
        ctx: ProgramContext,
        admin: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        stability_fee_accumulator: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        stablecoin_definition: AccountWithMetadata,
        stablecoin_master_holding: AccountWithMetadata,
        collateral_definition: AccountWithMetadata,
        market_price_oracle: AccountWithMetadata,
        clock: AccountWithMetadata,
        freeze_authority_account_id: AccountId,
        initial_stability_fee_per_millisecond: u128,
        initial_controller_proportional_gain: i128,
        initial_controller_integral_gain: i128,
        initial_minimum_collateralization_ratio: u128,
        minimum_milliseconds_between_rate_updates: u64,
        maximum_oracle_price_age_milliseconds: u64,
        initial_redemption_price: u128,
        stablecoin_name: String,
    ) -> SpelResult {
        let (post_states, chained_calls) = stablecoin_program::initialize_program::initialize_program(
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
            freeze_authority_account_id,
            initial_stability_fee_per_millisecond,
            initial_controller_proportional_gain,
            initial_controller_integral_gain,
            initial_minimum_collateralization_ratio,
            minimum_milliseconds_between_rate_updates,
            maximum_oracle_price_age_milliseconds,
            initial_redemption_price,
            stablecoin_name,
        );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }

    /// Roll the global stability-fee accumulator forward.
    #[instruction]
    pub fn accrue_stability_fee(
        ctx: ProgramContext,
        caller: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        stability_fee_accumulator: AccountWithMetadata,
        clock: AccountWithMetadata,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            stablecoin_program::accrue_stability_fee::accrue_stability_fee(
                caller,
                protocol_parameters,
                stability_fee_accumulator,
                clock,
                ctx.self_program_id,
            );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }

    /// Update stability-fee rate after accruing pending fees at the old rate.
    #[instruction]
    pub fn set_stability_fee_per_millisecond(
        ctx: ProgramContext,
        admin: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        stability_fee_accumulator: AccountWithMetadata,
        clock: AccountWithMetadata,
        new_rate: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            stablecoin_program::set_stability_fee_per_millisecond::set_stability_fee_per_millisecond(
                admin,
                protocol_parameters,
                stability_fee_accumulator,
                clock,
                ctx.self_program_id,
                new_rate,
            );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }

    /// Open a new collateral-only position for the calling owner.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface passes explicit position, vault, token, and protocol accounts"
    )]
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
        collateral_definition: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        clock: AccountWithMetadata,
        position_nonce: u64,
        initial_collateral_amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) = stablecoin_program::open_position::open_position(
            owner,
            position,
            vault,
            user_holding,
            collateral_definition,
            protocol_parameters,
            clock,
            ctx.self_program_id,
            position_nonce,
            initial_collateral_amount,
        );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }

    /// Mint stablecoin debt against an existing position.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface passes explicit position, token, oracle, and protocol accounts"
    )]
    #[instruction]
    pub fn generate_debt(
        ctx: ProgramContext,
        owner: AccountWithMetadata,
        position: AccountWithMetadata,
        stablecoin_definition: AccountWithMetadata,
        user_stablecoin_holding: AccountWithMetadata,
        stability_fee_accumulator: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        market_price_oracle: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        clock: AccountWithMetadata,
        amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) = stablecoin_program::generate_debt::generate_debt(
            owner,
            position,
            stablecoin_definition,
            user_stablecoin_holding,
            stability_fee_accumulator,
            redemption_price_state,
            market_price_oracle,
            protocol_parameters,
            clock,
            ctx.self_program_id,
            amount,
        );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }

    /// Withdraw collateral from an existing position.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface passes explicit position and protocol accounts"
    )]
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
        stability_fee_accumulator: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        clock: AccountWithMetadata,
        amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            stablecoin_program::withdraw_collateral::withdraw_collateral(
                owner,
                position,
                vault,
                destination,
                stability_fee_accumulator,
                redemption_price_state,
                protocol_parameters,
                clock,
                ctx.self_program_id,
                amount,
            );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }

    /// Repay stablecoin debt against an existing position.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface passes explicit position, token, fee, and protocol accounts"
    )]
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
        stability_fee_accumulator: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        clock: AccountWithMetadata,
        amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) = stablecoin_program::repay_debt::repay_debt(
            owner,
            position,
            stablecoin_definition,
            user_stablecoin_holding,
            stability_fee_accumulator,
            protocol_parameters,
            clock,
            ctx.self_program_id,
            amount,
        );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }
}
