use alloy_primitives::U256;
use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_vault_pda, isqrt_product, AmmConfig, Instruction, PoolDefinition, MINIMUM_LIQUIDITY,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use pretty_assertions::assert_eq;
use serde_json::json;
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::compute_current_tick_account_pda;

use super::{
    admin::transfer_ownership_plan,
    config::config_account as decode_config_account,
    context::resolve_tokens,
    holding::{select_holding, SelectedHolding},
    pair::{is_canonical_pair, pair_ids, PairIds},
    quote::{div_ceil_u256, minimum_opening_pair, Q64},
    swap::{swap_exact_in_plan, swap_exact_out_plan},
    ConfigAccountRequest, PairIdsRequest, ResolveTokensRequest, SwapExactInPlanRequest,
    SwapExactOutPlanRequest, TransferOwnershipPlanRequest,
};
use crate::{
    account::{account_id_hex, account_read, decode_account, program_id_base58, program_id_bytes},
    AccountRead,
};

const AMM_PROGRAM: ProgramId = [11; 8];
const TOKEN_PROGRAM: ProgramId = [22; 8];
const TWAP_PROGRAM: ProgramId = [33; 8];

fn account(owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner: owner,
        balance: 0,
        data,
        nonce: Nonce(0),
    }
}

fn default_read(id: AccountId) -> AccountRead {
    account_read(id, &Account::default())
}

fn config_account() -> Account {
    account(
        AMM_PROGRAM,
        Data::from(&AmmConfig {
            token_program_id: TOKEN_PROGRAM,
            twap_oracle_program_id: TWAP_PROGRAM,
            authority: AccountId::new([7; 32]),
        }),
    )
}

fn token_definition(name: &str, supply: u128) -> Account {
    account(
        TOKEN_PROGRAM,
        Data::from(&TokenDefinition::Fungible {
            name: String::from(name),
            total_supply: supply,
            metadata_id: None,
            authority: None,
        }),
    )
}

fn token_holding(definition_id: AccountId, balance: u128) -> Account {
    account(
        TOKEN_PROGRAM,
        Data::from(&TokenHolding::Fungible {
            definition_id,
            balance,
        }),
    )
}

fn ids() -> PairIds {
    let token_a = AccountId::new([2; 32]);
    let token_b = AccountId::new([1; 32]);
    let config = compute_config_pda(AMM_PROGRAM);
    let pool = compute_pool_pda(AMM_PROGRAM, token_a, token_b);
    PairIds {
        token_a,
        token_b,
        config,
        pool,
        vault_a: compute_vault_pda(AMM_PROGRAM, pool, token_a),
        vault_b: compute_vault_pda(AMM_PROGRAM, pool, token_b),
        lp_definition: compute_liquidity_token_pda(AMM_PROGRAM, pool),
        lp_lock_holding: compute_lp_lock_holding_pda(AMM_PROGRAM, pool),
        current_tick: compute_current_tick_account_pda(TWAP_PROGRAM, pool),
        clock: CLOCK_01_PROGRAM_ACCOUNT_ID,
    }
}

fn amm_program_id() -> String {
    hex::encode(program_id_bytes(AMM_PROGRAM))
}

#[test]
fn minimum_pair_exceeds_protocol_lock() {
    for price in [1, Q64 / 2_500, Q64 / 10, Q64, Q64 * 2, u128::MAX] {
        let (amount_a, amount_b) = minimum_opening_pair(price).unwrap();
        assert!(amount_a > 0);
        assert!(amount_b > 0);
        assert!(isqrt_product(amount_a, amount_b) > MINIMUM_LIQUIDITY);
    }
}

#[test]
fn minimum_pair_is_minimal_on_price_base_side() {
    let (amount_a, amount_b) = minimum_opening_pair(Q64 * 2).unwrap();
    assert!(isqrt_product(amount_a, amount_b) > MINIMUM_LIQUIDITY);
    let previous_b = div_ceil_u256(
        U256::from(amount_a - 1) * U256::from(Q64 * 2),
        U256::from(Q64),
    );
    assert!(
        U256::from(amount_a - 1) * previous_b <= U256::from(MINIMUM_LIQUIDITY * MINIMUM_LIQUIDITY)
    );
}

#[test]
fn highest_balance_holding_wins_then_lowest_id() {
    let definition = AccountId::new([9; 32]);
    let holding = |id: u8, balance| SelectedHolding {
        id: AccountId::new([id; 32]),
        definition_id: definition,
        balance,
    };
    let selected = select_holding(
        &[holding(4, 10), holding(2, 20), holding(1, 20)],
        definition,
    )
    .unwrap();
    assert_eq!(selected.id, AccountId::new([1; 32]));
}

#[test]
fn pair_manifest_uses_canonical_ids_and_current_program_types() {
    let token_a = AccountId::new([2; 32]);
    let token_b = AccountId::new([1; 32]);
    let config_id = compute_config_pda(AMM_PROGRAM);
    let result = pair_ids(PairIdsRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, &config_account()),
        token_a_id: token_a.to_string(),
        token_b_id: token_b.to_string(),
    })
    .unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["tokenAId"], account_id_hex(token_a));
    assert_eq!(result["tokenBId"], account_id_hex(token_b));
    assert_eq!(
        result["poolId"],
        account_id_hex(compute_pool_pda(AMM_PROGRAM, token_a, token_b))
    );
}

#[test]
fn pair_manifest_reports_invalid_token_as_domain_error() {
    let pair = ids();
    let result = pair_ids(PairIdsRequest {
        amm_program_id: amm_program_id(),
        config: account_read(pair.config, &config_account()),
        token_a_id: String::from("not-a-token-id"),
        token_b_id: pair.token_b.to_string(),
    })
    .expect("invalid user input is a domain result");

    assert_eq!(result["status"], "error");
    assert_eq!(result["code"], "invalid_token_id");
}

#[test]
fn pair_manifest_reports_unavailable_config_as_domain_error() {
    let pair = ids();
    let result = pair_ids(PairIdsRequest {
        amm_program_id: amm_program_id(),
        config: default_read(pair.config),
        token_a_id: pair.token_a.to_string(),
        token_b_id: pair.token_b.to_string(),
    })
    .expect("unavailable chain state is a domain result");

    assert_eq!(result["status"], "error");
    assert_eq!(result["code"], "config_unavailable");
}

#[test]
fn resolve_tokens_returns_lean_rows_held_first_and_omits_unresolvable() {
    let held = AccountId::new([2; 32]);
    let listed = AccountId::new([5; 32]);
    let missing = AccountId::new([9; 32]); // requested but no definition read supplied
    let config_id = compute_config_pda(AMM_PROGRAM);

    let value = resolve_tokens(ResolveTokensRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, &config_account()),
        token_ids: vec![
            account_id_hex(held),
            account_id_hex(listed),
            account_id_hex(missing),
        ],
        wallet_accounts: vec![account_read(
            AccountId::new([6; 32]),
            &token_holding(held, 42),
        )],
        token_definitions: vec![
            account_read(held, &token_definition("Held", 1_000)),
            account_read(listed, &token_definition("Listed", 2_000)),
        ],
    })
    .unwrap();

    // Held token sorts first; the requested id with no readable definition is omitted. Every row
    // carries the same fields — the non-held token gets an empty holdingId and "0" balance.
    assert_eq!(
        value["tokens"],
        json!([
            {
                "definitionId": held.to_string(),
                "name": "Held",
                "totalSupply": "1000",
                "holdingId": AccountId::new([6; 32]).to_string(),
                "balance": "42",
            },
            {
                "definitionId": listed.to_string(),
                "name": "Listed",
                "totalSupply": "2000",
                "holdingId": "",
                "balance": "0",
            },
        ])
    );
}

#[test]
fn transfer_ownership_plan_targets_config_and_current_admin() {
    let config_id = compute_config_pda(AMM_PROGRAM);
    let new_authority = AccountId::new([5; 32]);
    let plan = transfer_ownership_plan(TransferOwnershipPlanRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, &config_account()),
        new_authority_id: account_id_hex(new_authority),
    })
    .unwrap();

    // Accounts: [config (not signer), current admin (signs)]. The current admin is [7; 32] (the
    // config_account fixture's authority); new_authority is instruction data, not an account.
    assert_eq!(
        plan["accountIds"],
        json!([
            account_id_hex(config_id),
            account_id_hex(AccountId::new([7; 32]))
        ])
    );
    assert_eq!(plan["signingRequirements"], json!([false, true]));

    // The instruction decodes back to UpdateConfig { new_authority }.
    let words: Vec<u32> = plan["instruction"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_u64().unwrap() as u32)
        .collect();
    let Instruction::UpdateConfig {
        new_authority: decoded,
    } = risc0_zkvm::serde::from_slice(&words).unwrap()
    else {
        panic!("expected UpdateConfig");
    };
    assert_eq!(decoded, new_authority);
}

#[test]
fn config_account_decodes_authority_and_program_ids() {
    let config_id = compute_config_pda(AMM_PROGRAM);
    let value = decode_config_account(ConfigAccountRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, &config_account()),
    })
    .unwrap();

    assert_eq!(value["status"], "ok");
    assert_eq!(value["configId"], config_id.to_string());
    assert_eq!(value["ammProgramId"], program_id_base58(AMM_PROGRAM));
    assert_eq!(value["authority"], AccountId::new([7; 32]).to_string());
    assert_eq!(value["tokenProgramId"], program_id_base58(TOKEN_PROGRAM));
    assert_eq!(
        value["twapOracleProgramId"],
        program_id_base58(TWAP_PROGRAM)
    );
}

#[test]
fn config_account_is_unavailable_when_not_on_chain() {
    let config_id = compute_config_pda(AMM_PROGRAM);
    let value = decode_config_account(ConfigAccountRequest {
        amm_program_id: amm_program_id(),
        config: default_read(config_id),
    })
    .unwrap();

    assert_eq!(value["status"], "error");
    assert_eq!(value["error"], "config_unavailable");
}

#[test]
fn missing_pool_snapshot_defaults_remain_real_accounts() {
    let id = AccountId::new([5; 32]);
    let read = default_read(id);
    let (decoded_id, decoded) = decode_account(&read).unwrap();
    assert_eq!(decoded_id, id);
    assert_eq!(decoded, Account::default());
}

#[test]
fn swap_plan_uses_the_pool_stored_vaults_not_canonical_order() {
    // A pool created NON-canonically: its stored def_a is the smaller-valued
    // token, so pool.vault_a_id is the vault for the smaller token — the opposite
    // of what canonical_pair (larger first) would derive. The plan must emit the
    // pool's own stored vaults, which is what the guest asserts against.
    let token_small = AccountId::new([1; 32]);
    let token_large = AccountId::new([2; 32]);
    assert!(is_canonical_pair(token_large, token_small)); // large is canonical token_a

    let pool_id = compute_pool_pda(AMM_PROGRAM, token_small, token_large);
    let pool = PoolDefinition {
        definition_token_a_id: token_small, // stored non-canonically (small first)
        definition_token_b_id: token_large,
        vault_a_id: compute_vault_pda(AMM_PROGRAM, pool_id, token_small),
        vault_b_id: compute_vault_pda(AMM_PROGRAM, pool_id, token_large),
        liquidity_pool_id: compute_liquidity_token_pda(AMM_PROGRAM, pool_id),
        liquidity_pool_supply: 1_000,
        reserve_a: 1_000,
        reserve_b: 1_000,
        fees: 30,
    };

    let holding = AccountId::new([9; 32]);
    let plan = swap_exact_in_plan(SwapExactInPlanRequest {
        amm_program_id: amm_program_id(),
        token_in_id: account_id_hex(token_small),
        token_out_id: account_id_hex(token_large),
        config: account_read(compute_config_pda(AMM_PROGRAM), &config_account()),
        user_input_holding_id: account_id_hex(holding),
        user_output_holding_id: account_id_hex(holding),
        amount_in: String::from("100"),
        min_out: String::from("0"),
        deadline_ms: String::from("0"),
        pool_data: hex::encode(borsh::to_vec(&pool).unwrap()),
    })
    .unwrap();

    // Slots 2 and 3 are vault_a / vault_b — in the pool's stored order, not the
    // canonical order. (A domain error would leave accountIds absent, so these
    // also assert the plan succeeded.)
    assert_eq!(plan["accountIds"][2], account_id_hex(pool.vault_a_id));
    assert_eq!(plan["accountIds"][3], account_id_hex(pool.vault_b_id));
    // Guard: the stored vault_a genuinely differs from the canonical derivation
    // (the pre-fix bug would have emitted this one in slot 2).
    assert_ne!(
        pool.vault_a_id,
        compute_vault_pda(AMM_PROGRAM, pool_id, token_large)
    );
}

#[test]
fn swap_exact_in_plan_missing_pool_fails_closed_with_err() {
    // A valid config (so derive_pair succeeds) but undecodable pool_data must
    // surface as Err("no_pool") — NOT an Ok envelope. The FFI wraps Ok as
    // { ok: true, .. }, so an Ok envelope would leave AmmModuleImpl's
    // `planResult.ok` true and let it submit a tx with empty account/instruction
    // vectors. Failing closed matches swap_exact_out_plan.
    let token_a = AccountId::new([1; 32]);
    let token_b = AccountId::new([2; 32]);
    let holding = AccountId::new([9; 32]);
    let err = swap_exact_in_plan(SwapExactInPlanRequest {
        amm_program_id: amm_program_id(),
        token_in_id: account_id_hex(token_a),
        token_out_id: account_id_hex(token_b),
        config: account_read(compute_config_pda(AMM_PROGRAM), &config_account()),
        user_input_holding_id: account_id_hex(holding),
        user_output_holding_id: account_id_hex(holding),
        amount_in: String::from("100"),
        min_out: String::from("0"),
        deadline_ms: String::from("0"),
        pool_data: String::new(), // absent/undecodable
    })
    .unwrap_err();
    assert_eq!(err, "no_pool");
}

#[test]
fn swap_exact_out_plan_uses_the_pool_stored_vaults_not_canonical_order() {
    // Same non-canonical pool as the exact-input case: the exact-output plan must
    // likewise emit the pool's stored vaults, not the canonical derivation.
    let token_small = AccountId::new([1; 32]);
    let token_large = AccountId::new([2; 32]);
    assert!(is_canonical_pair(token_large, token_small)); // large is canonical token_a

    let pool_id = compute_pool_pda(AMM_PROGRAM, token_small, token_large);
    let pool = PoolDefinition {
        definition_token_a_id: token_small, // stored non-canonically (small first)
        definition_token_b_id: token_large,
        vault_a_id: compute_vault_pda(AMM_PROGRAM, pool_id, token_small),
        vault_b_id: compute_vault_pda(AMM_PROGRAM, pool_id, token_large),
        liquidity_pool_id: compute_liquidity_token_pda(AMM_PROGRAM, pool_id),
        liquidity_pool_supply: 1_000,
        reserve_a: 1_000,
        reserve_b: 1_000,
        fees: 30,
    };

    let holding = AccountId::new([9; 32]);
    let plan = swap_exact_out_plan(SwapExactOutPlanRequest {
        amm_program_id: amm_program_id(),
        token_in_id: account_id_hex(token_small),
        token_out_id: account_id_hex(token_large),
        config: account_read(compute_config_pda(AMM_PROGRAM), &config_account()),
        user_input_holding_id: account_id_hex(holding),
        user_output_holding_id: account_id_hex(holding),
        amount_out: String::from("100"),
        max_in: String::from("1000"),
        deadline_ms: String::from("0"),
        pool_data: hex::encode(borsh::to_vec(&pool).unwrap()),
    })
    .unwrap();

    assert_eq!(plan["accountIds"][2], account_id_hex(pool.vault_a_id));
    assert_eq!(plan["accountIds"][3], account_id_hex(pool.vault_b_id));
    assert_ne!(
        pool.vault_a_id,
        compute_vault_pda(AMM_PROGRAM, pool_id, token_large)
    );
}
