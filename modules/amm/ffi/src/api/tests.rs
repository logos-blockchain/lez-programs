use alloy_primitives::U256;
use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_protocol_fee_pda, compute_vault_pda, isqrt_product, AmmConfig, Message, PoolDefinition,
    MINIMUM_LIQUIDITY,
};
use lee_core::account::{Account, AccountId, ActorState};
use pretty_assertions::assert_eq;
use serde_json::json;
use token_core::{TokenDefinition, TokenHolding};

use super::{
    admin::{transfer_ownership_plan, withdraw_protocol_fees_plan},
    config::config_account as decode_config_account,
    context::resolve_tokens,
    holding::{select_holding, SelectedHolding},
    oracle::{create_oracle_price_account_plan, create_price_observations_plan},
    pair::{is_canonical_pair, pair_ids, PairIds},
    quote::{div_ceil_u256, minimum_opening_pair, Q64},
    swap::{swap_exact_in_plan, swap_exact_out_plan},
    ConfigAccountRequest, CreateOraclePriceAccountPlanRequest, CreatePriceObservationsPlanRequest,
    PairIdsRequest, ResolveTokensRequest, SwapExactInPlanRequest, SwapExactOutPlanRequest,
    TransferOwnershipPlanRequest, WithdrawProtocolFeesPlanRequest,
};
use crate::{
    account::{account_id_hex, account_read, decode_account, program_id_base58},
    AccountRead,
};

const AMM_PROGRAM: AccountId = AccountId::new([11; 32]);
const TOKEN_PROGRAM: AccountId = AccountId::new([22; 32]);
const TWAP_PROGRAM: AccountId = AccountId::new([33; 32]);

fn account(owner: AccountId, data: ActorState) -> Account {
    Account::default().with_actor_state(owner, data)
}

fn default_read(id: AccountId) -> AccountRead {
    account_read(id, &Account::default())
}

/// The namespace root (config PDA id) the tests derive pools under. A fixed `(owner, nonce)`
/// instance under `AMM_PROGRAM`; the pool derivations and the config `account_read` id must both
/// use this so `derive_pair` (which reads the config's id back) lines up with the expected pools.
fn config_id() -> AccountId {
    compute_config_pda(AMM_PROGRAM, AccountId::new([0x50; 32]), [0; 32])
}

fn config_account() -> Account {
    account(
        AMM_PROGRAM,
        ActorState::from(
            borsh::to_vec(&amm_core::AmmState::Config(AmmConfig {
                token_program_id: TOKEN_PROGRAM,
                twap_oracle_program_id: TWAP_PROGRAM,
                authority: AccountId::new([7; 32]),
                swap_fee_bps: 30,
                protocol_fee_bps: 2_000,
            }))
            .unwrap(),
        ),
    )
}

fn token_definition(name: &str, supply: u128) -> Account {
    account(
        TOKEN_PROGRAM,
        ActorState::from(
            borsh::to_vec(&token_core::TokenState::Definition(TokenDefinition {
                name: String::from(name),
                total_supply: supply,
                authority: None,
            }))
            .unwrap(),
        ),
    )
}

fn token_holding(definition_id: AccountId, balance: u128) -> Account {
    account(
        TOKEN_PROGRAM,
        ActorState::from(
            borsh::to_vec(&token_core::TokenState::Holding(TokenHolding {
                definition_id,
                balance,
            }))
            .unwrap(),
        ),
    )
}

fn ids() -> PairIds {
    let token_a = AccountId::new([2; 32]);
    let token_b = AccountId::new([1; 32]);
    let config = config_id();
    let pool = compute_pool_pda(AMM_PROGRAM, config, token_a, token_b);
    PairIds {
        token_a,
        token_b,
        config,
        pool,
        vault_a: compute_vault_pda(AMM_PROGRAM, pool, token_a),
        vault_b: compute_vault_pda(AMM_PROGRAM, pool, token_b),
        lp_definition: compute_liquidity_token_pda(AMM_PROGRAM, pool),
        lp_lock_holding: compute_lp_lock_holding_pda(AMM_PROGRAM, pool),
        token_program: TOKEN_PROGRAM,
        twap_oracle_program: TWAP_PROGRAM,
    }
}

fn amm_program_id() -> String {
    account_id_hex(AMM_PROGRAM)
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
    let config_id = config_id();
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
        account_id_hex(compute_pool_pda(AMM_PROGRAM, config_id, token_a, token_b))
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
    let config_id = config_id();

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
    let config_id = config_id();
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
            account_id_hex(AccountId::new([7; 32])),
            account_id_hex(config_id)
        ])
    );
    assert_eq!(plan["signingRequirements"], json!([true, false]));

    // The instruction decodes back to UpdateConfig { new_authority }.
    let bytes: Vec<u8> = plan["message"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
        .collect();
    let Message::UpdateConfig {
        new_authority: decoded,
        ..
    } = borsh::from_slice(&bytes).unwrap()
    else {
        panic!("expected UpdateConfig");
    };
    assert_eq!(decoded, new_authority);
}

#[test]
fn withdraw_protocol_fees_plan_targets_protocol_pda_destination_and_admin() {
    let config_id = config_id();
    let token_def = AccountId::new([3; 32]);
    let destination = AccountId::new([8; 32]);
    let request = WithdrawProtocolFeesPlanRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, &config_account()),
        token_definition_id: account_id_hex(token_def),
        destination_id: account_id_hex(destination),
        amount: "500".into(),
    };
    let plan = withdraw_protocol_fees_plan(request.clone()).unwrap();

    let authority = AccountId::new([7; 32]);
    assert_eq!(
        plan["accountIds"],
        json!([
            account_id_hex(authority),
            account_id_hex(config_id),
            account_id_hex(compute_protocol_fee_pda(AMM_PROGRAM, config_id, token_def)),
            account_id_hex(destination),
        ])
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([true, false, false, false])
    );

    // The instruction decodes back to WithdrawProtocolFees { amount }.
    let bytes: Vec<u8> = plan["message"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
        .collect();
    let Message::WithdrawProtocolFees { amount, .. } = borsh::from_slice(&bytes).unwrap() else {
        panic!("expected WithdrawProtocolFees");
    };
    assert_eq!(amount, 500);

    let self_plan = withdraw_protocol_fees_plan(WithdrawProtocolFeesPlanRequest {
        destination_id: account_id_hex(authority),
        ..request
    })
    .unwrap();
    assert_eq!(self_plan["accountIds"][0], account_id_hex(authority));
    assert_eq!(self_plan["actorProgramIds"][0], account_id_hex(AMM_PROGRAM));
    assert_eq!(self_plan["accountIds"][3], account_id_hex(authority));
    assert_eq!(
        self_plan["actorProgramIds"][3],
        account_id_hex(TOKEN_PROGRAM)
    );
    assert_eq!(
        self_plan["signingRequirements"],
        json!([true, false, false, true])
    );
}

#[test]
fn create_price_observations_plan_targets_the_window_feed_accounts() {
    let token_a = AccountId::new([2; 32]);
    let token_b = AccountId::new([1; 32]);
    let config_id = config_id();
    let pool = compute_pool_pda(AMM_PROGRAM, config_id, token_a, token_b);
    let window = amm_core::oracle::MIN_WINDOW_DURATION;

    let request = CreatePriceObservationsPlanRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, &config_account()),
        token_a_id: account_id_hex(token_a),
        token_b_id: account_id_hex(token_b),
        window_duration_ms: window,
    };
    assert_eq!(
        create_price_observations_plan(CreatePriceObservationsPlanRequest {
            window_duration_ms: window - 1,
            ..request.clone()
        })
        .unwrap_err(),
        "invalid_window"
    );
    let plan = create_price_observations_plan(request).unwrap();

    assert_eq!(plan["rootMention"], 0);
    assert_eq!(plan["accountIds"][0], account_id_hex(pool));
    assert_eq!(plan["actorProgramIds"][0], account_id_hex(AMM_PROGRAM));
    assert_eq!(plan["accountIds"][2], account_id_hex(pool));
    assert_eq!(plan["actorProgramIds"][2], account_id_hex(TWAP_PROGRAM));
    assert!(plan["signingRequirements"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s == false));

    let bytes: Vec<u8> = plan["message"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
        .collect();
    let Message::CreatePriceObservations { window_duration } = borsh::from_slice(&bytes).unwrap()
    else {
        panic!("expected CreatePriceObservations");
    };
    assert_eq!(window_duration, window);
}

#[test]
fn create_oracle_price_account_plan_targets_the_window_price_account() {
    let token_a = AccountId::new([2; 32]);
    let token_b = AccountId::new([1; 32]);
    let config_id = config_id();
    let pool = compute_pool_pda(AMM_PROGRAM, config_id, token_a, token_b);
    let window = amm_core::oracle::MIN_WINDOW_DURATION;

    let request = CreateOraclePriceAccountPlanRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, &config_account()),
        token_a_id: account_id_hex(token_a),
        token_b_id: account_id_hex(token_b),
        window_duration_ms: window,
    };
    assert_eq!(
        create_oracle_price_account_plan(CreateOraclePriceAccountPlanRequest {
            window_duration_ms: window - 1,
            ..request.clone()
        })
        .unwrap_err(),
        "invalid_window"
    );
    let plan = create_oracle_price_account_plan(request).unwrap();

    assert_eq!(plan["rootMention"], 0);
    assert_eq!(plan["accountIds"][0], account_id_hex(pool));
    assert_eq!(plan["actorProgramIds"][0], account_id_hex(AMM_PROGRAM));
    assert_eq!(plan["accountIds"][2], account_id_hex(pool));
    assert_eq!(plan["actorProgramIds"][2], account_id_hex(TWAP_PROGRAM));
    assert!(plan["signingRequirements"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s == false));

    let bytes: Vec<u8> = plan["message"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
        .collect();
    let Message::CreateOraclePriceAccount { window_duration } = borsh::from_slice(&bytes).unwrap()
    else {
        panic!("expected CreateOraclePriceAccount");
    };
    assert_eq!(window_duration, window);
}

#[test]
fn config_account_decodes_authority_and_program_ids() {
    let config_id = config_id();
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
    // The instance-wide swap fee and protocol-fee split are surfaced from the config (the fixture
    // sets 30 bps swap fee, 2000 bps = 20% of it to the protocol).
    assert_eq!(value["swapFeeBps"], 30);
    assert_eq!(value["protocolFeeBps"], 2_000);
}

#[test]
fn config_account_is_unavailable_when_not_on_chain() {
    let config_id = config_id();
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

    let pool_id = compute_pool_pda(AMM_PROGRAM, config_id(), token_small, token_large);
    let pool = PoolDefinition {
        config_id: AccountId::new([0; 32]),
        definition_token_a_id: token_small, // stored non-canonically (small first)
        definition_token_b_id: token_large,
        vault_a_id: compute_vault_pda(AMM_PROGRAM, pool_id, token_small),
        vault_b_id: compute_vault_pda(AMM_PROGRAM, pool_id, token_large),
        liquidity_pool_id: compute_liquidity_token_pda(AMM_PROGRAM, pool_id),
        liquidity_pool_supply: 1_000,
        reserve_a: 1_000,
        reserve_b: 1_000,
    };

    let holding = AccountId::new([9; 32]);
    let plan = swap_exact_in_plan(SwapExactInPlanRequest {
        amm_program_id: amm_program_id(),
        token_in_id: account_id_hex(token_small),
        token_out_id: account_id_hex(token_large),
        config: account_read(config_id(), &config_account()),
        user_input_holding_id: account_id_hex(holding),
        user_output_holding_id: account_id_hex(AccountId::new([0x99; 32])),
        amount_in: String::from("100"),
        min_out: String::from("0"),
        deadline_ms: String::from("0"),
        pool_data: hex::encode(borsh::to_vec(&amm_core::AmmState::Pool(pool.clone())).unwrap()),
    })
    .unwrap();

    // The vault mentions follow the pool's stored token order.
    assert_eq!(plan["accountIds"][3], account_id_hex(pool.vault_a_id));
    assert_eq!(plan["accountIds"][4], account_id_hex(pool.vault_b_id));
    assert_ne!(
        pool.vault_a_id,
        compute_vault_pda(AMM_PROGRAM, pool_id, token_large)
    );
    // Both protocol-fee holdings are mentioned. The input holding authorizes the debit.
    assert_eq!(plan["accountIds"].as_array().unwrap().len(), 11);
    assert_eq!(
        plan["accountIds"][8],
        account_id_hex(compute_protocol_fee_pda(
            AMM_PROGRAM,
            config_id(),
            token_small
        ))
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([false, false, false, false, false, true, false, false, false, false, false])
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
        config: account_read(config_id(), &config_account()),
        user_input_holding_id: account_id_hex(holding),
        user_output_holding_id: account_id_hex(AccountId::new([0x99; 32])),
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

    let pool_id = compute_pool_pda(AMM_PROGRAM, config_id(), token_small, token_large);
    let pool = PoolDefinition {
        config_id: AccountId::new([0; 32]),
        definition_token_a_id: token_small, // stored non-canonically (small first)
        definition_token_b_id: token_large,
        vault_a_id: compute_vault_pda(AMM_PROGRAM, pool_id, token_small),
        vault_b_id: compute_vault_pda(AMM_PROGRAM, pool_id, token_large),
        liquidity_pool_id: compute_liquidity_token_pda(AMM_PROGRAM, pool_id),
        liquidity_pool_supply: 1_000,
        reserve_a: 1_000,
        reserve_b: 1_000,
    };

    let holding = AccountId::new([9; 32]);
    let plan = swap_exact_out_plan(SwapExactOutPlanRequest {
        amm_program_id: amm_program_id(),
        token_in_id: account_id_hex(token_small),
        token_out_id: account_id_hex(token_large),
        config: account_read(config_id(), &config_account()),
        user_input_holding_id: account_id_hex(holding),
        user_output_holding_id: account_id_hex(AccountId::new([0x99; 32])),
        amount_out: String::from("100"),
        max_in: String::from("1000"),
        deadline_ms: String::from("0"),
        pool_data: hex::encode(borsh::to_vec(&amm_core::AmmState::Pool(pool.clone())).unwrap()),
    })
    .unwrap();

    assert_eq!(plan["accountIds"][3], account_id_hex(pool.vault_a_id));
    assert_eq!(plan["accountIds"][4], account_id_hex(pool.vault_b_id));
    assert_ne!(
        pool.vault_a_id,
        compute_vault_pda(AMM_PROGRAM, pool_id, token_large)
    );
    // The exact-output plan declares the same protocol-fee holdings as the exact-input plan.
    assert_eq!(plan["accountIds"].as_array().unwrap().len(), 11);
    assert_eq!(
        plan["accountIds"][8],
        account_id_hex(compute_protocol_fee_pda(
            AMM_PROGRAM,
            config_id(),
            token_small
        ))
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([false, false, false, false, false, true, false, false, false, false, false])
    );
}

pub(super) fn empty_pool() -> PoolDefinition {
    PoolDefinition {
        config_id: AccountId::new([0; 32]),
        definition_token_a_id: AccountId::new([0; 32]),
        definition_token_b_id: AccountId::new([0; 32]),
        vault_a_id: AccountId::new([0; 32]),
        vault_b_id: AccountId::new([0; 32]),
        liquidity_pool_id: AccountId::new([0; 32]),
        liquidity_pool_supply: 0,
        reserve_a: 0,
        reserve_b: 0,
    }
}

#[test]
fn holdings_select_the_configured_actor_and_reject_malformed_state() {
    let id = AccountId::new([0x91; 32]);
    let definition = AccountId::new([0x92; 32]);
    let mut holding = token_holding(definition, 42);
    holding
        .data
        .set_actor_state(AMM_PROGRAM, ActorState::from(vec![255]));
    let read = account_read(id, &holding);
    let selected = super::holding::decode_fungible_holding(&read, TOKEN_PROGRAM).unwrap();
    assert_eq!(selected.definition_id, definition);
    assert_eq!(selected.balance, 42);
    assert!(super::holding::decode_fungible_holding(&read, AMM_PROGRAM).is_err());
    holding
        .data
        .set_actor_state(TOKEN_PROGRAM, ActorState::from(vec![255]));
    assert!(
        super::holding::decode_fungible_holding(&account_read(id, &holding), TOKEN_PROGRAM)
            .is_err()
    );
}

#[test]
fn tagged_pool_snapshots_reject_legacy_and_transient_state() {
    let pool = empty_pool();
    assert!(crate::account::decode_pool(&borsh::to_vec(&pool).unwrap()).is_err());
    let busy = amm_core::AmmState::Settling(Box::new(amm_core::AmmState::Pool(pool.clone())));
    assert!(crate::account::decode_pool(&busy.encode()).is_err());
    assert_eq!(
        crate::account::decode_pool(&amm_core::AmmState::Pool(pool.clone()).encode()).unwrap(),
        pool
    );
}
