use amm_core::{
    compute_pool_pda_seed, compute_protocol_fee_pda, compute_protocol_fee_pda_seed,
    compute_vault_pda_seed, protocol_fee_amount, spot_price_q64_64, swap_exact_in_amounts,
    swap_exact_out_amounts, ConfigFacts, PoolDefinition, SwapBinding, MINIMUM_LIQUIDITY,
};
use lee_core::{
    account::{AccountId, ProgramShardSelector, ShardData},
    program::{AccountMeta, ChainedCall},
};
use twap_oracle_core::compute_current_tick_account_pda;

use crate::new_definition::transfer_call;

const CONTEXT: &str = "Swap";

/// Plan half shared by both swap instructions: check the addresses and chain the
/// two transfers, the protocol-fee transfer when there is one, and the tick
/// refresh.
///
/// The swap *direction* used to be read off the trader's input holding. A plan
/// reads nothing, so it is declared as `binding.definition_id_in` and resolved
/// here into the vault addresses the transfers need —
/// [`PoolIdentity::sides`](amm_core::PoolIdentity::sides) — while the pool's own
/// apply resolves the same declaration against the stored definition and so
/// decides whether it was true.
///
/// # Panics
/// Panics if:
/// - `clock` is not the canonical 1-block LEZ clock shard.
/// - `pool` is not the pool PDA for `(config, declared pair)`.
/// - either vault slot is not the pool's own.
/// - `binding.definition_id_in` is not one of the declared pair.
/// - either trader holding is one of the pool's vaults, which would turn a self-transfer into free
///   funding.
/// - `protocol_fee_holding` is not the input token's protocol-fee PDA.
/// - `current_tick_account` is not the pool's current-tick PDA.
/// - `user_input_holding` is not authorized, so the input transfer could not be debited.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit pool, vault, user, and oracle accounts"
)]
#[must_use]
pub fn plan_swap(
    config: &AccountMeta,
    pool: &AccountMeta,
    vault_a: &AccountMeta,
    vault_b: &AccountMeta,
    user_input_holding: &AccountMeta,
    user_output_holding: &AccountMeta,
    current_tick_account: &AccountMeta,
    clock: &AccountMeta,
    protocol_fee_holding: &AccountMeta,
    facts: &ConfigFacts,
    binding: &SwapBinding,
    now: u64,
    amm_program_id: AccountId,
) -> Vec<ChainedCall> {
    let identity = &binding.identity;
    crate::shared::assert_canonical_clock(clock, CONTEXT);
    crate::shared::assert_pool_in_namespace(
        pool,
        config.account_id,
        identity,
        amm_program_id,
        CONTEXT,
    );
    crate::shared::assert_vault_slots(vault_a, vault_b, identity, CONTEXT);
    assert!(
        user_input_holding.is_authorized,
        "{CONTEXT}: input holding must authorize the transfer that debits it"
    );

    let ((definition_id_in, vault_in_id), (definition_id_out, vault_out_id)) = identity
        .sides(binding.definition_id_in)
        .expect("AccountId is not a token type for the pool");

    // A trader holding that is also a vault would turn a self-transfer into new
    // funding for the pool, or a withdrawal the reserves never backed.
    for holding in [user_input_holding, user_output_holding] {
        assert!(
            holding.account_id != vault_in_id && holding.account_id != vault_out_id,
            "{CONTEXT}: a trader holding cannot be a pool vault"
        );
    }

    let config_id = config.account_id;
    assert_eq!(
        protocol_fee_holding.account_id,
        compute_protocol_fee_pda(amm_program_id, config_id, definition_id_in),
        "{CONTEXT}: protocol-fee holding does not match the input token's protocol PDA"
    );
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(facts.twap_oracle_program_id, pool.account_id),
        "{CONTEXT}: current tick Account ID does not match PDA"
    );

    let pool_id = pool.account_id;
    let token_program_id = facts.token_program_id;
    let mut calls = Vec::new();

    // 1. trader -> input vault (the full input, protocol fee included).
    calls.push(transfer_call(
        token_program_id,
        user_input_holding.account_id,
        vault_in_id,
        definition_id_in,
        binding.amount_in,
    ));

    // 2. output vault -> trader, under the output vault's seed.
    calls.push(
        transfer_call(
            token_program_id,
            vault_out_id,
            user_output_holding.account_id,
            definition_id_out,
            binding.amount_out,
        )
        .with_pda_seeds(vec![compute_vault_pda_seed(pool_id, definition_id_out)]),
    );

    // 3. the protocol's cut leaves the input vault for the instance-wide protocol-fee holding,
    //    which is created lazily on first use — hence its own seed alongside the vault's. This is
    //    the input vault's *second* touch in one transaction; the runtime resolves call 1's credit
    //    from the transaction's own diff, so there is nothing to hand-build.
    if binding.protocol_fee != 0 {
        calls.push(
            transfer_call(
                token_program_id,
                vault_in_id,
                protocol_fee_holding.account_id,
                definition_id_in,
                binding.protocol_fee,
            )
            .with_pda_seeds(vec![
                compute_vault_pda_seed(pool_id, definition_id_in),
                compute_protocol_fee_pda_seed(config_id, definition_id_in),
            ]),
        );
    }

    calls.push(
        ChainedCall::new(
            facts.twap_oracle_program_id,
            vec![
                ProgramShardSelector::new(
                    current_tick_account.account_id,
                    facts.twap_oracle_program_id,
                ),
                ProgramShardSelector::new(pool_id, amm_program_id),
                ProgramShardSelector::from(clock),
            ],
            &twap_oracle_core::Instruction::UpdateCurrentTick {
                price: binding.new_price,
                now,
            },
        )
        .with_pda_seeds(vec![compute_pool_pda_seed(
            config_id,
            identity.definition_token_a_id,
            identity.definition_token_b_id,
        )]),
    );

    calls
}

/// Which side of the trade the caller fixed, and so which way the amount bound
/// points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exactness {
    /// `amount_in` is the trader's figure; `amount_out` is bounded from above by
    /// what the live curve affords.
    Input,
    /// `amount_out` is the trader's figure; `amount_in` is bounded from below by
    /// what the live curve demands.
    Output,
}

/// Apply half of `SwapExactInput`: price the declared trade against the live
/// reserves and record it.
#[must_use]
pub fn swap_exact_in(pre_data: &ShardData, binding: &SwapBinding) -> ShardData {
    apply_swap(pre_data, binding, Exactness::Input)
}

/// Apply half of `SwapExactOutput`: the mirror of [`swap_exact_in`].
#[must_use]
pub fn swap_exact_out(pre_data: &ShardData, binding: &SwapBinding) -> ShardData {
    apply_swap(pre_data, binding, Exactness::Output)
}

/// The pool's whole say in a swap.
///
/// Three independent things are settled here, and only here:
///
/// * The *direction* — `sides` resolves the declared input token against the pool's stored pair, so
///   a forged direction has nowhere to land.
/// * The *price* — the trade is re-quoted from the reserves this shard actually holds, and the
///   trader's side of it is bounded by that quote. An over-declared output, or an under-declared
///   input, is refused; the opposite slack simply stays in the pool. That bound is what makes a
///   vault drain impossible, and it holds however stale the caller's view of the pool was.
/// * The *fee split* — `protocol_fee` is recomputed from the amounts and the fee tiers, which the
///   config guard pinned, so the protocol's cut cannot be understated to leave more with the
///   providers (or overstated to drain them).
///
/// `new_price` is then pinned exactly, because an oracle tick reaches consumers
/// who act on it. That strictness is what makes a swap retry when the pool moved
/// under it — see [`SwapBinding::new_price`].
///
/// Reserves grow by the input *net* of the protocol fee, which left the vault, so
/// `reserve == vault` keeps holding and the providers' share of the fee accrues
/// as constant-product growth.
///
/// # Panics
/// Panics if:
/// - the shard is not a valid pool, or the declared identity differs.
/// - the pool's LP supply is below [`MINIMUM_LIQUIDITY`] (not yet a live pool).
/// - the declared input token is not one of the pool's two.
/// - either reserve or either amount is zero.
/// - `amount_out` is at or above the output reserve.
/// - the declared amounts fall outside the live quote, in the direction the caller's `Exactness`
///   makes unsafe.
/// - the trade misses the caller's own `limit`.
/// - `protocol_fee` or `new_price` is not the value the pool recomputes.
fn apply_swap(pre_data: &ShardData, binding: &SwapBinding, exactness: Exactness) -> ShardData {
    let pool = crate::shared::pool_of(pre_data, &binding.identity, CONTEXT);
    assert!(
        pool.liquidity_pool_supply >= MINIMUM_LIQUIDITY,
        "Pool liquidity supply is below minimum liquidity"
    );

    let (input, output) = pool
        .sides(binding.definition_id_in)
        .expect("AccountId is not a token type for the pool");
    let (reserve_in, reserve_out) = (input.reserve, output.reserve);
    assert!(
        reserve_in != 0 && reserve_out != 0,
        "Pool reserves must be nonzero"
    );
    assert!(
        binding.amount_in != 0 && binding.amount_out != 0,
        "Swap amounts must be nonzero"
    );
    assert!(
        binding.amount_out < reserve_out,
        "Swap output exhausts the reserve"
    );

    // The fee-adjusted input, from the same shared pricing the off-chain quote
    // uses, so a preview and the executed trade agree exactly.
    let effective_amount_in = match exactness {
        Exactness::Input => {
            let (effective, quote) = swap_exact_in_amounts(
                binding.amount_in,
                reserve_in,
                reserve_out,
                binding.swap_fee_bps,
            );
            assert!(effective != 0, "Effective swap amount should be nonzero");
            assert!(
                binding.amount_out <= quote,
                "The pool cannot afford this offer at its live price"
            );
            assert!(
                binding.amount_out >= binding.limit,
                "Withdraw amount is less than minimal amount out"
            );
            effective
        }
        Exactness::Output => {
            let (effective, required_in) = swap_exact_out_amounts(
                binding.amount_out,
                reserve_in,
                reserve_out,
                binding.swap_fee_bps,
            )
            .expect("swap exact output: reserves and fee must yield a valid input");
            assert!(
                binding.amount_in >= required_in,
                "Declared input is below what the pool's live price demands"
            );
            assert!(
                binding.amount_in <= binding.limit,
                "Required input exceeds maximum amount in"
            );
            effective
        }
    };

    // `swap_fee` is the input the trader forfeits to the pool; `protocol_fee` is
    // the slice of it diverted from the providers to the protocol holding.
    let swap_fee = binding
        .amount_in
        .checked_sub(effective_amount_in)
        .expect("effective_amount_in <= amount_in");
    assert_eq!(
        binding.protocol_fee,
        protocol_fee_amount(swap_fee, binding.protocol_fee_bps),
        "Declared protocol fee does not match the config's fee split"
    );

    let net_amount_in = binding
        .amount_in
        .checked_sub(binding.protocol_fee)
        .expect("protocol_fee <= swap_fee <= amount_in");
    let post_reserve_in = reserve_in
        .checked_add(net_amount_in)
        .expect("reserve_in + net_amount_in overflows u128");
    let post_reserve_out = reserve_out
        .checked_sub(binding.amount_out)
        .expect("amount_out < reserve_out after validation");
    let (reserve_a, reserve_b) =
        pool.reserves_in_ab_order(binding.definition_id_in, post_reserve_in, post_reserve_out);
    assert_eq!(
        binding.new_price,
        spot_price_q64_64(reserve_a, reserve_b),
        "Declared post-swap price does not match the pool's new spot price"
    );

    ShardData::from(&PoolDefinition {
        reserve_a,
        reserve_b,
        ..pool
    })
}
