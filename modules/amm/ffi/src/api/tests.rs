use alloy_primitives::U256;
use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_protocol_fee_pda, compute_vault_pda, isqrt_product, AmmConfig, Instruction,
    PoolDefinition, MINIMUM_LIQUIDITY,
};
use clock_core::{clock_account_id, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::account::{AccountId, ShardData};
use pretty_assertions::assert_eq;
use serde_json::json;
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda,
};

use super::{
    admin::{transfer_ownership_plan, withdraw_protocol_fees_plan},
    clock::clock_read,
    config::config_account as decode_config_account,
    context::resolve_tokens,
    envelope::identity_of,
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
    account::{account_id_hex, account_read, program_id_base58, WalletAccount, WalletAccountData},
    AccountRead,
};

const AMM_PROGRAM: AccountId = AccountId::new([11; 32]);
const TOKEN_PROGRAM: AccountId = AccountId::new([22; 32]);
const TWAP_PROGRAM: AccountId = AccountId::new([33; 32]);

/// A read of an address no program has written to: v0.2.5's `Account::default()`,
/// which is now simply an empty shard map. Decoding any program's shard of it
/// yields the same answer the old `account == Account::default()` check gave.
fn empty_read(id: AccountId) -> AccountRead {
    AccountRead {
        id: account_id_hex(id),
        status: String::from("ok"),
        account: Some(WalletAccount {
            nonce: hex::encode(0_u128.to_le_bytes()),
            data: WalletAccountData::default(),
        }),
    }
}

/// The namespace root (config PDA id) the tests derive pools under. A fixed `(owner, nonce)`
/// instance under `AMM_PROGRAM`; the pool derivations and the config `account_read` id must both
/// use this so `derive_pair` (which reads the config's id back) lines up with the expected pools.
fn config_id() -> AccountId {
    compute_config_pda(AMM_PROGRAM, AccountId::new([0x50; 32]), [0; 32])
}

fn config_account() -> ShardData {
    ShardData::from(&AmmConfig {
        token_program_id: TOKEN_PROGRAM,
        twap_oracle_program_id: TWAP_PROGRAM,
        authority: AccountId::new([7; 32]),
        swap_fee_bps: 30,
        protocol_fee_bps: 2_000,
    })
}

/// The config fields every instruction declares, matching `config_account`.
fn facts() -> amm_core::ConfigFacts {
    amm_core::ConfigFacts {
        token_program_id: TOKEN_PROGRAM,
        twap_oracle_program_id: TWAP_PROGRAM,
        authority: AccountId::new([7; 32]),
        swap_fee_bps: 30,
        protocol_fee_bps: 2_000,
    }
}

fn token_definition(name: &str, supply: u128) -> ShardData {
    ShardData::from(&TokenDefinition::Fungible {
        name: String::from(name),
        total_supply: supply,
        metadata_id: None,
        authority: None,
    })
}

fn token_holding(definition_id: AccountId, balance: u128) -> ShardData {
    ShardData::from(&TokenHolding::Fungible {
        definition_id,
        balance,
    })
}

fn ids() -> PairIds {
    let token_a = AccountId::new([2; 32]);
    let token_b = AccountId::new([1; 32]);
    let config = config_id();
    let pool = compute_pool_pda(AMM_PROGRAM, config, token_a, token_b);
    PairIds {
        amm_program: AMM_PROGRAM,
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
        twap_oracle_program: TWAP_PROGRAM,
        facts: facts(),
    }
}

fn amm_program_id() -> String {
    account_id_hex(AMM_PROGRAM)
}

/// The pool record the oracle-setup plans read their declared `PoolIdentity` and
/// reserves out of.
fn pool_definition(config: AccountId, token_a: AccountId, token_b: AccountId) -> PoolDefinition {
    let pool = compute_pool_pda(AMM_PROGRAM, config, token_a, token_b);
    PoolDefinition {
        definition_token_a_id: token_a,
        definition_token_b_id: token_b,
        vault_a_id: compute_vault_pda(AMM_PROGRAM, pool, token_a),
        vault_b_id: compute_vault_pda(AMM_PROGRAM, pool, token_b),
        liquidity_pool_id: compute_liquidity_token_pda(AMM_PROGRAM, pool),
        liquidity_pool_supply: 2_000,
        reserve_a: 1_000,
        reserve_b: 4_000,
    }
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
        config: account_read(config_id, AMM_PROGRAM, &config_account()),
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
        config: account_read(pair.config, AMM_PROGRAM, &config_account()),
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
        config: empty_read(pair.config),
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
        config: account_read(config_id, AMM_PROGRAM, &config_account()),
        token_ids: vec![
            account_id_hex(held),
            account_id_hex(listed),
            account_id_hex(missing),
        ],
        wallet_accounts: vec![account_read(
            AccountId::new([6; 32]),
            TOKEN_PROGRAM,
            &token_holding(held, 42),
        )],
        token_definitions: vec![
            account_read(held, TOKEN_PROGRAM, &token_definition("Held", 1_000)),
            account_read(listed, TOKEN_PROGRAM, &token_definition("Listed", 2_000)),
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
        config: account_read(config_id, AMM_PROGRAM, &config_account()),
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
    let bytes: Vec<u8> = plan["instruction"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
        .collect();
    let Instruction::UpdateConfig {
        new_authority: decoded,
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
    let plan = withdraw_protocol_fees_plan(WithdrawProtocolFeesPlanRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, AMM_PROGRAM, &config_account()),
        token_definition_id: account_id_hex(token_def),
        destination_id: account_id_hex(destination),
        amount: "500".into(),
    })
    .unwrap();

    // Accounts: [config, protocol-fee PDA (for token_def), destination, admin authority (signs)].
    // The admin is [7; 32] (the config_account fixture's authority); the protocol-fee holding is
    // the instance-wide PDA for token_def; `amount` is instruction data, not an account.
    assert_eq!(
        plan["accountIds"],
        json!([
            account_id_hex(config_id),
            account_id_hex(compute_protocol_fee_pda(AMM_PROGRAM, config_id, token_def)),
            account_id_hex(destination),
            account_id_hex(AccountId::new([7; 32])),
        ])
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([false, false, false, true])
    );
    // The config is the AMM's shard; the two holdings are the Token Program's. The
    // authority slot wants a signature and nothing else, so it names the program
    // being called -- nothing is written there for a `program_account_id` to mean.
    assert_eq!(
        plan["programAccountIds"],
        json!([
            account_id_hex(AMM_PROGRAM),
            account_id_hex(TOKEN_PROGRAM),
            account_id_hex(TOKEN_PROGRAM),
            account_id_hex(AMM_PROGRAM),
        ])
    );

    // The instruction decodes back to WithdrawProtocolFees with the declared facts and
    // the token descriptor that replaced the two holding reads.
    let bytes: Vec<u8> = plan["instruction"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
        .collect();
    let Instruction::WithdrawProtocolFees {
        amount,
        facts: declared_facts,
        descriptor,
    } = borsh::from_slice(&bytes).unwrap()
    else {
        panic!("expected WithdrawProtocolFees");
    };
    assert_eq!(amount, 500);
    assert_eq!(declared_facts, facts());
    assert_eq!(descriptor.definition_id, token_def);
    assert_eq!(descriptor.kind, token_core::TokenKind::Fungible);
}

#[test]
fn create_price_observations_plan_targets_the_window_feed_accounts() {
    let token_a = AccountId::new([2; 32]);
    let token_b = AccountId::new([1; 32]);
    let config_id = config_id();
    let pool = compute_pool_pda(AMM_PROGRAM, config_id, token_a, token_b);
    let window = 3_600_000_u64;

    let pool_definition = pool_definition(config_id, token_a, token_b);
    let current_tick_id = compute_current_tick_account_pda(TWAP_PROGRAM, pool);
    let plan = create_price_observations_plan(CreatePriceObservationsPlanRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, AMM_PROGRAM, &config_account()),
        token_a_id: account_id_hex(token_a),
        token_b_id: account_id_hex(token_b),
        window_duration_ms: window,
        pool_data: hex::encode(borsh::to_vec(&pool_definition).unwrap()),
        // The feed's initial tick comes from the oracle's own current-tick shard,
        // read here and pinned on-chain by a guard on it.
        current_tick: account_read(
            current_tick_id,
            TWAP_PROGRAM,
            &ShardData::from(&twap_oracle_core::CurrentTickAccount {
                tick: -4_200,
                last_updated: 900,
            }),
        ),
        clock: clock_read(1_000),
    })
    .unwrap();

    // config, pool (price source), current_tick (initial tick), price_observations (init), clock.
    assert_eq!(
        plan["accountIds"],
        json!([
            account_id_hex(config_id),
            account_id_hex(pool),
            account_id_hex(compute_current_tick_account_pda(TWAP_PROGRAM, pool)),
            account_id_hex(compute_price_observations_pda(TWAP_PROGRAM, pool, window)),
            account_id_hex(CLOCK_01_PROGRAM_ACCOUNT_ID),
        ])
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([false, false, false, false, false])
    );
    // The config and the pool are the AMM's shards; the tick and the feed being
    // created are the configured oracle's, since it is the program that writes them
    // through the chained call.
    assert_eq!(
        plan["programAccountIds"],
        json!([
            account_id_hex(AMM_PROGRAM),
            account_id_hex(AMM_PROGRAM),
            account_id_hex(TWAP_PROGRAM),
            account_id_hex(TWAP_PROGRAM),
            account_id_hex(clock_account_id()),
        ])
    );

    let bytes: Vec<u8> = plan["instruction"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
        .collect();
    let Instruction::CreatePriceObservations {
        window_duration,
        facts: declared_facts,
        identity,
        initial_tick,
        now,
    } = borsh::from_slice(&bytes).unwrap()
    else {
        panic!("expected CreatePriceObservations");
    };
    assert_eq!(window_duration, window);
    assert_eq!(declared_facts, facts());
    assert_eq!(identity, identity_of(&pool_definition));
    assert_eq!(initial_tick, -4_200);
    assert_eq!(now, 1_000);
}

#[test]
fn create_oracle_price_account_plan_targets_the_window_price_account() {
    let token_a = AccountId::new([2; 32]);
    let token_b = AccountId::new([1; 32]);
    let config_id = config_id();
    let pool = compute_pool_pda(AMM_PROGRAM, config_id, token_a, token_b);
    let window = 900_000_u64;

    let pool_definition = pool_definition(config_id, token_a, token_b);
    let plan = create_oracle_price_account_plan(CreateOraclePriceAccountPlanRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, AMM_PROGRAM, &config_account()),
        token_a_id: account_id_hex(token_a),
        token_b_id: account_id_hex(token_b),
        window_duration_ms: window,
        pool_data: hex::encode(borsh::to_vec(&pool_definition).unwrap()),
        clock: clock_read(1_000),
    })
    .unwrap();

    // config, pool (price source), oracle_price_account (init), clock.
    assert_eq!(
        plan["accountIds"],
        json!([
            account_id_hex(config_id),
            account_id_hex(pool),
            account_id_hex(compute_oracle_price_account_pda(TWAP_PROGRAM, pool, window)),
            account_id_hex(CLOCK_01_PROGRAM_ACCOUNT_ID),
        ])
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([false, false, false, false])
    );
    assert_eq!(
        plan["programAccountIds"],
        json!([
            account_id_hex(AMM_PROGRAM),
            account_id_hex(AMM_PROGRAM),
            account_id_hex(TWAP_PROGRAM),
            account_id_hex(clock_account_id()),
        ])
    );

    let bytes: Vec<u8> = plan["instruction"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
        .collect();
    let Instruction::CreateOraclePriceAccount {
        window_duration,
        facts: declared_facts,
        identity,
        reserve_a,
        reserve_b,
        now,
    } = borsh::from_slice(&bytes).unwrap()
    else {
        panic!("expected CreateOraclePriceAccount");
    };
    assert_eq!(window_duration, window);
    assert_eq!(declared_facts, facts());
    assert_eq!(identity, identity_of(&pool_definition));
    // The feed is seeded at the pool's own spot price, derived from reserves the
    // pool's guard pins *exactly* -- so these are the live ones, not a caller's.
    assert_eq!((reserve_a, reserve_b), (1_000_u128, 4_000_u128));
    assert_eq!(now, 1_000);
}

#[test]
fn config_account_decodes_authority_and_program_ids() {
    let config_id = config_id();
    let value = decode_config_account(ConfigAccountRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, AMM_PROGRAM, &config_account()),
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
        config: empty_read(config_id),
    })
    .unwrap();

    assert_eq!(value["status"], "error");
    assert_eq!(value["error"], "config_unavailable");
}

#[test]
fn an_account_no_program_has_written_to_decodes_as_no_shard() {
    // v0.2.5 read this back as a real `Account::default()` and the callers compared
    // against it. There is no such value any more: an address nobody has written to
    // carries an empty shard map, so asking for a program's shard of it is what
    // reports "nothing here".
    let id = AccountId::new([5; 32]);
    let read = empty_read(id);
    assert_eq!(
        crate::account::decode_shard(&read, AMM_PROGRAM),
        Err(crate::account::AccountReadError::NoShard)
    );
    // And an empty shard under the right key says the same thing, rather than
    // decoding to zero bytes of config.
    let empty_shard = account_read(id, AMM_PROGRAM, &ShardData::default());
    assert_eq!(
        crate::account::decode_shard(&empty_shard, AMM_PROGRAM),
        Err(crate::account::AccountReadError::NoShard)
    );
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
        config: account_read(config_id(), AMM_PROGRAM, &config_account()),
        user_input_holding_id: account_id_hex(holding),
        user_output_holding_id: account_id_hex(holding),
        amount_in: String::from("100"),
        min_out: String::from("0"),
        deadline_ms: String::from("0"),
        clock: clock_read(1_000),
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
    // Slot 8 is the 9th account the guest now requires: the INPUT token's instance-wide
    // protocol-fee holding PDA. Only the input holding (slot 4) signs; the protocol PDA does not.
    assert_eq!(plan["accountIds"].as_array().unwrap().len(), 9);
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
        json!([false, false, false, false, true, false, false, false, false])
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
        config: account_read(config_id(), AMM_PROGRAM, &config_account()),
        user_input_holding_id: account_id_hex(holding),
        user_output_holding_id: account_id_hex(holding),
        amount_in: String::from("100"),
        min_out: String::from("0"),
        deadline_ms: String::from("0"),
        clock: clock_read(1_000),
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
        config: account_read(config_id(), AMM_PROGRAM, &config_account()),
        user_input_holding_id: account_id_hex(holding),
        user_output_holding_id: account_id_hex(holding),
        amount_out: String::from("100"),
        max_in: String::from("1000"),
        deadline_ms: String::from("0"),
        clock: clock_read(1_000),
        pool_data: hex::encode(borsh::to_vec(&pool).unwrap()),
    })
    .unwrap();

    assert_eq!(plan["accountIds"][2], account_id_hex(pool.vault_a_id));
    assert_eq!(plan["accountIds"][3], account_id_hex(pool.vault_b_id));
    assert_ne!(
        pool.vault_a_id,
        compute_vault_pda(AMM_PROGRAM, pool_id, token_large)
    );
    // Slot 8 is the input token's protocol-fee holding PDA (the 9th account); only slot 4 signs.
    assert_eq!(plan["accountIds"].as_array().unwrap().len(), 9);
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
        json!([false, false, false, false, true, false, false, false, false])
    );
}

/// The envelope the module's C++ builds, parsed from JSON rather than constructed.
///
/// Every other test here builds `AccountRead` as a struct, so nothing checks that
/// the two halves of the seam agree on field names -- and they are in different
/// languages, with no shared schema. `shardedRead` in
/// `modules/amm/src/amm_module_impl.cpp` emits exactly this: the module's own
/// `id`/`status` envelope around an object shaped like `lee_core::Account`, with
/// `lez_core`'s array of `{program, data}` already re-keyed into a map.
#[test]
fn the_account_read_the_module_emits_deserializes_and_decodes() {
    let read: AccountRead = serde_json::from_value(json!({
        "id": account_id_hex(config_id()),
        "status": "ok",
        "account": {
            "nonce": "00000000000000000000000000000000",
            "data": {
                "shards": {
                    account_id_hex(AMM_PROGRAM): hex::encode(config_account().as_ref()),
                    // A foreign shard at the same address must not disturb the
                    // decode: the whole-account read returns every shard anyone
                    // wrote here.
                    account_id_hex(TOKEN_PROGRAM): "ff",
                }
            }
        }
    }))
    .expect("the module's account-read JSON must deserialize");

    let value = decode_config_account(ConfigAccountRequest {
        amm_program_id: amm_program_id(),
        config: read,
    })
    .unwrap();
    assert_eq!(value["status"], "ok");
    assert_eq!(value["authority"], AccountId::new([7; 32]).to_string());

    // The same envelope with the v0.2.5 flat account inside no longer parses, so
    // an unbumped lez_core fails loudly rather than decoding as an empty account.
    assert!(serde_json::from_value::<AccountRead>(json!({
        "id": account_id_hex(config_id()),
        "status": "ok",
        "account": {
            "program_owner": account_id_hex(AMM_PROGRAM),
            "balance": "00000000000000000000000000000000",
            "nonce": "00000000000000000000000000000000",
            "data": "00",
        }
    }))
    .is_err());
}
