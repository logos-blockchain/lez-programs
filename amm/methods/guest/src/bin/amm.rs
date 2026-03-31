#![no_main]

use std::num::NonZeroU128;

use spel_framework::prelude::*;
use nssa_core::{
    account::{AccountId, AccountWithMetadata},
    program::ProgramId,
};

risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "amm_core::Instruction")]
mod amm {
    #[allow(unused_imports)]
    use super::*;

    /// Initializes a new Pool (or re-initializes an inactive Pool).
    #[instruction]
    pub fn new_definition(
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
        amm_program_id: ProgramId,
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::new_definition::new_definition(
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
            amm_program_id,
        );
        Ok(SpelOutput::with_chained_calls(post_states, chained_calls))
    }

    /// Adds liquidity to the Pool.
    #[instruction]
    pub fn add_liquidity(
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
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::add::add_liquidity(
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
        );
        Ok(SpelOutput::with_chained_calls(post_states, chained_calls))
    }

    /// Removes liquidity from the Pool.
    #[instruction]
    pub fn remove_liquidity(
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
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::remove::remove_liquidity(
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
        );
        Ok(SpelOutput::with_chained_calls(post_states, chained_calls))
    }

    /// Swap some quantity of tokens while maintaining the pool constant product.
    #[instruction]
    pub fn swap_exact_input(
        pool: AccountWithMetadata,
        vault_a: AccountWithMetadata,
        vault_b: AccountWithMetadata,
        user_holding_a: AccountWithMetadata,
        user_holding_b: AccountWithMetadata,
        swap_amount_in: u128,
        min_amount_out: u128,
        token_definition_id_in: AccountId,
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::swap::swap_exact_input(
            pool,
            vault_a,
            vault_b,
            user_holding_a,
            user_holding_b,
            swap_amount_in,
            min_amount_out,
            token_definition_id_in,
        );
        Ok(SpelOutput::with_chained_calls(post_states, chained_calls))
    }

    /// Swap tokens specifying the exact desired output amount.
    #[instruction]
    pub fn swap_exact_output(
        pool: AccountWithMetadata,
        vault_a: AccountWithMetadata,
        vault_b: AccountWithMetadata,
        user_holding_a: AccountWithMetadata,
        user_holding_b: AccountWithMetadata,
        exact_amount_out: u128,
        max_amount_in: u128,
        token_definition_id_in: AccountId,
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::swap::swap_exact_output(
            pool,
            vault_a,
            vault_b,
            user_holding_a,
            user_holding_b,
            exact_amount_out,
            max_amount_in,
            token_definition_id_in,
        );
        Ok(SpelOutput::with_chained_calls(post_states, chained_calls))
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
        Ok(SpelOutput::with_chained_calls(post_states, chained_calls))
    }
}
