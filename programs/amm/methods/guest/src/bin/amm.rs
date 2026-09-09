#![cfg_attr(not(test), no_main)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    reason = "SPEL macro emits cloned validation slices for one-account instructions"
)]

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

    /// Initializes a namespaced AMM instance by creating its config account.
    ///
    /// A single deployed AMM Program hosts many independent instances, each keyed by a
    /// namespace `(owner, nonce)`. The owner signs to squat-proof the namespace; the config
    /// PDA's account id is the namespace root every pool and downstream PDA derives from.
    /// (See `amm_program::initialize::initialize` for the full semantics.)
    ///
    /// Expected accounts:
    /// 1. `owner` — signs this instruction (the namespace owner). A fresh owner is claimed by
    ///    the AMM on first use (hence writable); on later instances under the same owner it is
    ///    already AMM-owned and echoed unchanged.
    /// 2. `config` — uninitialized config PDA at
    ///    `compute_config_pda(self_program_id, owner.account_id, nonce)`.
    ///
    /// `swap_fee_bps` is the instance-wide swap fee (basis points) stored in the config; every
    /// swap in this namespace reads it. Fees are no longer configured per pool.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit owner, config, namespace, program ids, and fee"
    )]
    #[instruction]
    pub fn initialize(
        ctx: ProgramContext,
        #[account(mut, signer)]
        owner: AccountWithMetadata,
        #[account(init)]
        config: AccountWithMetadata,
        nonce: [u8; 32],
        token_program_id: ProgramId,
        twap_oracle_program_id: ProgramId,
        authority: AccountId,
        swap_fee_bps: u128,
        protocol_fee_bps: u128,
    ) -> SpelResult {
        let post_states = amm_program::initialize::initialize(
            owner,
            config,
            nonce,
            token_program_id,
            twap_oracle_program_id,
            authority,
            swap_fee_bps,
            protocol_fee_bps,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, vec![]))
    }

    /// Transfers the AMM Program's admin authority. Only the configured admin authority may call
    /// this. The Token Program and TWAP oracle program IDs are immutable deployment parameters and
    /// cannot be changed here.
    ///
    /// Expected accounts:
    /// 1. `config` — initialized AMM config account.
    /// 2. `authority` — the config's current admin, passed authorized (signed).
    #[instruction]
    pub fn update_config(
        ctx: ProgramContext,
        #[account(mut)]
        config: AccountWithMetadata,
        #[account(signer)]
        authority: AccountWithMetadata,
        new_authority: AccountId,
    ) -> SpelResult {
        let post_states = amm_program::update_config::update_config(
            config,
            authority,
            new_authority,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, vec![]))
    }

    /// Creates a TWAP price-observations account for a pool over a time window, on behalf of the
    /// AMM, via a chained call to the configured TWAP oracle program.
    ///
    /// Expected accounts:
    /// 1. `config` — initialized AMM config account.
    /// 2. `pool` — initialized AMM pool; acts as the (authorized) price source.
    /// 3. `current_tick_account` — the pool's initialized TWAP current-tick PDA; supplies the
    ///    initial tick.
    /// 4. `price_observations` — uninitialized TWAP PDA for `(pool, window_duration)`.
    /// 5. `clock` — the canonical 1-block LEZ clock account.
    #[instruction]
    pub fn create_price_observations(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        pool: AccountWithMetadata,
        current_tick_account: AccountWithMetadata,
        #[account(init)]
        price_observations: AccountWithMetadata,
        clock: AccountWithMetadata,
        window_duration: u64,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            amm_program::create_price_observations::create_price_observations(
                config,
                pool,
                current_tick_account,
                price_observations,
                clock,
                window_duration,
                ctx.self_program_id,
            );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
    }

    /// Creates a TWAP oracle price account for a pool over a time window, on behalf of the AMM, via
    /// a chained call to the configured TWAP oracle program.
    ///
    /// Expected accounts:
    /// 1. `config` — initialized AMM config account.
    /// 2. `pool` — initialized AMM pool; acts as the (authorized) price source and supplies the
    ///    asset pair and the initial (spot) price.
    /// 3. `oracle_price_account` — uninitialized TWAP price-account PDA for `(pool, window_duration)`.
    /// 4. `clock` — the canonical 1-block LEZ clock account.
    #[instruction]
    pub fn create_oracle_price_account(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        pool: AccountWithMetadata,
        #[account(init)]
        oracle_price_account: AccountWithMetadata,
        clock: AccountWithMetadata,
        window_duration: u64,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            amm_program::create_oracle_price_account::create_oracle_price_account(
                config,
                pool,
                oracle_price_account,
                clock,
                window_duration,
                ctx.self_program_id,
            );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
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
        #[account(init)]
        pool: AccountWithMetadata,
        #[account(mut)]
        vault_a: AccountWithMetadata,
        #[account(mut)]
        vault_b: AccountWithMetadata,
        #[account(init)]
        pool_definition_lp: AccountWithMetadata,
        #[account(init)]
        lp_lock_holding: AccountWithMetadata,
        #[account(mut, signer)]
        user_holding_a: AccountWithMetadata,
        #[account(mut, signer)]
        user_holding_b: AccountWithMetadata,
        #[account(mut, signer)]
        user_holding_lp: AccountWithMetadata,
        #[account(init)]
        current_tick_account: AccountWithMetadata,
        clock: AccountWithMetadata,
        token_a_amount: u128,
        token_b_amount: u128,
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
            current_tick_account,
            clock,
            NonZeroU128::new(token_a_amount).expect("token_a_amount must be nonzero"),
            NonZeroU128::new(token_b_amount).expect("token_b_amount must be nonzero"),
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
        #[account(mut)]
        pool: AccountWithMetadata,
        #[account(mut)]
        vault_a: AccountWithMetadata,
        #[account(mut)]
        vault_b: AccountWithMetadata,
        #[account(mut)]
        pool_definition_lp: AccountWithMetadata,
        #[account(mut, signer)]
        user_holding_a: AccountWithMetadata,
        #[account(mut, signer)]
        user_holding_b: AccountWithMetadata,
        #[account(mut)]
        user_holding_lp: AccountWithMetadata,
        #[account(mut)]
        current_tick_account: AccountWithMetadata,
        clock: AccountWithMetadata,
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
            current_tick_account,
            clock,
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
        #[account(mut)]
        pool: AccountWithMetadata,
        #[account(mut)]
        vault_a: AccountWithMetadata,
        #[account(mut)]
        vault_b: AccountWithMetadata,
        #[account(mut)]
        pool_definition_lp: AccountWithMetadata,
        #[account(mut)]
        user_holding_a: AccountWithMetadata,
        #[account(mut)]
        user_holding_b: AccountWithMetadata,
        #[account(mut, signer)]
        user_holding_lp: AccountWithMetadata,
        #[account(mut)]
        current_tick_account: AccountWithMetadata,
        clock: AccountWithMetadata,
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
            current_tick_account,
            clock,
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
    ///
    /// The swap direction is the input holding's own token; `user_input_holding` must be signed so
    /// the downstream token transfer can debit it. `user_output_holding` only receives.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit pool, vault, user accounts, and bounds"
    )]
    #[instruction]
    pub fn swap_exact_input(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        #[account(mut)]
        pool: AccountWithMetadata,
        #[account(mut)]
        vault_a: AccountWithMetadata,
        #[account(mut)]
        vault_b: AccountWithMetadata,
        #[account(mut, signer)]
        user_input_holding: AccountWithMetadata,
        #[account(mut)]
        user_output_holding: AccountWithMetadata,
        #[account(mut)]
        current_tick_account: AccountWithMetadata,
        clock: AccountWithMetadata,
        // The input-token protocol-fee holding (created lazily on first use); mutated when a
        // protocol fee is taken.
        #[account(mut)]
        protocol_fee_holding: AccountWithMetadata,
        swap_amount_in: u128,
        min_amount_out: u128,
        deadline: u64,
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::swap::swap_exact_input(
            config,
            pool,
            vault_a,
            vault_b,
            user_input_holding,
            user_output_holding,
            current_tick_account,
            clock,
            protocol_fee_holding,
            swap_amount_in,
            min_amount_out,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls)
            .with_timestamp_validity_window(..deadline))
    }

    /// Swap tokens specifying the exact desired output amount.
    ///
    /// The swap direction is the input holding's own token; `user_input_holding` must be signed so
    /// the downstream token transfer can debit it. `user_output_holding` only receives.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit pool, vault, user accounts, and bounds"
    )]
    #[instruction]
    pub fn swap_exact_output(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        #[account(mut)]
        pool: AccountWithMetadata,
        #[account(mut)]
        vault_a: AccountWithMetadata,
        #[account(mut)]
        vault_b: AccountWithMetadata,
        #[account(mut, signer)]
        user_input_holding: AccountWithMetadata,
        #[account(mut)]
        user_output_holding: AccountWithMetadata,
        #[account(mut)]
        current_tick_account: AccountWithMetadata,
        clock: AccountWithMetadata,
        // The input-token protocol-fee holding (created lazily on first use); mutated when a
        // protocol fee is taken.
        #[account(mut)]
        protocol_fee_holding: AccountWithMetadata,
        exact_amount_out: u128,
        max_amount_in: u128,
        deadline: u64,
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::swap::swap_exact_output(
            config,
            pool,
            vault_a,
            vault_b,
            user_input_holding,
            user_output_holding,
            current_tick_account,
            clock,
            protocol_fee_holding,
            exact_amount_out,
            max_amount_in,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls)
            .with_timestamp_validity_window(..deadline))
    }

    /// Sync pool reserves with current vault balances, refreshing the pool's TWAP current tick.
    #[instruction]
    pub fn sync_reserves(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        #[account(mut)]
        pool: AccountWithMetadata,
        // vault_a / vault_b are only read to compute balances in
        // amm_program::sync::sync_reserves (their post-states are unchanged
        // clones), so they are not writable — no `mut` metadata.
        vault_a: AccountWithMetadata,
        vault_b: AccountWithMetadata,
        #[account(mut)]
        current_tick_account: AccountWithMetadata,
        clock: AccountWithMetadata,
    ) -> SpelResult {
        let (post_states, chained_calls) = amm_program::sync::sync_reserves(
            config,
            pool,
            vault_a,
            vault_b,
            current_tick_account,
            clock,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
    }

    /// Withdraw accrued protocol fees for one token to a destination holding. Only the config's
    /// admin `authority` may call this (it signs). The protocol-fee holding's own token definition
    /// selects the token withdrawn; `destination` must be an initialized holding of that token.
    ///
    /// Expected accounts:
    /// 1. `config` — the AMM instance's config; its stored `authority` gates the call.
    /// 2. `protocol_fee_holding` — the source PDA at
    ///    `compute_protocol_fee_pda(self_program_id, config.account_id, token_definition_id)`.
    /// 3. `destination` — an initialized Token Holding of the same token that receives the fees.
    /// 4. `authority` — must equal `config.authority`, signed.
    #[instruction]
    pub fn withdraw_protocol_fees(
        ctx: ProgramContext,
        config: AccountWithMetadata,
        #[account(mut)]
        protocol_fee_holding: AccountWithMetadata,
        #[account(mut)]
        destination: AccountWithMetadata,
        #[account(signer)]
        authority: AccountWithMetadata,
        amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            amm_program::withdraw_protocol_fees::withdraw_protocol_fees(
                config,
                protocol_fee_holding,
                destination,
                authority,
                amount,
                ctx.self_program_id,
            );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
    }
}
