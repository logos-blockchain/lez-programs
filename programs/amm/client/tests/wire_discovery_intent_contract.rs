mod common;

use amm_client::wire::{quote_json, WIRE_SCHEMA};
use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_vault_pda, AmmConfig, PoolDefinition, FEE_TIER_BPS_30, MINIMUM_LIQUIDITY,
};
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use common::program_id_hex;
use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::{compute_current_tick_account_pda, CurrentTickAccount};

const AMM_PROGRAM_ID: ProgramId = [42; 8];
const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];
const Q64_64_ONE: u128 = 1_u128 << 64;

fn config_snapshot() -> Value {
    let config = AmmConfig {
        token_program_id: TOKEN_PROGRAM_ID,
        twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
        authority: AccountId::new([9; 32]),
    };
    snapshot(
        compute_config_pda(AMM_PROGRAM_ID),
        &Account {
            program_owner: AMM_PROGRAM_ID,
            balance: 0,
            data: Data::from(&config),
            nonce: Nonce(0),
        },
    )
}

fn account(program_owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner,
        balance: 0,
        data,
        nonce: Nonce(0),
    }
}

fn fungible_definition(total_supply: u128, authority: Option<AccountId>) -> Account {
    account(
        TOKEN_PROGRAM_ID,
        Data::from(&TokenDefinition::Fungible {
            name: String::from("Token"),
            total_supply,
            metadata_id: None,
            authority,
        }),
    )
}

fn fungible_holding(program_owner: ProgramId, definition_id: AccountId, balance: u128) -> Account {
    account(
        program_owner,
        Data::from(&TokenHolding::Fungible {
            definition_id,
            balance,
        }),
    )
}

fn clock_account() -> Account {
    let bytes = ClockAccountData {
        block_id: 123,
        timestamp: 456,
    }
    .to_bytes();
    account(
        [88; 8],
        Data::try_from(bytes).expect("clock account data must fit"),
    )
}

struct PairIds {
    first_token_id: AccountId,
    second_token_id: AccountId,
    pool_id: AccountId,
    first_vault_id: AccountId,
    second_vault_id: AccountId,
    liquidity_definition_id: AccountId,
    lp_lock_holding_id: AccountId,
    current_tick_id: AccountId,
}

impl PairIds {
    fn new() -> Self {
        let first_token_id = AccountId::new([1; 32]);
        let second_token_id = AccountId::new([2; 32]);
        let pool_id = compute_pool_pda(AMM_PROGRAM_ID, second_token_id, first_token_id);
        Self {
            first_token_id,
            second_token_id,
            pool_id,
            first_vault_id: compute_vault_pda(AMM_PROGRAM_ID, pool_id, first_token_id),
            second_vault_id: compute_vault_pda(AMM_PROGRAM_ID, pool_id, second_token_id),
            liquidity_definition_id: compute_liquidity_token_pda(AMM_PROGRAM_ID, pool_id),
            lp_lock_holding_id: compute_lp_lock_holding_pda(AMM_PROGRAM_ID, pool_id),
            current_tick_id: compute_current_tick_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id),
        }
    }

    fn inspect_request(&self, snapshots: Value) -> Value {
        json!({
            "operation": "inspect_pair",
            "ammProgramId": program_id_hex(AMM_PROGRAM_ID),
            "config": config_snapshot(),
            "firstTokenDefinitionId": self.first_token_id.to_string(),
            "secondTokenDefinitionId": self.second_token_id.to_string(),
            "snapshots": snapshots,
        })
    }

    fn missing_snapshots(&self) -> Value {
        json!({
            "pool": snapshot(self.pool_id, &Account::default()),
            "firstTokenDefinition": snapshot(
                self.first_token_id,
                &fungible_definition(10_000, None),
            ),
            "secondTokenDefinition": snapshot(
                self.second_token_id,
                &fungible_definition(20_000, None),
            ),
            "firstTokenVault": snapshot(
                self.first_vault_id,
                &fungible_holding(TOKEN_PROGRAM_ID, self.first_token_id, 7),
            ),
            "secondTokenVault": snapshot(self.second_vault_id, &Account::default()),
            "liquidityDefinition": snapshot(
                self.liquidity_definition_id,
                &Account::default(),
            ),
            "lpLockHolding": snapshot(self.lp_lock_holding_id, &Account::default()),
            "currentTick": snapshot(self.current_tick_id, &Account::default()),
            "clock": snapshot(CLOCK_01_PROGRAM_ACCOUNT_ID, &clock_account()),
        })
    }

    fn active_snapshots(&self) -> Value {
        let pool = PoolDefinition {
            definition_token_a_id: self.second_token_id,
            definition_token_b_id: self.first_token_id,
            vault_a_id: self.second_vault_id,
            vault_b_id: self.first_vault_id,
            liquidity_pool_id: self.liquidity_definition_id,
            liquidity_pool_supply: 2_000,
            reserve_a: 1_000,
            reserve_b: 500,
            fees: FEE_TIER_BPS_30,
        };
        json!({
            "pool": snapshot(self.pool_id, &account(AMM_PROGRAM_ID, Data::from(&pool))),
            "firstTokenDefinition": snapshot(
                self.first_token_id,
                &fungible_definition(10_000, None),
            ),
            "secondTokenDefinition": snapshot(
                self.second_token_id,
                &fungible_definition(20_000, None),
            ),
            "firstTokenVault": snapshot(
                self.first_vault_id,
                &fungible_holding(TOKEN_PROGRAM_ID, self.first_token_id, 550),
            ),
            "secondTokenVault": snapshot(
                self.second_vault_id,
                &fungible_holding(TOKEN_PROGRAM_ID, self.second_token_id, 1_100),
            ),
            "liquidityDefinition": snapshot(
                self.liquidity_definition_id,
                &fungible_definition(2_000, Some(self.liquidity_definition_id)),
            ),
            "lpLockHolding": snapshot(
                self.lp_lock_holding_id,
                &fungible_holding(
                    TOKEN_PROGRAM_ID,
                    self.liquidity_definition_id,
                    MINIMUM_LIQUIDITY,
                ),
            ),
            "currentTick": snapshot(
                self.current_tick_id,
                &account(
                    TWAP_ORACLE_PROGRAM_ID,
                    Data::from(&CurrentTickAccount {
                        tick: -1,
                        last_updated: 400,
                    }),
                ),
            ),
            "clock": snapshot(CLOCK_01_PROGRAM_ACCOUNT_ID, &clock_account()),
        })
    }
}

fn snapshot(id: AccountId, account: &Account) -> Value {
    json!({
        "id": id.to_string(),
        "programOwner": program_id_hex(account.program_owner),
        "balance": account.balance.to_string(),
        "nonce": account.nonce.0.to_string(),
        "data": account
            .data
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    })
}

fn assert_decimal_string(value: &Value) {
    let text = value.as_str().expect("wire amount must be a string");
    assert!(
        !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()),
        "wire amount must be unsigned decimal"
    );
}

#[test]
fn discovery_operations_return_exact_string_account_ids() {
    let first_token_id = AccountId::new([1; 32]);
    let second_token_id = AccountId::new([2; 32]);

    let config_id = quote_json(json!({
        "operation": "derive_config_id",
        "ammProgramId": program_id_hex(AMM_PROGRAM_ID),
    }))
    .expect("legacy schema-less request remains accepted");
    assert_eq!(config_id["schema"], WIRE_SCHEMA);
    assert_eq!(
        config_id["configId"],
        compute_config_pda(AMM_PROGRAM_ID).to_string()
    );

    let config = config_snapshot();
    let inspected = quote_json(json!({
        "schema": WIRE_SCHEMA,
        "operation": "inspect_config",
        "ammProgramId": program_id_hex(AMM_PROGRAM_ID),
        "config": config.clone(),
    }))
    .expect("config must inspect");
    assert_eq!(inspected["schema"], WIRE_SCHEMA);
    assert_eq!(inspected["ammProgramId"], program_id_hex(AMM_PROGRAM_ID));
    assert_eq!(
        inspected["tokenProgramId"],
        program_id_hex(TOKEN_PROGRAM_ID)
    );
    assert_eq!(
        inspected["twapOracleProgramId"],
        program_id_hex(TWAP_ORACLE_PROGRAM_ID)
    );
    assert_eq!(inspected["authority"], AccountId::new([9; 32]).to_string());

    let canonical = quote_json(json!({
        "operation": "canonical_pair",
        "firstTokenDefinitionId": first_token_id.to_string(),
        "secondTokenDefinitionId": second_token_id.to_string(),
    }))
    .expect("distinct pair must canonicalize");
    assert_eq!(canonical["tokenAId"], second_token_id.to_string());
    assert_eq!(canonical["tokenBId"], first_token_id.to_string());

    let manifest = quote_json(json!({
        "operation": "derive_pair_read_manifest",
        "ammProgramId": program_id_hex(AMM_PROGRAM_ID),
        "config": config,
        "firstTokenDefinitionId": first_token_id.to_string(),
        "secondTokenDefinitionId": second_token_id.to_string(),
    }))
    .expect("pair read manifest must derive");
    let pool_id = compute_pool_pda(AMM_PROGRAM_ID, second_token_id, first_token_id);
    assert_eq!(manifest["poolId"], pool_id.to_string());
    assert_eq!(
        manifest["firstToken"]["definitionId"],
        first_token_id.to_string()
    );
    assert_eq!(
        manifest["firstToken"]["vaultId"],
        compute_vault_pda(AMM_PROGRAM_ID, pool_id, first_token_id).to_string()
    );
    assert_eq!(
        manifest["secondToken"]["vaultId"],
        compute_vault_pda(AMM_PROGRAM_ID, pool_id, second_token_id).to_string()
    );
    assert_eq!(
        manifest["liquidityDefinitionId"],
        compute_liquidity_token_pda(AMM_PROGRAM_ID, pool_id).to_string()
    );
    assert_eq!(
        manifest["lpLockHoldingId"],
        compute_lp_lock_holding_pda(AMM_PROGRAM_ID, pool_id).to_string()
    );
    assert_eq!(
        manifest["currentTickId"],
        compute_current_tick_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id).to_string()
    );
    assert_eq!(manifest["clockId"], CLOCK_01_PROGRAM_ACCOUNT_ID.to_string());
}

#[test]
fn inspect_pair_reports_missing_caller_ordered_state() {
    let ids = PairIds::new();
    let inspected = quote_json(ids.inspect_request(ids.missing_snapshots()))
        .expect("missing pair snapshots must inspect");

    assert_eq!(inspected["status"], "missing");
    assert_eq!(inspected["manifest"]["poolId"], ids.pool_id.to_string());
    assert_eq!(
        inspected["firstTokenDefinition"]["id"],
        ids.first_token_id.to_string()
    );
    assert_eq!(inspected["firstTokenDefinition"]["totalSupply"], "10000");
    assert_eq!(
        inspected["secondTokenDefinition"]["id"],
        ids.second_token_id.to_string()
    );
    assert_eq!(inspected["secondTokenDefinition"]["totalSupply"], "20000");
    assert_eq!(inspected["firstVault"]["status"], "existing_fungible");
    assert_eq!(inspected["firstVault"]["balance"], "7");
    assert_eq!(inspected["secondVault"]["status"], "uninitialized");
    assert_eq!(inspected["clock"]["blockId"], "123");
    assert_eq!(inspected["clock"]["timestamp"], "456");
}

#[test]
fn inspect_pair_reports_active_stored_state_for_reversed_caller_order() {
    let ids = PairIds::new();
    let inspected = quote_json(ids.inspect_request(ids.active_snapshots()))
        .expect("active pair snapshots must inspect");

    assert_eq!(inspected["status"], "active");
    assert_eq!(inspected["callerOrder"], "reversed");
    assert_eq!(
        inspected["stored"]["tokenADefinitionId"],
        ids.second_token_id.to_string()
    );
    assert_eq!(
        inspected["stored"]["tokenBDefinitionId"],
        ids.first_token_id.to_string()
    );
    assert_eq!(
        inspected["stored"]["vaultAId"],
        ids.second_vault_id.to_string()
    );
    assert_eq!(
        inspected["stored"]["vaultBId"],
        ids.first_vault_id.to_string()
    );
    assert_eq!(inspected["stored"]["reserveA"], "1000");
    assert_eq!(inspected["stored"]["reserveB"], "500");
    assert_eq!(inspected["stored"]["vaultABalance"], "1100");
    assert_eq!(inspected["stored"]["vaultBBalance"], "550");
    assert_eq!(inspected["stored"]["liquidityPoolSupply"], "2000");
    assert_eq!(inspected["stored"]["lpLockBalance"], "1000");
    assert_eq!(inspected["stored"]["feeBps"], "30");
    assert_eq!(
        inspected["storedSpotPriceQ64_64"],
        (Q64_64_ONE / 2).to_string()
    );
    assert_eq!(inspected["currentTick"]["tick"], "-1");
    assert_eq!(inspected["currentTick"]["lastUpdated"], "400");
    assert_eq!(inspected["clock"]["blockId"], "123");
    assert_eq!(inspected["clock"]["timestamp"], "456");
}

#[test]
fn inspect_pair_preserves_stable_snapshot_validation_errors() {
    let ids = PairIds::new();
    let mut snapshots = ids.missing_snapshots();
    snapshots["pool"]["id"] = Value::String(AccountId::new([99; 32]).to_string());

    let error = quote_json(ids.inspect_request(snapshots))
        .expect_err("wrong pool snapshot ID must fail before lifecycle inspection");
    assert_eq!(error.code(), "account_id_mismatch");
}

#[test]
fn opening_intent_operations_preserve_lossless_decimal_values() {
    let desired_price = Q64_64_ONE.checked_mul(2).expect("test price fits");
    let fee_bps = FEE_TIER_BPS_30.to_string();

    let minimum = quote_json(json!({
        "operation": "prepare_minimum_opening_pair",
        "desiredPriceQ64_64": desired_price.to_string(),
        "feeBps": fee_bps,
    }))
    .expect("minimum executable pair must prepare");
    for field in [
        "desiredPriceQ64_64",
        "actualPriceQ64_64",
        "tokenAAmount",
        "tokenBAmount",
        "feeBps",
    ] {
        assert_decimal_string(&minimum[field]);
    }
    assert_eq!(
        minimum["quote"]["pool"]["reserveA"],
        minimum["tokenAAmount"]
    );
    assert_eq!(
        minimum["quote"]["pool"]["reserveB"],
        minimum["tokenBAmount"]
    );

    let from_a = quote_json(json!({
        "operation": "prepare_opening_from_token_a",
        "tokenAAmount": "2000",
        "desiredPriceQ64_64": desired_price.to_string(),
        "feeBps": FEE_TIER_BPS_30.to_string(),
    }))
    .expect("token-A edit must prepare");
    assert_eq!(from_a["tokenAAmount"], "2000");
    assert_eq!(from_a["tokenBAmount"], "4000");
    assert_eq!(from_a["actualPriceQ64_64"], desired_price.to_string());

    let from_b = quote_json(json!({
        "operation": "prepare_opening_from_token_b",
        "tokenBAmount": "4000",
        "desiredPriceQ64_64": desired_price.to_string(),
        "feeBps": FEE_TIER_BPS_30.to_string(),
    }))
    .expect("token-B edit must prepare");
    assert_eq!(from_b["tokenAAmount"], "2000");
    assert_eq!(from_b["tokenBAmount"], "4000");

    let above_javascript_integer_range = 1_u128 << 80;
    let paired = above_javascript_integer_range
        .checked_mul(2)
        .expect("test pair fits");
    let explicit = quote_json(json!({
        "operation": "validate_explicit_opening_pair",
        "tokenAAmount": above_javascript_integer_range.to_string(),
        "tokenBAmount": paired.to_string(),
        "desiredPriceQ64_64": desired_price.to_string(),
        "feeBps": FEE_TIER_BPS_30.to_string(),
    }))
    .expect("explicit pair must validate");
    assert_eq!(
        explicit["tokenAAmount"],
        above_javascript_integer_range.to_string()
    );
    assert_eq!(explicit["tokenBAmount"], paired.to_string());
}

#[test]
fn caller_opening_intents_map_reversed_order_without_local_price_math() {
    let ids = PairIds::new();
    let desired_price = Q64_64_ONE.checked_mul(2).expect("test price fits");
    let request = |intent: Value| {
        json!({
            "operation": "prepare_caller_opening_pair",
            "firstTokenDefinitionId": ids.first_token_id.to_string(),
            "secondTokenDefinitionId": ids.second_token_id.to_string(),
            "desiredPriceQ64_64": desired_price.to_string(),
            "feeBps": FEE_TIER_BPS_30.to_string(),
            "intent": intent,
        })
    };

    let first = quote_json(request(json!({
        "kind": "first_amount",
        "amount": "4000",
    })))
    .expect("caller first amount must prepare");
    assert_eq!(first["callerOrder"], "reversed");
    assert_eq!(first["firstAmount"], "4000");
    assert_eq!(first["secondAmount"], "2000");
    assert_eq!(first["stored"]["tokenAAmount"], "2000");
    assert_eq!(first["stored"]["tokenBAmount"], "4000");

    let second = quote_json(request(json!({
        "kind": "second_amount",
        "amount": "2000",
    })))
    .expect("caller second amount must prepare");
    assert_eq!(second["firstAmount"], "4000");
    assert_eq!(second["secondAmount"], "2000");

    let explicit = quote_json(request(json!({
        "kind": "explicit",
        "firstAmount": "4000",
        "secondAmount": "2000",
    })))
    .expect("caller explicit amounts must prepare");
    assert_eq!(
        explicit["stored"]["actualPriceQ64_64"],
        desired_price.to_string()
    );

    let minimum = quote_json(request(json!({ "kind": "minimum" })))
        .expect("caller minimum amounts must prepare");
    assert_eq!(minimum["callerOrder"], "reversed");
    assert_decimal_string(&minimum["firstAmount"]);
    assert_decimal_string(&minimum["secondAmount"]);
}

#[test]
fn opening_intent_wire_errors_keep_stable_codes_and_string_inputs() {
    let zero_price = quote_json(json!({
        "operation": "prepare_minimum_opening_pair",
        "desiredPriceQ64_64": "0",
        "feeBps": FEE_TIER_BPS_30.to_string(),
    }))
    .expect_err("zero desired price must fail");
    assert_eq!(zero_price.code(), "zero_desired_price");

    let numeric_amount = quote_json(json!({
        "operation": "prepare_opening_from_token_a",
        "tokenAAmount": 2000,
        "desiredPriceQ64_64": Q64_64_ONE.to_string(),
        "feeBps": FEE_TIER_BPS_30.to_string(),
    }))
    .expect_err("numeric chain amount must not enter the lossless wire contract");
    assert_eq!(numeric_amount.code(), "invalid_request");

    let mismatched = quote_json(json!({
        "operation": "validate_explicit_opening_pair",
        "tokenAAmount": "2000",
        "tokenBAmount": "4001",
        "desiredPriceQ64_64": (Q64_64_ONE * 2).to_string(),
        "feeBps": FEE_TIER_BPS_30.to_string(),
    }))
    .expect_err("nonmatching explicit spot price must fail");
    assert_eq!(mismatched.code(), "spot_price_mismatch");
}
