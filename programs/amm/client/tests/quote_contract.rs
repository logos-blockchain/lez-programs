use amm_client::{
    plan_add_liquidity, plan_create_pool, plan_remove_liquidity, plan_swap_exact_input,
    plan_swap_exact_output, prepare_add_liquidity, prepare_create_pool, prepare_remove_liquidity,
    prepare_swap_exact_input, prepare_swap_exact_output,
    quote::{
        self, AccountSnapshot, ValidatedFungibleDefinition, ValidatedFungibleHolding,
        ValidatedPoolSnapshot,
    },
    AddLiquidityPlanInput, AmmContext, ClientError, CreatePoolPlanInput, PoolContext,
    RemoveLiquidityPlanInput, SlippageTolerance, SwapExactInputPlanInput, SwapExactOutputPlanInput,
};
use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_pool_pda, compute_vault_pda,
    AmmConfig, Instruction, PoolDefinition, FEE_TIER_BPS_30, MINIMUM_LIQUIDITY,
};
use amm_program::quote as program_quote;
use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OBSERVATIONS_CAPACITY;

const AMM_PROGRAM_ID: ProgramId = [42; 8];
const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];
const LP_SUPPLY: u128 = 2_000;
const RESERVE_A: u128 = 1_000;
const RESERVE_B: u128 = 500;
const VAULT_A_BALANCE: u128 = 1_100;
const VAULT_B_BALANCE: u128 = 550;

fn token_a_id() -> AccountId {
    AccountId::new([1; 32])
}

fn token_b_id() -> AccountId {
    AccountId::new([2; 32])
}

fn pool_id() -> AccountId {
    compute_pool_pda(AMM_PROGRAM_ID, token_a_id(), token_b_id())
}

fn vault_a_id() -> AccountId {
    compute_vault_pda(AMM_PROGRAM_ID, pool_id(), token_a_id())
}

fn vault_b_id() -> AccountId {
    compute_vault_pda(AMM_PROGRAM_ID, pool_id(), token_b_id())
}

fn liquidity_definition_id() -> AccountId {
    compute_liquidity_token_pda(AMM_PROGRAM_ID, pool_id())
}

fn account(program_owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner,
        balance: 0,
        data,
        nonce: Nonce(0),
    }
}

fn fungible_definition(
    account_id: AccountId,
    total_supply: u128,
    authority: Option<AccountId>,
) -> AccountSnapshot {
    AccountSnapshot::new(
        account_id,
        account(
            TOKEN_PROGRAM_ID,
            Data::from(&TokenDefinition::Fungible {
                name: String::from("Token"),
                total_supply,
                metadata_id: None,
                authority,
            }),
        ),
    )
}

fn fungible_holding(
    account_id: AccountId,
    definition_id: AccountId,
    balance: u128,
) -> AccountSnapshot {
    AccountSnapshot::new(
        account_id,
        account(
            TOKEN_PROGRAM_ID,
            Data::from(&TokenHolding::Fungible {
                definition_id,
                balance,
            }),
        ),
    )
}

struct Fixture {
    context: AmmContext,
    pool: AccountSnapshot,
    token_a_definition: AccountSnapshot,
    token_b_definition: AccountSnapshot,
    vault_a: AccountSnapshot,
    vault_b: AccountSnapshot,
    liquidity_definition: AccountSnapshot,
}

impl Fixture {
    fn new() -> Self {
        let config = AmmConfig {
            token_program_id: TOKEN_PROGRAM_ID,
            twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
            authority: AccountId::new([9; 32]),
        };
        let config_account = AccountSnapshot::new(
            compute_config_pda(AMM_PROGRAM_ID),
            account(AMM_PROGRAM_ID, Data::from(&config)),
        );
        let context = AmmContext::from_config_account(AMM_PROGRAM_ID, &config_account)
            .expect("canonical config snapshot must validate");
        let pool_definition = PoolDefinition {
            definition_token_a_id: token_a_id(),
            definition_token_b_id: token_b_id(),
            vault_a_id: vault_a_id(),
            vault_b_id: vault_b_id(),
            liquidity_pool_id: liquidity_definition_id(),
            liquidity_pool_supply: LP_SUPPLY,
            reserve_a: RESERVE_A,
            reserve_b: RESERVE_B,
            fees: FEE_TIER_BPS_30,
        };

        Self {
            context,
            pool: AccountSnapshot::new(
                pool_id(),
                account(AMM_PROGRAM_ID, Data::from(&pool_definition)),
            ),
            token_a_definition: fungible_definition(token_a_id(), 100_000, None),
            token_b_definition: fungible_definition(token_b_id(), 100_000, None),
            vault_a: fungible_holding(vault_a_id(), token_a_id(), VAULT_A_BALANCE),
            vault_b: fungible_holding(vault_b_id(), token_b_id(), VAULT_B_BALANCE),
            liquidity_definition: fungible_definition(
                liquidity_definition_id(),
                LP_SUPPLY,
                Some(liquidity_definition_id()),
            ),
        }
    }

    fn validated_pool(&self) -> Result<ValidatedPoolSnapshot, ClientError> {
        ValidatedPoolSnapshot::new(
            &self.context,
            &self.pool,
            &self.token_a_definition,
            &self.token_b_definition,
            &self.vault_a,
            &self.vault_b,
            &self.liquidity_definition,
        )
    }

    fn token_a(&self) -> ValidatedFungibleDefinition {
        ValidatedFungibleDefinition::new(&self.context, &self.token_a_definition)
            .expect("token A definition must validate")
    }

    fn token_b(&self) -> ValidatedFungibleDefinition {
        ValidatedFungibleDefinition::new(&self.context, &self.token_b_definition)
            .expect("token B definition must validate")
    }

    fn liquidity_token(&self) -> ValidatedFungibleDefinition {
        ValidatedFungibleDefinition::new(&self.context, &self.liquidity_definition)
            .expect("liquidity definition must validate")
    }
}

#[test]
fn validates_context_pool_vaults_and_fungible_definitions() {
    let fixture = Fixture::new();
    let snapshot = fixture
        .validated_pool()
        .expect("canonical pool snapshot must validate");

    assert_eq!(fixture.context.amm_program_id, AMM_PROGRAM_ID);
    assert_eq!(fixture.context.token_program_id(), TOKEN_PROGRAM_ID);
    assert_eq!(snapshot.pool_id(), pool_id());
    assert_eq!(snapshot.pool().reserve_a, RESERVE_A);
    assert_eq!(snapshot.pool().reserve_b, RESERVE_B);
    assert_eq!(snapshot.vault_a().balance(), VAULT_A_BALANCE);
    assert_eq!(snapshot.vault_b().balance(), VAULT_B_BALANCE);
    assert_eq!(snapshot.token_a_definition().account_id(), token_a_id());
    assert_eq!(snapshot.token_b_definition().account_id(), token_b_id());
    assert_eq!(
        snapshot.liquidity_definition().total_supply(),
        snapshot.pool().liquidity_pool_supply
    );
}

#[test]
fn rejects_unrelated_vault_even_when_its_holding_data_matches() {
    let fixture = Fixture::new();
    let unrelated_vault =
        AccountSnapshot::new(AccountId::new([99; 32]), fixture.vault_a.account().clone());
    let result = ValidatedPoolSnapshot::new(
        &fixture.context,
        &fixture.pool,
        &fixture.token_a_definition,
        &fixture.token_b_definition,
        &unrelated_vault,
        &fixture.vault_b,
        &fixture.liquidity_definition,
    );
    let error = result.err().expect("unrelated vault must be rejected");

    assert_eq!(error.code(), "account_id_mismatch");
    assert!(matches!(
        error,
        ClientError::AccountIdMismatch {
            account: "vault A",
            expected,
            actual,
        } if expected == vault_a_id() && actual == AccountId::new([99; 32])
    ));
}

#[test]
fn rejects_inconsistent_liquidity_definition_state() {
    let fixture = Fixture::new();
    let wrong_supply = fungible_definition(
        liquidity_definition_id(),
        1_999,
        Some(liquidity_definition_id()),
    );
    let result = ValidatedPoolSnapshot::new(
        &fixture.context,
        &fixture.pool,
        &fixture.token_a_definition,
        &fixture.token_b_definition,
        &fixture.vault_a,
        &fixture.vault_b,
        &wrong_supply,
    );
    let error = result
        .err()
        .expect("LP definition supply mismatch must be rejected");

    assert_eq!(error.code(), "invalid_account_data");
}

#[test]
fn client_quotes_match_program_quotes_for_every_economic_operation() {
    let fixture = Fixture::new();
    let snapshot = fixture
        .validated_pool()
        .expect("canonical pool snapshot must validate");
    let token_a = fixture.token_a();
    let token_b = fixture.token_b();
    let liquidity_token = fixture.liquidity_token();
    let user_a = ValidatedFungibleHolding::new(
        &fixture.context,
        &fungible_holding(AccountId::new([20; 32]), token_a_id(), 10_000),
        &token_a,
    )
    .expect("user token-A holding must validate");
    let user_b = ValidatedFungibleHolding::new(
        &fixture.context,
        &fungible_holding(AccountId::new([21; 32]), token_b_id(), 10_000),
        &token_b,
    )
    .expect("user token-B holding must validate");
    let user_liquidity = ValidatedFungibleHolding::new(
        &fixture.context,
        &fungible_holding(AccountId::new([22; 32]), liquidity_definition_id(), 1_000),
        &liquidity_token,
    )
    .expect("user LP holding must validate");

    assert_eq!(
        quote::create_pool(
            &fixture.context,
            &token_a,
            &token_b,
            4_000,
            9_000,
            FEE_TIER_BPS_30
        ),
        program_quote::create_pool(4_000, 9_000, FEE_TIER_BPS_30).map_err(ClientError::from)
    );
    assert_eq!(
        quote::preview_add_liquidity(&snapshot, 400, 100),
        program_quote::preview_add_liquidity(
            snapshot.pool(),
            VAULT_A_BALANCE,
            VAULT_B_BALANCE,
            400,
            100,
        )
        .map_err(ClientError::from)
    );
    assert_eq!(
        quote::add_liquidity(&snapshot, 400, 100, 399),
        program_quote::add_liquidity(
            snapshot.pool(),
            VAULT_A_BALANCE,
            VAULT_B_BALANCE,
            400,
            100,
            399,
        )
        .map_err(ClientError::from)
    );
    assert_eq!(
        quote::preview_remove_liquidity(&snapshot, &user_liquidity, 500),
        program_quote::preview_remove_liquidity(snapshot.pool(), 1_000, 500)
            .map_err(ClientError::from)
    );
    assert_eq!(
        quote::remove_liquidity(&snapshot, &user_liquidity, 500, 250, 125),
        program_quote::remove_liquidity(snapshot.pool(), 1_000, 500, 250, 125)
            .map_err(ClientError::from)
    );
    assert_eq!(
        quote::preview_swap_exact_input(&snapshot, &user_a, &user_b, 100),
        program_quote::preview_swap_exact_input(
            snapshot.pool(),
            VAULT_A_BALANCE,
            VAULT_B_BALANCE,
            program_quote::SwapDirection::AToB,
            100,
        )
        .map_err(ClientError::from)
    );
    assert_eq!(
        quote::swap_exact_input(&snapshot, &user_b, &user_a, 100, 165),
        program_quote::swap_exact_input(
            snapshot.pool(),
            VAULT_A_BALANCE,
            VAULT_B_BALANCE,
            program_quote::SwapDirection::BToA,
            100,
            165,
        )
        .map_err(ClientError::from)
    );
    assert_eq!(
        quote::preview_swap_exact_output(&snapshot, &user_a, &user_b, 45),
        program_quote::preview_swap_exact_output(
            snapshot.pool(),
            VAULT_A_BALANCE,
            VAULT_B_BALANCE,
            program_quote::SwapDirection::AToB,
            45,
        )
        .map_err(ClientError::from)
    );
    assert_eq!(
        quote::swap_exact_output(&snapshot, &user_a, &user_b, 45, 100),
        program_quote::swap_exact_output(
            snapshot.pool(),
            VAULT_A_BALANCE,
            VAULT_B_BALANCE,
            program_quote::SwapDirection::AToB,
            45,
            100,
        )
        .map_err(ClientError::from)
    );
    assert_eq!(
        quote::sync_reserves(&snapshot),
        program_quote::sync_reserves(snapshot.pool(), VAULT_A_BALANCE, VAULT_B_BALANCE)
            .map_err(ClientError::from)
    );
    let window_duration = u64::from(OBSERVATIONS_CAPACITY);
    assert_eq!(
        quote::create_oracle_price_account(&snapshot, window_duration),
        program_quote::create_oracle_price_account(snapshot.pool(), window_duration)
            .map_err(ClientError::from)
    );
    assert_eq!(
        quote::pair_order(&snapshot, &token_b, &token_a),
        Ok(program_quote::PairOrder::Reversed)
    );
}

#[test]
fn swap_rejects_unrelated_output_and_insufficient_input_balance() {
    let fixture = Fixture::new();
    let snapshot = fixture
        .validated_pool()
        .expect("canonical pool snapshot must validate");
    let token_a = fixture.token_a();
    let token_b = fixture.token_b();
    let token_c_account = fungible_definition(AccountId::new([3; 32]), 100_000, None);
    let token_c = ValidatedFungibleDefinition::new(&fixture.context, &token_c_account)
        .expect("third fungible definition must validate");
    let user_a = ValidatedFungibleHolding::new(
        &fixture.context,
        &fungible_holding(AccountId::new([20; 32]), token_a_id(), 99),
        &token_a,
    )
    .expect("user token-A holding must validate");
    let user_b = ValidatedFungibleHolding::new(
        &fixture.context,
        &fungible_holding(AccountId::new([21; 32]), token_b_id(), 0),
        &token_b,
    )
    .expect("user token-B holding must validate");
    let user_c = ValidatedFungibleHolding::new(
        &fixture.context,
        &fungible_holding(AccountId::new([23; 32]), token_c.account_id(), 0),
        &token_c,
    )
    .expect("user token-C holding must validate");

    let unrelated_output = quote::swap_exact_input(&snapshot, &user_a, &user_c, 99, 1)
        .expect_err("unrelated output holding must be rejected");
    assert_eq!(unrelated_output.code(), "token_definition_mismatch");

    let insufficient = quote::swap_exact_input(&snapshot, &user_a, &user_b, 100, 1)
        .expect_err("input above the holding balance must be rejected");
    assert_eq!(insufficient.code(), "insufficient_balance");
    assert!(matches!(
        insufficient,
        ClientError::InsufficientBalance {
            account: "user input holding",
            available: 99,
            required: 100,
        }
    ));
}

#[test]
fn raw_amounts_above_javascript_integer_range_remain_exact() {
    const ABOVE_TWO_POW_53: u128 = 9_007_199_254_740_993;
    const USER_LIQUIDITY: u128 = 9_007_199_254_739_993;

    let fixture = Fixture::new();
    let token_a = fixture.token_a();
    let token_b = fixture.token_b();
    let quote = quote::create_pool(
        &fixture.context,
        &token_a,
        &token_b,
        ABOVE_TWO_POW_53,
        ABOVE_TWO_POW_53,
        FEE_TIER_BPS_30,
    )
    .expect("large exact integer amounts must quote");
    let holding = ValidatedFungibleHolding::new(
        &fixture.context,
        &fungible_holding(AccountId::new([20; 32]), token_a_id(), ABOVE_TWO_POW_53),
        &token_a,
    )
    .expect("large exact integer holding must validate");

    assert_eq!(quote.pool.reserve_a, ABOVE_TWO_POW_53);
    assert_eq!(quote.pool.reserve_b, ABOVE_TWO_POW_53);
    assert_eq!(quote.pool.liquidity_pool_supply, ABOVE_TWO_POW_53);
    assert_eq!(quote.locked_liquidity, MINIMUM_LIQUIDITY);
    assert_eq!(quote.user_liquidity, USER_LIQUIDITY);
    assert_eq!(holding.balance(), ABOVE_TWO_POW_53);
}

#[test]
fn prepared_instruction_args_feed_canonical_planners_without_ui_math() {
    let fixture = Fixture::new();
    let snapshot = fixture
        .validated_pool()
        .expect("canonical pool snapshot must validate");
    let pool = PoolContext::new(&fixture.context, snapshot.pool_id(), snapshot.pool())
        .expect("validated pool has canonical identity");
    let token_a = fixture.token_a();
    let token_b = fixture.token_b();
    let liquidity_token = fixture.liquidity_token();
    let user_holding_a_id = AccountId::new([20; 32]);
    let user_holding_b_id = AccountId::new([21; 32]);
    let user_holding_lp_id = AccountId::new([22; 32]);
    let user_a = ValidatedFungibleHolding::new(
        &fixture.context,
        &fungible_holding(user_holding_a_id, token_a_id(), 10_000),
        &token_a,
    )
    .expect("user token-A holding must validate");
    let user_b = ValidatedFungibleHolding::new(
        &fixture.context,
        &fungible_holding(user_holding_b_id, token_b_id(), 10_000),
        &token_b,
    )
    .expect("user token-B holding must validate");
    let user_liquidity = ValidatedFungibleHolding::new(
        &fixture.context,
        &fungible_holding(user_holding_lp_id, liquidity_definition_id(), 1_000),
        &liquidity_token,
    )
    .expect("user LP holding must validate");
    let tolerance = SlippageTolerance::new(100).expect("one percent is valid");
    let deadline = u64::MAX;

    let prepared_create = prepare_create_pool(
        &fixture.context,
        &token_a,
        &token_b,
        4_000,
        9_000,
        FEE_TIER_BPS_30,
    )
    .expect("pool creation must prepare");
    let create_plan = plan_create_pool(CreatePoolPlanInput {
        context: &fixture.context,
        token_a_definition_id: token_a.account_id(),
        token_b_definition_id: token_b.account_id(),
        user_holding_a: user_holding_a_id,
        user_holding_b: user_holding_b_id,
        user_holding_lp: user_holding_lp_id,
        token_a_amount: prepared_create.token_a_amount,
        token_b_amount: prepared_create.token_b_amount,
        fees: prepared_create.fees,
        deadline,
    })
    .expect("prepared create args must plan");
    assert!(matches!(
        create_plan.instruction(),
        Instruction::NewDefinition {
            token_a_amount,
            token_b_amount,
            fees,
            deadline: planned_deadline,
        } if *token_a_amount == prepared_create.token_a_amount
            && *token_b_amount == prepared_create.token_b_amount
            && *fees == prepared_create.fees
            && *planned_deadline == deadline
    ));

    let prepared_add =
        prepare_add_liquidity(&snapshot, 400, 100, tolerance).expect("add liquidity must prepare");
    assert_eq!(prepared_add.max_amount_to_add_token_a, 200);
    assert_eq!(prepared_add.max_amount_to_add_token_b, 100);
    assert_eq!(
        prepared_add.max_amount_to_add_token_a,
        prepared_add.quote.actual_amount_a
    );
    assert_eq!(
        prepared_add.max_amount_to_add_token_b,
        prepared_add.quote.actual_amount_b
    );
    let add_plan = plan_add_liquidity(AddLiquidityPlanInput {
        context: &fixture.context,
        pool,
        user_holding_a: user_holding_a_id,
        user_holding_b: user_holding_b_id,
        user_holding_lp: user_holding_lp_id,
        min_amount_liquidity: prepared_add.min_amount_liquidity,
        max_amount_to_add_token_a: prepared_add.max_amount_to_add_token_a,
        max_amount_to_add_token_b: prepared_add.max_amount_to_add_token_b,
        deadline,
    });
    assert!(matches!(
        add_plan.instruction(),
        Instruction::AddLiquidity {
            min_amount_liquidity,
            max_amount_to_add_token_a,
            max_amount_to_add_token_b,
            deadline: planned_deadline,
        } if *min_amount_liquidity == prepared_add.min_amount_liquidity
            && *max_amount_to_add_token_a == prepared_add.max_amount_to_add_token_a
            && *max_amount_to_add_token_b == prepared_add.max_amount_to_add_token_b
            && *planned_deadline == deadline
    ));

    let prepared_remove = prepare_remove_liquidity(&snapshot, &user_liquidity, 500, tolerance)
        .expect("remove liquidity must prepare");
    let remove_plan = plan_remove_liquidity(RemoveLiquidityPlanInput {
        context: &fixture.context,
        pool,
        user_holding_a: user_holding_a_id,
        user_holding_b: user_holding_b_id,
        user_holding_lp: user_holding_lp_id,
        remove_liquidity_amount: prepared_remove.remove_liquidity_amount,
        min_amount_to_remove_token_a: prepared_remove.min_amount_to_remove_token_a,
        min_amount_to_remove_token_b: prepared_remove.min_amount_to_remove_token_b,
        deadline,
    });
    assert!(matches!(
        remove_plan.instruction(),
        Instruction::RemoveLiquidity {
            remove_liquidity_amount,
            min_amount_to_remove_token_a,
            min_amount_to_remove_token_b,
            deadline: planned_deadline,
        } if *remove_liquidity_amount == prepared_remove.remove_liquidity_amount
            && *min_amount_to_remove_token_a == prepared_remove.min_amount_to_remove_token_a
            && *min_amount_to_remove_token_b == prepared_remove.min_amount_to_remove_token_b
            && *planned_deadline == deadline
    ));

    let prepared_exact_input =
        prepare_swap_exact_input(&snapshot, &user_a, &user_b, 100, tolerance)
            .expect("exact-input swap must prepare");
    let exact_input_plan = plan_swap_exact_input(SwapExactInputPlanInput {
        context: &fixture.context,
        pool,
        user_input_holding: user_holding_a_id,
        user_output_holding: user_holding_b_id,
        swap_amount_in: prepared_exact_input.swap_amount_in,
        min_amount_out: prepared_exact_input.min_amount_out,
        deadline,
    });
    assert!(matches!(
        exact_input_plan.instruction(),
        Instruction::SwapExactInput {
            swap_amount_in,
            min_amount_out,
            deadline: planned_deadline,
        } if *swap_amount_in == prepared_exact_input.swap_amount_in
            && *min_amount_out == prepared_exact_input.min_amount_out
            && *planned_deadline == deadline
    ));

    let prepared_exact_output =
        prepare_swap_exact_output(&snapshot, &user_a, &user_b, 45, tolerance)
            .expect("exact-output swap must prepare");
    let exact_output_plan = plan_swap_exact_output(SwapExactOutputPlanInput {
        context: &fixture.context,
        pool,
        user_input_holding: user_holding_a_id,
        user_output_holding: user_holding_b_id,
        exact_amount_out: prepared_exact_output.exact_amount_out,
        max_amount_in: prepared_exact_output.max_amount_in,
        deadline,
    });
    assert!(matches!(
        exact_output_plan.instruction(),
        Instruction::SwapExactOutput {
            exact_amount_out,
            max_amount_in,
            deadline: planned_deadline,
        } if *exact_amount_out == prepared_exact_output.exact_amount_out
            && *max_amount_in == prepared_exact_output.max_amount_in
            && *planned_deadline == deadline
    ));
}
