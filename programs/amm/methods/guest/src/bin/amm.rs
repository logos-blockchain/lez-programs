#![cfg_attr(not(test), no_main)]

use std::num::NonZeroU128;

use amm_core::{CheckPoolPrice, ConfigFacts, PoolIdentity};
use nssa_core::account::{AccountId, ShardData};
use spel_framework::{context::ProgramContext, prelude::*};
use token_core::TokenDescriptor;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

// Two phases. Each `#[plan]` runs once with no account data in sight: it checks
// ids and authorization, derives addresses, and declares one effect per shard it
// means to touch. Each `#[apply]` then runs once per effect, sees exactly that
// shard's bytes, and returns their replacement — or `None` to leave them alone,
// which is how a guard reads a shard without writing it.
//
// Nine of these eleven instructions read the AMM config and six read the pool.
// None of them can any more, so both became declarations the shard's own effect
// pins: `facts` against the config, `identity` against the pool. Two further
// kinds of read moved the same way — the oracle's current tick, and (for
// `SyncReserves` alone) the vault balances — except that those shards belong to
// other programs, so their effects are `guard`s: read-only by construction,
// since the runtime rejects a write to a shard this program does not own.
//
// The checks that simply disappeared are worth naming. `config.program_owner ==
// self` is structural now: a shard is keyed by the program that wrote it, so
// emitting an effect on `shards[self]` *is* that assertion. The
// `vault_balance >= reserve` asserts are gone because only this program can debit
// a vault. And the fresh-LP-holding signature requirement is gone with address
// claiming itself — writing a shard at an address denies that address to nobody,
// so there is nothing left to squat on.
#[lez_program(instruction = "amm_core::Instruction")]
mod amm {
    #[expect(
        unused_imports,
        reason = "SPEL instruction macro requires importing parent-scope handler types"
    )]
    use super::*;

    // ── Configuration ───────────────────────────────────────────────────────

    /// Initializes a namespaced AMM instance by creating its config account.
    ///
    /// A single deployed AMM Program hosts many independent instances, each keyed
    /// by a namespace `(owner, nonce)`. The owner signs to squat-proof the
    /// namespace; the config PDA's account id is the namespace root every pool
    /// and downstream PDA derives from.
    ///
    /// The one instruction that reads nothing, so the only one with no declared
    /// values: the plan builds the whole record and the apply half only has to
    /// refuse to overwrite.
    ///
    /// Expected accounts:
    /// 1. `owner` — signs this instruction (the namespace owner).
    /// 2. `config` — the config PDA at `compute_config_pda(self_program_id, owner.account_id,
    ///    nonce)`, whose shard must still be empty.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit owner, config, namespace, program ids, and fees"
    )]
    #[plan]
    pub fn initialize(
        ctx: ProgramContext,
        #[account(signer)] owner: AccountMeta,
        #[account(init)] config: AccountMeta,
        nonce: [u8; 32],
        token_program_id: AccountId,
        twap_oracle_program_id: AccountId,
        authority: AccountId,
        swap_fee_bps: u128,
        protocol_fee_bps: u128,
    ) -> SpelResult {
        let data = amm_program::initialize::plan_initialize(
            &owner,
            &config,
            nonce,
            token_program_id,
            twap_oracle_program_id,
            authority,
            swap_fee_bps,
            protocol_fee_bps,
            ctx.self_account_id,
        );
        Ok(SpelOutput::empty().effect(
            &config,
            config.program_account_id,
            &Effect::Create(amm_core::Create { data }),
        ))
    }

    /// Transfers the AMM instance's admin authority.
    ///
    /// Whether the signer is the *right* account is not knowable in the plan — the
    /// stored authority lives in the config shard — so the plan reports which
    /// account claims the role and `rotate_authority` compares it against the
    /// shard it can see.
    ///
    /// Expected accounts:
    /// 1. `config` — the instance's config account.
    /// 2. `authority` — the config's current admin, signed.
    #[plan]
    pub fn update_config(
        #[account(mut)] config: AccountMeta,
        #[account(signer)] authority: AccountMeta,
        new_authority: AccountId,
    ) -> SpelResult {
        amm_program::update_config::plan_update_config(&authority);
        Ok(SpelOutput::empty().effect(
            &config,
            config.program_account_id,
            &Effect::RotateAuthority(amm_core::RotateAuthority {
                expected_authority: authority.account_id,
                new_authority,
            }),
        ))
    }

    // ── Oracle account creation ─────────────────────────────────────────────

    /// Creates a TWAP price-observations account for a pool, via a chained call
    /// to the configured oracle.
    ///
    /// The pool is the price source, authorized by its PDA seed on the call. The
    /// feed's seed tick is still the oracle's own authoritative tick rather than
    /// the caller's — it is declared as `initial_tick` and the guard below holds
    /// that declaration to the oracle's current-tick shard.
    ///
    /// Expected accounts:
    /// 1. `config` — the instance's config account.
    /// 2. `pool` — the pool acting as price source.
    /// 3. `current_tick_account` — the pool's current-tick PDA (the oracle's shard); supplies the
    ///    seed tick.
    /// 4. `price_observations` — the oracle's TWAP PDA for `(pool, window_duration)`, whose shard
    ///    must still be empty.
    /// 5. `clock` — the canonical 1-block LEZ clock shard.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit config, pool, oracle, and clock accounts"
    )]
    #[plan]
    pub fn create_price_observations(
        ctx: ProgramContext,
        config: AccountMeta,
        pool: AccountMeta,
        current_tick_account: AccountMeta,
        #[account(init)] price_observations: AccountMeta,
        clock: AccountMeta,
        window_duration: u64,
        facts: ConfigFacts,
        identity: PoolIdentity,
        initial_tick: i32,
        now: u64,
    ) -> SpelResult {
        let call = amm_program::create_price_observations::plan_create_price_observations(
            &config,
            &pool,
            &current_tick_account,
            &price_observations,
            &clock,
            window_duration,
            &facts,
            &identity,
            initial_tick,
            now,
            ctx.self_account_id,
        );
        Ok(SpelOutput::plan(Vec::new(), vec![call])
            .effect(
                &config,
                config.program_account_id,
                &Effect::CheckConfig(facts),
            )
            .effect(&pool, pool.program_account_id, &Effect::CheckPool(identity))
            .guard(
                &current_tick_account,
                facts.twap_oracle_program_id,
                &Effect::CheckOracleTick(amm_core::CheckOracleTick { tick: initial_tick }),
            ))
    }

    /// Creates a TWAP oracle price account for a pool, via a chained call to the
    /// configured oracle.
    ///
    /// Neither the asset pair nor the initial price is caller-chosen: the pair and
    /// the reserves are declared and pinned exactly by the pool's guard, and the
    /// price is derived from the pinned reserves.
    ///
    /// Expected accounts:
    /// 1. `config` — the instance's config account.
    /// 2. `pool` — the pool acting as price source; supplies the pair and price.
    /// 3. `oracle_price_account` — the oracle's price PDA for `(pool, window_duration)`, whose
    ///    shard must still be empty.
    /// 4. `clock` — the canonical 1-block LEZ clock shard.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit config, pool, oracle, and clock accounts"
    )]
    #[plan]
    pub fn create_oracle_price_account(
        ctx: ProgramContext,
        config: AccountMeta,
        pool: AccountMeta,
        #[account(init)] oracle_price_account: AccountMeta,
        clock: AccountMeta,
        window_duration: u64,
        facts: ConfigFacts,
        identity: PoolIdentity,
        reserve_a: u128,
        reserve_b: u128,
        now: u64,
    ) -> SpelResult {
        let guard = CheckPoolPrice {
            identity,
            reserve_a,
            reserve_b,
        };
        let call = amm_program::create_oracle_price_account::plan_create_oracle_price_account(
            &config,
            &pool,
            &oracle_price_account,
            &clock,
            window_duration,
            &facts,
            &guard,
            now,
            ctx.self_account_id,
        );
        Ok(SpelOutput::plan(Vec::new(), vec![call])
            .effect(
                &config,
                config.program_account_id,
                &Effect::CheckConfig(facts),
            )
            .effect(
                &pool,
                pool.program_account_id,
                &Effect::CheckPoolPrice(guard),
            ))
    }

    // ── Liquidity ───────────────────────────────────────────────────────────

    /// Initializes a new pool, locking `MINIMUM_LIQUIDITY` LP permanently and
    /// minting the rest to the caller.
    ///
    /// The token pair is declared rather than read off the trader's holdings, and
    /// three things pin it: the pool PDA assertion ties it to the pool address,
    /// the chained transfers carry it as their `TokenDescriptor` so the Token
    /// Program refuses them unless the holdings really hold that asset, and the
    /// pool record written here is what every later instruction pins against.
    ///
    /// Expected accounts:
    /// 1. `config` — the instance's config account.
    /// 2. `pool` — the pool PDA, whose shard must still be empty.
    /// 3. `vault_a` / 4. `vault_b` — the pool's vault PDAs (the Token Program's shards), funded by
    ///    the chained transfers.
    /// 5. `pool_definition_lp` — the LP token definition PDA.
    /// 6. `lp_lock_holding` — the PDA holding the permanent liquidity lock.
    /// 7. `user_holding_a` / 8. `user_holding_b` — the caller's funding holdings, signed so the
    ///    chained transfers can debit them.
    /// 9. `user_holding_lp` — receives the caller's LP.
    /// 10. `current_tick_account` — the pool's current-tick PDA, created by a chained call at the
    ///     pool's opening price.
    /// 11. `clock` — the canonical 1-block LEZ clock shard.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit config, pool, vault, mint, lock, and user accounts"
    )]
    #[plan]
    pub fn new_definition(
        ctx: ProgramContext,
        config: AccountMeta,
        #[account(init)] pool: AccountMeta,
        #[account(mut)] vault_a: AccountMeta,
        #[account(mut)] vault_b: AccountMeta,
        #[account(init)] pool_definition_lp: AccountMeta,
        #[account(init)] lp_lock_holding: AccountMeta,
        #[account(mut, signer)] user_holding_a: AccountMeta,
        #[account(mut, signer)] user_holding_b: AccountMeta,
        #[account(mut)] user_holding_lp: AccountMeta,
        #[account(init)] current_tick_account: AccountMeta,
        clock: AccountMeta,
        token_a_amount: u128,
        token_b_amount: u128,
        deadline: u64,
        facts: ConfigFacts,
        definition_token_a_id: AccountId,
        definition_token_b_id: AccountId,
        now: u64,
    ) -> SpelResult {
        let (pool_data, calls) = amm_program::new_definition::plan_new_definition(
            &config,
            &pool,
            &vault_a,
            &vault_b,
            &pool_definition_lp,
            &lp_lock_holding,
            &user_holding_a,
            &user_holding_b,
            &user_holding_lp,
            &current_tick_account,
            &clock,
            NonZeroU128::new(token_a_amount).expect("token_a_amount must be nonzero"),
            NonZeroU128::new(token_b_amount).expect("token_b_amount must be nonzero"),
            &facts,
            definition_token_a_id,
            definition_token_b_id,
            now,
            ctx.self_account_id,
        );
        Ok(SpelOutput::plan(Vec::new(), calls)
            .effect(
                &config,
                config.program_account_id,
                &Effect::CheckConfig(facts),
            )
            .effect(
                &pool,
                pool.program_account_id,
                &Effect::Create(amm_core::Create { data: pool_data }),
            )
            .with_timestamp_validity_window(..deadline))
    }

    /// Adds liquidity to a pool.
    ///
    /// The deposits and the LP to mint ride on the chained calls, so the plan must
    /// name them — but they are functions of the live reserves. They are declared,
    /// and the pool's effect bounds them: the LP minted may not exceed the
    /// pro-rata share of what was actually deposited, on both sides.
    ///
    /// Expected accounts: as `new_definition`, minus the LP lock (the pool already
    /// holds it) and with the current-tick account refreshed rather than created.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit pool, vault, and user accounts"
    )]
    #[plan]
    pub fn add_liquidity(
        ctx: ProgramContext,
        config: AccountMeta,
        #[account(mut)] pool: AccountMeta,
        #[account(mut)] vault_a: AccountMeta,
        #[account(mut)] vault_b: AccountMeta,
        #[account(mut)] pool_definition_lp: AccountMeta,
        #[account(mut, signer)] user_holding_a: AccountMeta,
        #[account(mut, signer)] user_holding_b: AccountMeta,
        #[account(mut)] user_holding_lp: AccountMeta,
        #[account(mut)] current_tick_account: AccountMeta,
        clock: AccountMeta,
        min_amount_liquidity: u128,
        max_amount_to_add_token_a: u128,
        max_amount_to_add_token_b: u128,
        deadline: u64,
        facts: ConfigFacts,
        identity: PoolIdentity,
        amount_a: u128,
        amount_b: u128,
        delta_lp: u128,
        new_price: u128,
        now: u64,
    ) -> SpelResult {
        let effect = amm_core::AddLiquidity {
            identity,
            new_price,
            amount_a,
            amount_b,
            delta_lp,
            min_amount_liquidity: NonZeroU128::new(min_amount_liquidity)
                .expect("min_amount_liquidity must be nonzero")
                .get(),
            max_amount_a: max_amount_to_add_token_a,
            max_amount_b: max_amount_to_add_token_b,
        };
        let calls = amm_program::add::plan_add_liquidity(
            &config,
            &pool,
            &vault_a,
            &vault_b,
            &pool_definition_lp,
            &user_holding_a,
            &user_holding_b,
            &user_holding_lp,
            &current_tick_account,
            &clock,
            &facts,
            &effect,
            now,
            ctx.self_account_id,
        );
        Ok(SpelOutput::plan(Vec::new(), calls)
            .effect(
                &config,
                config.program_account_id,
                &Effect::CheckConfig(facts),
            )
            .effect(
                &pool,
                pool.program_account_id,
                &Effect::PoolAfterAdd(effect),
            )
            .with_timestamp_validity_window(..deadline))
    }

    /// Removes liquidity from a pool.
    ///
    /// The mirror of `add_liquidity`: the withdrawals are declared and bounded
    /// from above by the burner's pro-rata share of the live reserves. The two
    /// reads of the burner's own LP holding are gone — the chained `Burn` carries
    /// the LP definition as its descriptor and debits with a checked subtraction,
    /// so a wrong token or an overdraw fails in the Token Program.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit pool, vault, and user accounts"
    )]
    #[plan]
    pub fn remove_liquidity(
        ctx: ProgramContext,
        config: AccountMeta,
        #[account(mut)] pool: AccountMeta,
        #[account(mut)] vault_a: AccountMeta,
        #[account(mut)] vault_b: AccountMeta,
        #[account(mut)] pool_definition_lp: AccountMeta,
        #[account(mut)] user_holding_a: AccountMeta,
        #[account(mut)] user_holding_b: AccountMeta,
        #[account(mut, signer)] user_holding_lp: AccountMeta,
        #[account(mut)] current_tick_account: AccountMeta,
        clock: AccountMeta,
        remove_liquidity_amount: u128,
        min_amount_to_remove_token_a: u128,
        min_amount_to_remove_token_b: u128,
        deadline: u64,
        facts: ConfigFacts,
        identity: PoolIdentity,
        withdraw_amount_a: u128,
        withdraw_amount_b: u128,
        new_price: u128,
        now: u64,
    ) -> SpelResult {
        let effect = amm_core::RemoveLiquidity {
            identity,
            new_price,
            remove_liquidity_amount: NonZeroU128::new(remove_liquidity_amount)
                .expect("remove_liquidity_amount must be nonzero")
                .get(),
            withdraw_amount_a,
            withdraw_amount_b,
            min_amount_to_remove_token_a,
            min_amount_to_remove_token_b,
        };
        let calls = amm_program::remove::plan_remove_liquidity(
            &config,
            &pool,
            &vault_a,
            &vault_b,
            &pool_definition_lp,
            &user_holding_a,
            &user_holding_b,
            &user_holding_lp,
            &current_tick_account,
            &clock,
            &facts,
            &effect,
            now,
            ctx.self_account_id,
        );
        Ok(SpelOutput::plan(Vec::new(), calls)
            .effect(
                &config,
                config.program_account_id,
                &Effect::CheckConfig(facts),
            )
            .effect(
                &pool,
                pool.program_account_id,
                &Effect::PoolAfterRemove(effect),
            )
            .with_timestamp_validity_window(..deadline))
    }

    // ── Swaps ───────────────────────────────────────────────────────────────

    /// Swaps an exact input amount along the pool's constant product.
    ///
    /// The direction is declared as `definition_id_in` — the plan can no longer
    /// read the trader's holding to work it out — and resolved against the pool's
    /// own stored pair by its effect. `amount_out` is bounded from above by the
    /// live quote, so an over-declaration is refused and an under-declaration
    /// leaves the surplus with the pool.
    ///
    /// Expected accounts:
    /// 1. `config` — the instance's config account.
    /// 2. `pool` — the pool.
    /// 3. `vault_a` / 4. `vault_b` — the pool's vaults, in the pool's own A/B order regardless of
    ///    trade direction.
    /// 5. `user_input_holding` — the token being sold, signed.
    /// 6. `user_output_holding` — receives the token being bought.
    /// 7. `current_tick_account` — the pool's current-tick PDA, refreshed.
    /// 8. `clock` — the canonical 1-block LEZ clock shard.
    /// 9. `protocol_fee_holding` — the input token's protocol-fee PDA, credited when the instance
    ///    takes a protocol fee.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit pool, vault, user accounts, and bounds"
    )]
    #[plan]
    pub fn swap_exact_input(
        ctx: ProgramContext,
        config: AccountMeta,
        #[account(mut)] pool: AccountMeta,
        #[account(mut)] vault_a: AccountMeta,
        #[account(mut)] vault_b: AccountMeta,
        #[account(mut, signer)] user_input_holding: AccountMeta,
        #[account(mut)] user_output_holding: AccountMeta,
        #[account(mut)] current_tick_account: AccountMeta,
        clock: AccountMeta,
        #[account(mut)] protocol_fee_holding: AccountMeta,
        swap_amount_in: u128,
        min_amount_out: u128,
        deadline: u64,
        facts: ConfigFacts,
        identity: PoolIdentity,
        definition_id_in: AccountId,
        protocol_fee: u128,
        new_price: u128,
        amount_out: u128,
        now: u64,
    ) -> SpelResult {
        let binding = swap_binding(
            &facts,
            identity,
            definition_id_in,
            swap_amount_in,
            amount_out,
            protocol_fee,
            new_price,
            min_amount_out,
        );
        let calls = amm_program::swap::plan_swap(
            &config,
            &pool,
            &vault_a,
            &vault_b,
            &user_input_holding,
            &user_output_holding,
            &current_tick_account,
            &clock,
            &protocol_fee_holding,
            &facts,
            &binding,
            now,
            ctx.self_account_id,
        );
        Ok(SpelOutput::plan(Vec::new(), calls)
            .effect(
                &config,
                config.program_account_id,
                &Effect::CheckConfig(facts),
            )
            .effect(
                &pool,
                pool.program_account_id,
                &Effect::PoolAfterSwapIn(binding),
            )
            .with_timestamp_validity_window(..deadline))
    }

    /// Swaps for an exact output amount along the pool's constant product.
    ///
    /// The mirror of `swap_exact_input`: here `amount_in` is bounded from *below*
    /// by what the live curve demands, so paying more only leaves the surplus with
    /// the pool.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit pool, vault, user accounts, and bounds"
    )]
    #[plan]
    pub fn swap_exact_output(
        ctx: ProgramContext,
        config: AccountMeta,
        #[account(mut)] pool: AccountMeta,
        #[account(mut)] vault_a: AccountMeta,
        #[account(mut)] vault_b: AccountMeta,
        #[account(mut, signer)] user_input_holding: AccountMeta,
        #[account(mut)] user_output_holding: AccountMeta,
        #[account(mut)] current_tick_account: AccountMeta,
        clock: AccountMeta,
        #[account(mut)] protocol_fee_holding: AccountMeta,
        exact_amount_out: u128,
        max_amount_in: u128,
        deadline: u64,
        facts: ConfigFacts,
        identity: PoolIdentity,
        definition_id_in: AccountId,
        protocol_fee: u128,
        new_price: u128,
        amount_in: u128,
        now: u64,
    ) -> SpelResult {
        let binding = swap_binding(
            &facts,
            identity,
            definition_id_in,
            amount_in,
            exact_amount_out,
            protocol_fee,
            new_price,
            max_amount_in,
        );
        let calls = amm_program::swap::plan_swap(
            &config,
            &pool,
            &vault_a,
            &vault_b,
            &user_input_holding,
            &user_output_holding,
            &current_tick_account,
            &clock,
            &protocol_fee_holding,
            &facts,
            &binding,
            now,
            ctx.self_account_id,
        );
        Ok(SpelOutput::plan(Vec::new(), calls)
            .effect(
                &config,
                config.program_account_id,
                &Effect::CheckConfig(facts),
            )
            .effect(
                &pool,
                pool.program_account_id,
                &Effect::PoolAfterSwapOut(binding),
            )
            .with_timestamp_validity_window(..deadline))
    }

    /// Assembles the binding both swap modes share.
    ///
    /// The fee tiers are copied out of the `ConfigFacts` the caller declared, not
    /// taken as separate fields, so the config guard that pins those pins these
    /// too — there is no second input to lie about.
    #[expect(
        clippy::too_many_arguments,
        reason = "the binding is the whole declared swap; assembling it names every field"
    )]
    fn swap_binding(
        facts: &ConfigFacts,
        identity: PoolIdentity,
        definition_id_in: AccountId,
        amount_in: u128,
        amount_out: u128,
        protocol_fee: u128,
        new_price: u128,
        limit: u128,
    ) -> amm_core::SwapBinding {
        amm_core::SwapBinding {
            identity,
            definition_id_in,
            amount_in,
            amount_out,
            protocol_fee,
            new_price,
            swap_fee_bps: facts.swap_fee_bps,
            protocol_fee_bps: facts.protocol_fee_bps,
            limit,
        }
    }

    // ── Maintenance ─────────────────────────────────────────────────────────

    /// Syncs the pool's reserves to its vault balances and refreshes the tick.
    ///
    /// The one instruction that genuinely reads the vaults, so the only one that
    /// guards them: both balances are declared and each is pinned to its own Token
    /// Program shard. The synced price then follows from pinned numbers.
    ///
    /// Expected accounts:
    /// 1. `config` — the instance's config account.
    /// 2. `pool` — the pool.
    /// 3. `vault_a` / 4. `vault_b` — the pool's vaults, read-only.
    /// 5. `current_tick_account` — the pool's current-tick PDA, refreshed.
    /// 6. `clock` — the canonical 1-block LEZ clock shard.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit config, pool, vault, oracle, and clock accounts"
    )]
    #[plan]
    pub fn sync_reserves(
        ctx: ProgramContext,
        config: AccountMeta,
        #[account(mut)] pool: AccountMeta,
        vault_a: AccountMeta,
        vault_b: AccountMeta,
        #[account(mut)] current_tick_account: AccountMeta,
        clock: AccountMeta,
        facts: ConfigFacts,
        identity: PoolIdentity,
        vault_a_balance: u128,
        vault_b_balance: u128,
        now: u64,
    ) -> SpelResult {
        let effect = amm_core::SyncReserves {
            identity,
            vault_a_balance,
            vault_b_balance,
        };
        let (descriptor_a, descriptor_b, calls) = amm_program::sync::plan_sync_reserves(
            &config,
            &pool,
            &vault_a,
            &vault_b,
            &current_tick_account,
            &clock,
            &facts,
            &effect,
            now,
            ctx.self_account_id,
        );
        Ok(SpelOutput::plan(Vec::new(), calls)
            .effect(
                &config,
                config.program_account_id,
                &Effect::CheckConfig(facts),
            )
            .guard(
                &vault_a,
                facts.token_program_id,
                &Effect::CheckVaultBalance(amm_core::CheckVaultBalance {
                    descriptor: descriptor_a,
                    balance: vault_a_balance,
                }),
            )
            .guard(
                &vault_b,
                facts.token_program_id,
                &Effect::CheckVaultBalance(amm_core::CheckVaultBalance {
                    descriptor: descriptor_b,
                    balance: vault_b_balance,
                }),
            )
            .effect(
                &pool,
                pool.program_account_id,
                &Effect::PoolAfterSync(effect),
            ))
    }

    /// Withdraws accrued protocol fees for one token to a destination holding.
    ///
    /// Emits no guard beyond the config's: `descriptor` rides on the chained
    /// transfer, and the Token Program asserts it against both the protocol-fee
    /// holding and the destination, which is the whole of what the two holding
    /// reads used to establish.
    ///
    /// Expected accounts:
    /// 1. `config` — its stored `authority` gates the call.
    /// 2. `protocol_fee_holding` — the protocol-fee PDA for the declared token.
    /// 3. `destination` — an initialized holding of that token.
    /// 4. `authority` — must be `config.authority`, signed.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit config, holding, destination, and authority accounts"
    )]
    #[plan]
    pub fn withdraw_protocol_fees(
        ctx: ProgramContext,
        config: AccountMeta,
        #[account(mut)] protocol_fee_holding: AccountMeta,
        #[account(mut)] destination: AccountMeta,
        #[account(signer)] authority: AccountMeta,
        amount: u128,
        facts: ConfigFacts,
        descriptor: TokenDescriptor,
    ) -> SpelResult {
        let call = amm_program::withdraw_protocol_fees::plan_withdraw_protocol_fees(
            &config,
            &protocol_fee_holding,
            &destination,
            &authority,
            amount,
            &facts,
            &descriptor,
            ctx.self_account_id,
        );
        Ok(SpelOutput::plan(Vec::new(), vec![call]).effect(
            &config,
            config.program_account_id,
            &Effect::CheckConfig(facts),
        ))
    }

    // ── Apply halves ────────────────────────────────────────────────────────

    /// Write a whole record into a shard that must be empty: the config at
    /// `Initialize`, the pool at `NewDefinition`.
    #[apply]
    pub fn create(effect: amm_core::Create, pre_data: &ShardData) -> Option<ShardData> {
        Some(amm_program::shared::create(pre_data, effect.data))
    }

    #[apply]
    pub fn rotate_authority(
        effect: amm_core::RotateAuthority,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(amm_program::update_config::rotate_authority(
            pre_data, &effect,
        ))
    }

    #[apply]
    pub fn pool_after_add(
        effect: amm_core::AddLiquidity,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(amm_program::add::add_liquidity(pre_data, &effect))
    }

    #[apply]
    pub fn pool_after_remove(
        effect: amm_core::RemoveLiquidity,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(amm_program::remove::remove_liquidity(pre_data, &effect))
    }

    #[apply]
    pub fn pool_after_swap_in(
        effect: amm_core::SwapBinding,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(amm_program::swap::swap_exact_in(pre_data, &effect))
    }

    #[apply]
    pub fn pool_after_swap_out(
        effect: amm_core::SwapBinding,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(amm_program::swap::swap_exact_out(pre_data, &effect))
    }

    #[apply]
    pub fn pool_after_sync(
        effect: amm_core::SyncReserves,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(amm_program::sync::sync_reserves(pre_data, &effect))
    }

    // ── Guards ──────────────────────────────────────────────────────────────
    //
    // Each returns `None`. For the two that read another program's shard that is
    // enforced — the runtime rejects a foreign write — but for the two on this
    // program's own shards it is not, so a guard that accidentally wrote would be
    // a silent hole.

    #[apply]
    pub fn check_config(effect: ConfigFacts, pre_data: &ShardData) -> Option<ShardData> {
        amm_core::check_config(pre_data, &effect);
        None
    }

    #[apply]
    pub fn check_pool(effect: PoolIdentity, pre_data: &ShardData) -> Option<ShardData> {
        amm_program::shared::pool_of(pre_data, &effect, "Check pool");
        None
    }

    #[apply]
    pub fn check_pool_price(effect: CheckPoolPrice, pre_data: &ShardData) -> Option<ShardData> {
        amm_program::create_oracle_price_account::check_pool_price(pre_data, &effect);
        None
    }

    #[apply]
    pub fn check_oracle_tick(
        effect: amm_core::CheckOracleTick,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        amm_program::create_price_observations::check_oracle_tick(pre_data, effect.tick);
        None
    }

    #[apply]
    pub fn check_vault_balance(
        effect: amm_core::CheckVaultBalance,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        amm_core::check_vault_balance(pre_data, &effect);
        None
    }
}
