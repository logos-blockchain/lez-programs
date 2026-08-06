use alloy_primitives::U256;
use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_vault_pda, isqrt_product, spot_price_q64_64, AmmConfig, PoolDefinition,
    MINIMUM_LIQUIDITY,
};
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::{compute_current_tick_account_pda, CurrentTickAccount};

use super::{
    accounts::{active_account_plan, missing_account_plan},
    context::{context, token_ids},
    holding::{select_holding, wallet_holdings, SelectedHolding},
    pair::{is_canonical_pair, pair_ids, PairIds},
    plan::plan,
    position::AccountPlanHoldings,
    quote::{div_ceil_u256, minimum_opening_pair, quote, Q64},
    swap::swap_exact_in_plan,
    ContextRequest, PairIdsRequest, PairSnapshot, PlanRequest, PositionRequest, QuoteRequest,
    SwapExactInPlanRequest, TokenIdsRequest,
};
use crate::{
    account::{account_id_hex, account_read, decode_account, parse_base58_id, program_id_bytes},
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

fn clock_account() -> Account {
    account(
        [44; 8],
        Data::try_from(
            ClockAccountData {
                block_id: 10,
                timestamp: 1_000,
            }
            .to_bytes(),
        )
        .unwrap(),
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
        token_program: TOKEN_PROGRAM,
        twap_program: TWAP_PROGRAM,
    }
}

fn base_snapshot(pair: PairIds) -> PairSnapshot {
    let holding_a_id = AccountId::new([61; 32]);
    let holding_b_id = AccountId::new([62; 32]);
    PairSnapshot {
        config: account_read(pair.config, &config_account()),
        token_a: account_read(pair.token_a, &token_definition("A", 1_000_000)),
        token_b: account_read(pair.token_b, &token_definition("B", 2_000_000)),
        pool: default_read(pair.pool),
        vault_a: default_read(pair.vault_a),
        vault_b: default_read(pair.vault_b),
        lp_definition: default_read(pair.lp_definition),
        lp_lock_holding: default_read(pair.lp_lock_holding),
        current_tick: default_read(pair.current_tick),
        clock: account_read(pair.clock, &clock_account()),
        wallet_available: true,
        wallet_accounts: vec![
            account_read(holding_a_id, &token_holding(pair.token_a, 1_000_000)),
            account_read(holding_b_id, &token_holding(pair.token_b, 1_000_000)),
        ],
    }
}

fn request(pair: PairIds) -> PositionRequest {
    assert!(is_canonical_pair(pair.token_a, pair.token_b));
    PositionRequest {
        token_a_id: pair.token_a.to_string(),
        token_b_id: pair.token_b.to_string(),
        fee_bps: 30,
        amount_a_raw: None,
        amount_b_raw: None,
        max_amount_a_raw: None,
        max_amount_b_raw: None,
        slippage_bps: None,
        initial_price_real_raw: Some(Q64.to_string()),
    }
}

fn amm_program_id() -> String {
    hex::encode(program_id_bytes(AMM_PROGRAM))
}

struct Scenario {
    pair: PairIds,
    request: PositionRequest,
    snapshot: PairSnapshot,
    network_id: &'static str,
    network_fingerprint: &'static str,
}

impl Scenario {
    fn devnet() -> Self {
        Self::new("devnet", "channel:test")
    }

    fn testnet() -> Self {
        Self::new("testnet", "block10:test")
    }

    fn new(network_id: &'static str, network_fingerprint: &'static str) -> Self {
        let pair = ids();
        Self {
            pair,
            request: request(pair),
            snapshot: base_snapshot(pair),
            network_id,
            network_fingerprint,
        }
    }

    fn quote_request(&self) -> QuoteRequest {
        QuoteRequest {
            network_id: String::from(self.network_id),
            network_fingerprint: String::from(self.network_fingerprint),
            amm_program_id: amm_program_id(),
            request: self.request.clone(),
            snapshot: self.snapshot.clone(),
        }
    }

    fn quote(&self) -> Value {
        quote(self.quote_request()).unwrap()
    }

    fn plan(self, quote_hash: impl Into<String>, fresh_lp: Option<AccountRead>) -> Value {
        plan(PlanRequest {
            network_id: String::from(self.network_id),
            network_fingerprint: String::from(self.network_fingerprint),
            amm_program_id: amm_program_id(),
            request: self.request,
            snapshot: self.snapshot,
            quote_hash: quote_hash.into(),
            now_ms: 2_000,
            fresh_lp,
        })
        .unwrap()
    }
}

fn assert_preview_matches_plan(
    quote_value: &Value,
    plan_value: &Value,
    fresh_lp: Option<AccountId>,
) {
    let preview = quote_value["accountPreview"].as_array().unwrap();
    let account_ids = plan_value["accountIds"].as_array().unwrap();
    let signing_requirements = plan_value["signingRequirements"].as_array().unwrap();
    assert_eq!(preview.len(), account_ids.len());
    assert_eq!(preview.len(), signing_requirements.len());

    for (order, row) in preview.iter().enumerate() {
        assert_eq!(row["order"], order);
        assert_eq!(row["signer"], signing_requirements[order]);
        if let Some(account_id) = row["accountId"].as_str() {
            let account_id = parse_base58_id(account_id, "preview account id").unwrap();
            assert_eq!(account_ids[order], account_id_hex(account_id));
        } else {
            assert_eq!(row["role"], "user_holding_lp");
            assert_eq!(account_ids[order], account_id_hex(fresh_lp.unwrap()));
        }
    }
}

#[test]
fn account_plan_sources_follow_pool_branch() {
    let scenario = Scenario::devnet();
    let pair = scenario.pair;
    let input = scenario.quote_request();
    let holdings = wallet_holdings(&input.snapshot.wallet_accounts, pair.token_program);
    let holding_a = select_holding(&holdings, pair.token_a);
    let holding_b = select_holding(&holdings, pair.token_b);

    let missing = missing_account_plan(
        &input,
        pair,
        AMM_PROGRAM,
        AccountPlanHoldings {
            token_a: holding_a.as_ref(),
            token_b: holding_b.as_ref(),
            lp: None,
        },
    )
    .unwrap();
    assert_eq!(
        missing
            .sources
            .iter()
            .map(|source| source.role.as_str())
            .collect::<Vec<_>>(),
        vec![
            "config",
            "token_a",
            "token_b",
            "pool",
            "vault_a",
            "vault_b",
            "lp_definition",
            "lp_lock_holding",
            "current_tick",
            "holding_a",
            "holding_b",
        ]
    );

    let active = active_account_plan(
        &input,
        pair,
        AMM_PROGRAM,
        &PoolDefinition::default(),
        false,
        AccountPlanHoldings {
            token_a: holding_a.as_ref(),
            token_b: holding_b.as_ref(),
            lp: None,
        },
    )
    .unwrap();
    assert_eq!(
        active
            .sources
            .iter()
            .map(|source| source.role.as_str())
            .collect::<Vec<_>>(),
        vec![
            "config",
            "token_a",
            "token_b",
            "pool",
            "vault_a",
            "vault_b",
            "lp_definition",
            "current_tick",
            "holding_a",
            "holding_b",
        ]
    );
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
        account: account(
            TOKEN_PROGRAM,
            Data::from(&TokenHolding::Fungible {
                definition_id: definition,
                balance,
            }),
        ),
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
fn token_manifest_includes_compatible_wallet_holdings() {
    let config_id = compute_config_pda(AMM_PROGRAM);
    let configured = AccountId::new([1; 32]);
    let held = AccountId::new([2; 32]);
    let recent = AccountId::new([3; 32]);
    let resolved = AccountId::new([4; 32]);

    let value = token_ids(TokenIdsRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, &config_account()),
        wallet_accounts: vec![account_read(
            AccountId::new([5; 32]),
            &token_holding(held, 9),
        )],
        configured_token_ids: vec![account_id_hex(configured)],
        recent_token_ids: vec![recent.to_string()],
        resolved_token_ids: vec![resolved.to_string()],
    })
    .unwrap();

    assert_eq!(
        value["tokenIds"],
        json!([
            account_id_hex(configured),
            account_id_hex(held),
            account_id_hex(recent),
            account_id_hex(resolved),
        ])
    );
}

#[test]
fn wrong_program_holdings_do_not_contribute_token_candidates() {
    let config_id = compute_config_pda(AMM_PROGRAM);
    let config = config_account();
    let definition = AccountId::new([2; 32]);
    let wrong_owner_holding = account(
        [99; 8],
        Data::from(&TokenHolding::Fungible {
            definition_id: definition,
            balance: 9,
        }),
    );
    let wallet_accounts = vec![account_read(AccountId::new([3; 32]), &wrong_owner_holding)];

    let manifest = token_ids(TokenIdsRequest {
        amm_program_id: amm_program_id(),
        config: account_read(config_id, &config),
        wallet_accounts: wallet_accounts.clone(),
        configured_token_ids: Vec::new(),
        recent_token_ids: Vec::new(),
        resolved_token_ids: Vec::new(),
    })
    .unwrap();
    assert_eq!(manifest["tokenIds"], json!([]));

    let value = context(ContextRequest {
        network_id: String::from("testnet"),
        network_fingerprint: String::from("block10:abc"),
        amm_program_id: amm_program_id(),
        wallet_available: true,
        config: account_read(config_id, &config),
        wallet_accounts,
        token_definitions: vec![account_read(
            definition,
            &token_definition("Token", 1_000_000),
        )],
        configured_token_ids: Vec::new(),
        recent_token_ids: Vec::new(),
        resolved_token_ids: Vec::new(),
    })
    .unwrap();
    assert_eq!(value["tokens"], json!([]));
}

#[test]
fn context_selects_tokens_without_holdings() {
    let token_id = AccountId::new([3; 32]);
    let config_id = compute_config_pda(AMM_PROGRAM);
    let value = context(ContextRequest {
        network_id: String::from("testnet"),
        network_fingerprint: String::from("block10:abc"),
        amm_program_id: amm_program_id(),
        wallet_available: true,
        config: account_read(config_id, &config_account()),
        wallet_accounts: Vec::new(),
        token_definitions: vec![account_read(
            token_id,
            &token_definition("Token", 1_000_000),
        )],
        configured_token_ids: vec![account_id_hex(token_id)],
        recent_token_ids: Vec::new(),
        resolved_token_ids: Vec::new(),
    })
    .unwrap();
    assert_eq!(value["tokens"][0]["selectable"], true);
    assert_eq!(value["tokens"][0]["sources"], json!(["config"]));
    assert!(value["tokens"][0].get("holdingId").is_none());
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
fn missing_pool_quote_and_plan_use_current_account_order() {
    let scenario = Scenario::devnet();
    let quote_value = scenario.quote();
    assert_eq!(quote_value["status"], "ok");
    assert_eq!(quote_value["poolStatus"], "missing_pool");
    assert_eq!(quote_value["canSubmit"], true);
    assert_eq!(quote_value["accountPreview"].as_array().unwrap().len(), 11);
    let quote_hash = quote_value["quoteHash"].as_str().unwrap().to_owned();

    let fresh_lp = AccountId::new([63; 32]);
    let plan_value = scenario.plan(quote_hash, Some(default_read(fresh_lp)));
    assert_eq!(plan_value["status"], "ready");
    assert_eq!(plan_value["accountIds"].as_array().unwrap().len(), 11);
    assert_eq!(plan_value["accountIds"][8], account_id_hex(fresh_lp));
    assert_eq!(plan_value["signingRequirements"][6], true);
    assert_eq!(plan_value["signingRequirements"][7], true);
    assert_eq!(plan_value["signingRequirements"][8], true);
    assert_preview_matches_plan(&quote_value, &plan_value, Some(fresh_lp));
}

#[test]
fn missing_pool_plan_rejects_fresh_lp_account_collision() {
    let scenario = Scenario::devnet();
    let pool = scenario.pair.pool;
    let quote_value = scenario.quote();
    let quote_hash = quote_value["quoteHash"].as_str().unwrap().to_owned();

    let plan_value = scenario.plan(quote_hash, Some(default_read(pool)));

    assert_eq!(plan_value["status"], "error");
    assert_eq!(plan_value["code"], "wallet_submission_failed");
}

#[test]
fn missing_pool_quote_accepts_large_direct_raw_amounts() {
    let mut scenario = Scenario::devnet();
    let amount_a = 100_000_000;
    let amount_b = 150_000_000;
    scenario.request.amount_a_raw = Some(amount_a.to_string());
    scenario.request.amount_b_raw = Some(amount_b.to_string());
    scenario.request.initial_price_real_raw =
        Some(spot_price_q64_64(amount_a, amount_b).to_string());

    let quote_value = scenario.quote();

    assert_eq!(quote_value["status"], "ok");
    assert_eq!(quote_value["actualAmountARaw"], amount_a.to_string());
    assert_eq!(quote_value["actualAmountBRaw"], amount_b.to_string());
    assert!(quote_value.get("depositScaleBps").is_none());
}

#[test]
fn advancing_clock_does_not_stale_quote() {
    let mut scenario = Scenario::testnet();
    let quote_value = scenario.quote();

    scenario.snapshot.clock = account_read(
        scenario.pair.clock,
        &account(
            [44; 8],
            Data::try_from(
                ClockAccountData {
                    block_id: 11,
                    timestamp: 1_500,
                }
                .to_bytes(),
            )
            .unwrap(),
        ),
    );
    let plan_value = scenario.plan(
        quote_value["quoteHash"].as_str().unwrap(),
        Some(default_read(AccountId::new([63; 32]))),
    );

    assert_eq!(plan_value["status"], "ready");
}

#[test]
fn active_pool_quote_uses_ratio_and_existing_lp_holding() {
    let mut scenario = Scenario::testnet();
    let pair = scenario.pair;
    let pool = PoolDefinition {
        definition_token_a_id: pair.token_a,
        definition_token_b_id: pair.token_b,
        vault_a_id: pair.vault_a,
        vault_b_id: pair.vault_b,
        liquidity_pool_id: pair.lp_definition,
        liquidity_pool_supply: 10_000,
        reserve_a: 10_000,
        reserve_b: 20_000,
        fees: 30,
    };
    scenario.snapshot.pool = account_read(pair.pool, &account(AMM_PROGRAM, Data::from(&pool)));
    scenario.snapshot.vault_a =
        account_read(pair.vault_a, &token_holding(pair.token_a, pool.reserve_a));
    scenario.snapshot.vault_b =
        account_read(pair.vault_b, &token_holding(pair.token_b, pool.reserve_b));
    scenario.snapshot.lp_definition = account_read(
        pair.lp_definition,
        &account(
            TOKEN_PROGRAM,
            Data::from(&TokenDefinition::Fungible {
                name: String::from("LP"),
                total_supply: pool.liquidity_pool_supply,
                metadata_id: None,
                authority: Some(pair.lp_definition),
            }),
        ),
    );
    scenario.snapshot.current_tick = account_read(
        pair.current_tick,
        &account(
            TWAP_PROGRAM,
            Data::from(&CurrentTickAccount {
                tick: 0,
                last_updated: 1_000,
            }),
        ),
    );
    scenario.snapshot.wallet_accounts = vec![
        account_read(
            AccountId::new([61; 32]),
            &token_holding(pair.token_a, 1_000),
        ),
        account_read(
            AccountId::new([62; 32]),
            &token_holding(pair.token_b, 2_000),
        ),
    ];
    let lp_holding = AccountId::new([64; 32]);
    scenario.snapshot.wallet_accounts.push(account_read(
        lp_holding,
        &token_holding(pair.lp_definition, 500),
    ));
    scenario.request.initial_price_real_raw = None;
    scenario.request.max_amount_a_raw = Some(String::from("1000"));
    scenario.request.max_amount_b_raw = Some(String::from("3000"));
    scenario.request.slippage_bps = Some(50);

    let quote_value = scenario.quote();
    assert_eq!(quote_value["poolStatus"], "active_pool");
    assert_eq!(quote_value["actualAmountARaw"], "1000");
    assert_eq!(quote_value["actualAmountBRaw"], "2000");
    assert_eq!(quote_value["expectedLpRaw"], "1000");
    assert_eq!(quote_value["minimumLpRaw"], "995");
    assert_eq!(quote_value["requiresFreshLp"], false);
    assert_eq!(quote_value["canSubmit"], true);
    assert_eq!(quote_value["errors"], json!([]));

    let plan_value = scenario.plan(quote_value["quoteHash"].as_str().unwrap(), None);
    assert_eq!(plan_value["status"], "ready");
    assert_eq!(plan_value["accountIds"].as_array().unwrap().len(), 10);
    assert_eq!(plan_value["accountIds"][7], account_id_hex(lp_holding));
    assert_eq!(plan_value["signingRequirements"][7], false);
    assert_preview_matches_plan(&quote_value, &plan_value, None);
}

#[test]
fn matching_unfunded_quote_has_no_transaction_plan() {
    let mut scenario = Scenario::devnet();
    scenario.snapshot.wallet_available = false;
    scenario.snapshot.wallet_accounts.clear();
    let quote_value = scenario.quote();
    assert_eq!(quote_value["canSubmit"], false);

    let plan_value = scenario.plan(quote_value["quoteHash"].as_str().unwrap(), None);

    assert_eq!(plan_value["status"], "error");
    assert_eq!(plan_value["code"], "quote_not_submittable");
    assert_eq!(plan_value["quote"], quote_value);
}

#[test]
fn stale_hash_returns_recomputed_quote_without_plan() {
    let value = Scenario::devnet().plan("sha256:deadbeef", None);
    assert_eq!(value["status"], "error");
    assert_eq!(value["code"], "quote_changed");
    assert_eq!(value["quote"]["status"], "ok");
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
