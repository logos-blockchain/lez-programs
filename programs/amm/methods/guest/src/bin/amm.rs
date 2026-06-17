#![cfg_attr(not(test), no_main)]

use std::num::NonZeroU128;

use spel_framework::prelude::*;
use spel_framework::context::ProgramContext;
use nssa_core::{
    account::{AccountId, AccountWithMetadata},
    program::ProgramId,
};

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "amm_core::Instruction")]
mod amm {
    #[expect(
        unused_imports,
        reason = "SPEL instruction macro requires importing parent-scope handler types"
    )]
    use super::*;

    /// Initializes the AMM Program by creating its singleton config account.
    ///
    /// Expected accounts:
    /// 1. `config` — uninitialized config PDA derived from `compute_config_pda(self_program_id)`.
    #[instruction]
    pub fn initialize(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        token_program_id: ProgramId,
    ) -> SpelResult {
        let post_states =
            amm_program::initialize::initialize(config, token_program_id, ctx.self_program_id);
        Ok(spel_framework::SpelOutput::execute(post_states, vec![]))
    }

    /// Initializes a new Pool (or re-initializes an existing zero-supply Pool).
    /// A fresh user LP holding must be explicitly authorized by the caller.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit config, pool, vault, mint, lock, and user accounts"
    )]
    #[instruction]
    pub fn new_definition(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        pool: AccountWithMetadata,
        vault_a: AccountWithMetadata,
        vault_b: AccountWithMetadata,
        pool_definition_lp: AccountWithMetadata,
        lp_lock_holding: AccountWithMetadata,
        user_holding_a: AccountWithMetadata,
        user_holding_b: AccountWithMetadata,
        user_holding_lp: AccountWithMetadata,
        token_a_amount: u128,
        token_b_amount: u128,
        fees: u128,
        deadline: u64,
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::new_definition::new_definition(
            config,
            pool,
            vault_a,
            vault_b,
            pool_definition_lp,
            lp_lock_holding,
            user_holding_a,
            user_holding_b,
            user_holding_lp,
            NonZeroU128::new(token_a_amount).expect("token_a_amount must be nonzero"),
            NonZeroU128::new(token_b_amount).expect("token_b_amount must be nonzero"),
            fees,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls)
            .with_timestamp_validity_window(..deadline))
    }

    /// Adds liquidity to the Pool.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit pool, vault, and user accounts"
    )]
    #[instruction]
    pub fn add_liquidity(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        pool: AccountWithMetadata,
        vault_a: AccountWithMetadata,
        vault_b: AccountWithMetadata,
        pool_definition_lp: AccountWithMetadata,
        user_holding_a: AccountWithMetadata,
        user_holding_b: AccountWithMetadata,
        user_holding_lp: AccountWithMetadata,
        min_amount_liquidity: u128,
        max_amount_to_add_token_a: u128,
        max_amount_to_add_token_b: u128,
        deadline: u64,
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::add::add_liquidity(
            config,
            pool,
            vault_a,
            vault_b,
            pool_definition_lp,
            user_holding_a,
            user_holding_b,
            user_holding_lp,
            NonZeroU128::new(min_amount_liquidity).expect("min_amount_liquidity must be nonzero"),
            max_amount_to_add_token_a,
            max_amount_to_add_token_b,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls)
            .with_timestamp_validity_window(..deadline))
    }

    /// Removes liquidity from the Pool.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit pool, vault, and user accounts"
    )]
    #[instruction]
    pub fn remove_liquidity(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        pool: AccountWithMetadata,
        vault_a: AccountWithMetadata,
        vault_b: AccountWithMetadata,
        pool_definition_lp: AccountWithMetadata,
        user_holding_a: AccountWithMetadata,
        user_holding_b: AccountWithMetadata,
        user_holding_lp: AccountWithMetadata,
        remove_liquidity_amount: u128,
        min_amount_to_remove_token_a: u128,
        min_amount_to_remove_token_b: u128,
        deadline: u64,
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::remove::remove_liquidity(
            config,
            pool,
            vault_a,
            vault_b,
            pool_definition_lp,
            user_holding_a,
            user_holding_b,
            user_holding_lp,
            NonZeroU128::new(remove_liquidity_amount)
                .expect("remove_liquidity_amount must be nonzero"),
            min_amount_to_remove_token_a,
            min_amount_to_remove_token_b,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls)
            .with_timestamp_validity_window(..deadline))
    }

    /// Swap some quantity of tokens while maintaining the pool constant product.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit pool, vault, user accounts, and bounds"
    )]
    #[instruction]
    pub fn swap_exact_input(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        pool: AccountWithMetadata,
        vault_a: AccountWithMetadata,
        vault_b: AccountWithMetadata,
        user_holding_a: AccountWithMetadata,
        user_holding_b: AccountWithMetadata,
        swap_amount_in: u128,
        min_amount_out: u128,
        token_definition_id_in: AccountId,
        deadline: u64,
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::swap::swap_exact_input(
            config,
            pool,
            vault_a,
            vault_b,
            user_holding_a,
            user_holding_b,
            swap_amount_in,
            min_amount_out,
            token_definition_id_in,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls)
            .with_timestamp_validity_window(..deadline))
    }

    /// Swap tokens specifying the exact desired output amount.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit pool, vault, user accounts, and bounds"
    )]
    #[instruction]
    pub fn swap_exact_output(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        pool: AccountWithMetadata,
        vault_a: AccountWithMetadata,
        vault_b: AccountWithMetadata,
        user_holding_a: AccountWithMetadata,
        user_holding_b: AccountWithMetadata,
        exact_amount_out: u128,
        max_amount_in: u128,
        token_definition_id_in: AccountId,
        deadline: u64,
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::swap::swap_exact_output(
            config,
            pool,
            vault_a,
            vault_b,
            user_holding_a,
            user_holding_b,
            exact_amount_out,
            max_amount_in,
            token_definition_id_in,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls)
            .with_timestamp_validity_window(..deadline))
    }

    /// Sync pool reserves with current vault balances.
    #[instruction]
    pub fn sync_reserves(
        pool: AccountWithMetadata,
        vault_a: AccountWithMetadata,
        vault_b: AccountWithMetadata,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            amm_program::sync::sync_reserves(pool, vault_a, vault_b);
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
    }
}
