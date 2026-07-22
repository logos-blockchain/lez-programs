use amm_client::{
    transaction::{
        ensure_quote_unchanged, prepare_add_liquidity_transaction, prepare_create_pool_transaction,
        prepare_remove_liquidity_transaction, prepare_swap_exact_input_transaction,
        prepare_swap_exact_output_transaction, AddLiquidityTransactionInput,
        CreatePoolTransactionInput, PoolAccountSnapshots, RemoveLiquidityTransactionInput,
        SwapExactInputTransactionInput, SwapExactOutputTransactionInput, TransactionError,
    },
    PairReadSnapshots, SlippageTolerance,
};
use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_vault_pda, AmmConfig, Instruction, PoolDefinition, FEE_TIER_BPS_30,
};
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::{compute_current_tick_account_pda, CurrentTickAccount};

const AMM_PROGRAM_ID: ProgramId = [42; 8];
const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];
const DEADLINE: u64 = 1_900_000_000_000;

fn lower_token_id() -> AccountId {
    AccountId::new([1; 32])
}

fn higher_token_id() -> AccountId {
    AccountId::new([2; 32])
}

fn account(program_owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner,
        balance: 0,
        data,
        nonce: Nonce(0),
    }
}

fn definition(id: AccountId, total_supply: u128, authority: Option<AccountId>) -> AccountSnapshot {
    AccountSnapshot::new(
        id,
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

fn holding(id: AccountId, definition_id: AccountId, balance: u128) -> AccountSnapshot {
    AccountSnapshot::new(
        id,
        account(
            TOKEN_PROGRAM_ID,
            Data::from(&TokenHolding::Fungible {
                definition_id,
                balance,
            }),
        ),
    )
}

fn clock_snapshot() -> AccountSnapshot {
    let data = ClockAccountData {
        block_id: 123,
        timestamp: 456,
    }
    .to_bytes();
    AccountSnapshot::new(
        CLOCK_01_PROGRAM_ACCOUNT_ID,
        account([88; 8], Data::try_from(data).expect("clock data must fit")),
    )
}

use amm_client::quote::AccountSnapshot;

struct Fixture {
    config: AccountSnapshot,
    pool: AccountSnapshot,
    stored_a_definition: AccountSnapshot,
    stored_b_definition: AccountSnapshot,
    vault_a: AccountSnapshot,
    vault_b: AccountSnapshot,
    liquidity_definition: AccountSnapshot,
    lp_lock_holding: AccountSnapshot,
    current_tick: AccountSnapshot,
    clock: AccountSnapshot,
    caller_first_holding: AccountSnapshot,
    caller_second_holding: AccountSnapshot,
    liquidity_holding: AccountSnapshot,
}

impl Fixture {
    fn new() -> Self {
        // Pool storage is canonical descending ID order. Callers below deliberately use lower,
        // higher order to prove the facade performs the mapping once.
        let stored_a = higher_token_id();
        let stored_b = lower_token_id();
        let pool_id = compute_pool_pda(AMM_PROGRAM_ID, stored_a, stored_b);
        let vault_a_id = compute_vault_pda(AMM_PROGRAM_ID, pool_id, stored_a);
        let vault_b_id = compute_vault_pda(AMM_PROGRAM_ID, pool_id, stored_b);
        let liquidity_id = compute_liquidity_token_pda(AMM_PROGRAM_ID, pool_id);
        let lp_lock_id = compute_lp_lock_holding_pda(AMM_PROGRAM_ID, pool_id);
        let current_tick_id = compute_current_tick_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id);
        let config = AmmConfig {
            token_program_id: TOKEN_PROGRAM_ID,
            twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
            authority: AccountId::new([9; 32]),
        };
        let pool = PoolDefinition {
            definition_token_a_id: stored_a,
            definition_token_b_id: stored_b,
            vault_a_id,
            vault_b_id,
            liquidity_pool_id: liquidity_id,
            liquidity_pool_supply: 2_000,
            reserve_a: 1_000,
            reserve_b: 500,
            fees: FEE_TIER_BPS_30,
        };

        Self {
            config: AccountSnapshot::new(
                compute_config_pda(AMM_PROGRAM_ID),
                account(AMM_PROGRAM_ID, Data::from(&config)),
            ),
            pool: AccountSnapshot::new(pool_id, account(AMM_PROGRAM_ID, Data::from(&pool))),
            stored_a_definition: definition(stored_a, 100_000, None),
            stored_b_definition: definition(stored_b, 100_000, None),
            vault_a: holding(vault_a_id, stored_a, 1_100),
            vault_b: holding(vault_b_id, stored_b, 550),
            liquidity_definition: definition(liquidity_id, 2_000, Some(liquidity_id)),
            lp_lock_holding: holding(lp_lock_id, liquidity_id, 1_000),
            current_tick: AccountSnapshot::new(
                current_tick_id,
                account(
                    TWAP_ORACLE_PROGRAM_ID,
                    Data::from(&CurrentTickAccount {
                        tick: -1,
                        last_updated: 400,
                    }),
                ),
            ),
            clock: clock_snapshot(),
            caller_first_holding: holding(AccountId::new([20; 32]), lower_token_id(), 10_000),
            caller_second_holding: holding(AccountId::new([21; 32]), higher_token_id(), 10_000),
            liquidity_holding: holding(AccountId::new([22; 32]), liquidity_id, 1_000),
        }
    }

    fn pool_accounts(&self) -> PoolAccountSnapshots<'_> {
        self.pool_accounts_with(&self.config, &self.current_tick, &self.clock)
    }

    fn pool_accounts_with<'a>(
        &'a self,
        config: &'a AccountSnapshot,
        current_tick: &'a AccountSnapshot,
        clock: &'a AccountSnapshot,
    ) -> PoolAccountSnapshots<'a> {
        PoolAccountSnapshots {
            config,
            pair: PairReadSnapshots {
                pool: &self.pool,
                first_token_definition: &self.stored_b_definition,
                second_token_definition: &self.stored_a_definition,
                first_token_vault: &self.vault_b,
                second_token_vault: &self.vault_a,
                liquidity_definition: &self.liquidity_definition,
                lp_lock_holding: &self.lp_lock_holding,
                current_tick,
                clock,
            },
        }
    }

    fn stored_order_pool_accounts(&self) -> PoolAccountSnapshots<'_> {
        PoolAccountSnapshots {
            config: &self.config,
            pair: PairReadSnapshots {
                pool: &self.pool,
                first_token_definition: &self.stored_a_definition,
                second_token_definition: &self.stored_b_definition,
                first_token_vault: &self.vault_a,
                second_token_vault: &self.vault_b,
                liquidity_definition: &self.liquidity_definition,
                lp_lock_holding: &self.lp_lock_holding,
                current_tick: &self.current_tick,
                clock: &self.clock,
            },
        }
    }

    fn slippage() -> SlippageTolerance {
        SlippageTolerance::new(100).expect("one-percent slippage must validate")
    }
}

struct MissingPairFixture {
    pool: AccountSnapshot,
    first_vault: AccountSnapshot,
    second_vault: AccountSnapshot,
    liquidity_definition: AccountSnapshot,
    lp_lock_holding: AccountSnapshot,
    current_tick: AccountSnapshot,
    clock: AccountSnapshot,
}

impl MissingPairFixture {
    fn new(fixture: &Fixture) -> Self {
        Self {
            pool: AccountSnapshot::new(fixture.pool.account_id(), Account::default()),
            first_vault: AccountSnapshot::new(fixture.vault_b.account_id(), Account::default()),
            second_vault: AccountSnapshot::new(fixture.vault_a.account_id(), Account::default()),
            liquidity_definition: AccountSnapshot::new(
                fixture.liquidity_definition.account_id(),
                Account::default(),
            ),
            lp_lock_holding: AccountSnapshot::new(
                fixture.lp_lock_holding.account_id(),
                Account::default(),
            ),
            current_tick: AccountSnapshot::new(
                fixture.current_tick.account_id(),
                Account::default(),
            ),
            clock: clock_snapshot(),
        }
    }

    fn pair<'a>(&'a self, fixture: &'a Fixture) -> PairReadSnapshots<'a> {
        PairReadSnapshots {
            pool: &self.pool,
            first_token_definition: &fixture.stored_b_definition,
            second_token_definition: &fixture.stored_a_definition,
            first_token_vault: &self.first_vault,
            second_token_vault: &self.second_vault,
            liquidity_definition: &self.liquidity_definition,
            lp_lock_holding: &self.lp_lock_holding,
            current_tick: &self.current_tick,
            clock: &self.clock,
        }
    }
}

fn add_input<'a>(
    fixture: &'a Fixture,
    pool_accounts: PoolAccountSnapshots<'a>,
    first_holding: &'a AccountSnapshot,
    max_first_amount: u128,
    max_second_amount: u128,
    slippage_bps: u128,
    expected_fee_bps: Option<u128>,
) -> AddLiquidityTransactionInput<'a> {
    AddLiquidityTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        pool_accounts,
        first_token_definition_id: lower_token_id(),
        second_token_definition_id: higher_token_id(),
        first_token_holding: first_holding,
        second_token_holding: &fixture.caller_second_holding,
        liquidity_holding: &fixture.liquidity_holding,
        max_first_amount,
        max_second_amount,
        slippage: SlippageTolerance::new(slippage_bps).expect("test slippage must validate"),
        expected_fee_bps,
        deadline: DEADLINE,
    }
}

#[test]
fn five_facades_emit_exact_plans_and_caller_order_amounts() {
    let fixture = Fixture::new();
    let missing = MissingPairFixture::new(&fixture);
    let fresh_lp = AccountSnapshot::new(AccountId::new([30; 32]), Account::default());
    let create = prepare_create_pool_transaction(CreatePoolTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        config: &fixture.config,
        pair: missing.pair(&fixture),
        first_token_definition_id: lower_token_id(),
        second_token_definition_id: higher_token_id(),
        first_token_holding: &fixture.caller_first_holding,
        second_token_holding: &fixture.caller_second_holding,
        liquidity_holding: &fresh_lp,
        first_amount: 4_000,
        second_amount: 9_000,
        fee_bps: FEE_TIER_BPS_30,
        deadline: DEADLINE,
    })
    .expect("funded create request must prepare");
    let Instruction::NewDefinition {
        token_a_amount,
        token_b_amount,
        deadline,
        ..
    } = create.plan().instruction()
    else {
        panic!("create facade emitted wrong instruction")
    };
    assert_eq!((*token_a_amount, *token_b_amount), (9_000, 4_000));
    assert_eq!(*deadline, DEADLINE);
    assert_eq!(create.caller_amounts().first(), 4_000);
    assert_eq!(create.caller_amounts().second(), 9_000);
    assert_eq!(
        create.wallet_prerequisites().fresh_account_ids(),
        &[fresh_lp.account_id()]
    );

    let add = prepare_add_liquidity_transaction(AddLiquidityTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        pool_accounts: fixture.pool_accounts(),
        first_token_definition_id: lower_token_id(),
        second_token_definition_id: higher_token_id(),
        first_token_holding: &fixture.caller_first_holding,
        second_token_holding: &fixture.caller_second_holding,
        liquidity_holding: &fixture.liquidity_holding,
        max_first_amount: 100,
        max_second_amount: 400,
        slippage: Fixture::slippage(),
        expected_fee_bps: Some(FEE_TIER_BPS_30),
        deadline: DEADLINE,
    })
    .expect("funded add request must prepare");
    let Instruction::AddLiquidity {
        max_amount_to_add_token_a,
        max_amount_to_add_token_b,
        ..
    } = add.plan().instruction()
    else {
        panic!("add facade emitted wrong instruction")
    };
    assert_eq!(
        (*max_amount_to_add_token_a, *max_amount_to_add_token_b),
        (400, 100)
    );
    assert_eq!(add.caller_amounts().first(), 100);
    assert_eq!(add.caller_amounts().second(), 200);
    assert_eq!(add.wallet_prerequisites().funding()[0].required(), 100);
    assert_eq!(add.wallet_prerequisites().funding()[1].required(), 400);

    let remove = prepare_remove_liquidity_transaction(RemoveLiquidityTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        pool_accounts: fixture.pool_accounts(),
        first_token_definition_id: lower_token_id(),
        second_token_definition_id: higher_token_id(),
        first_token_holding: &fixture.caller_first_holding,
        second_token_holding: &fixture.caller_second_holding,
        liquidity_holding: &fixture.liquidity_holding,
        remove_liquidity_amount: 500,
        slippage: Fixture::slippage(),
        expected_fee_bps: Some(FEE_TIER_BPS_30),
        deadline: DEADLINE,
    })
    .expect("remove request must prepare");
    assert_eq!(remove.caller_amounts().first(), 125);
    assert_eq!(remove.caller_amounts().second(), 250);

    let exact_input = prepare_swap_exact_input_transaction(SwapExactInputTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        pool_accounts: fixture.pool_accounts(),
        input_token_definition_id: lower_token_id(),
        output_token_definition_id: higher_token_id(),
        input_holding: &fixture.caller_first_holding,
        output_holding: &fixture.caller_second_holding,
        amount_in: 100,
        slippage: Fixture::slippage(),
        expected_fee_bps: Some(FEE_TIER_BPS_30),
        deadline: DEADLINE,
    })
    .expect("exact-input swap must prepare");
    assert_eq!(exact_input.caller_amounts().first(), 100);
    assert_eq!(
        exact_input.caller_amounts().second(),
        exact_input.quote().amount_out
    );
    assert_eq!(exact_input.pool_spot_change_bps(), Some(4_371));

    let exact_output = prepare_swap_exact_output_transaction(SwapExactOutputTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        pool_accounts: fixture.pool_accounts(),
        input_token_definition_id: lower_token_id(),
        output_token_definition_id: higher_token_id(),
        input_holding: &fixture.caller_first_holding,
        output_holding: &fixture.caller_second_holding,
        exact_amount_out: 100,
        slippage: Fixture::slippage(),
        expected_fee_bps: Some(FEE_TIER_BPS_30),
        deadline: DEADLINE,
    })
    .expect("exact-output swap must prepare");
    assert_eq!(
        exact_output.caller_amounts().first(),
        exact_output.quote().amount_in
    );
    assert_eq!(exact_output.caller_amounts().second(), 100);
    assert!(exact_output.pool_spot_change_bps().is_some());
    let Instruction::SwapExactOutput { max_amount_in, .. } = exact_output.plan().instruction()
    else {
        panic!("exact-output facade emitted wrong instruction");
    };
    assert_eq!(
        exact_output.wallet_prerequisites().funding()[0].required(),
        *max_amount_in
    );
    assert!(*max_amount_in > exact_output.quote().amount_in);

    for (plan, affected) in [
        (create.plan(), create.affected_account_ids()),
        (add.plan(), add.affected_account_ids()),
        (remove.plan(), remove.affected_account_ids()),
        (exact_input.plan(), exact_input.affected_account_ids()),
        (exact_output.plan(), exact_output.affected_account_ids()),
    ] {
        let words = plan
            .instruction_data()
            .expect("prepared instruction must encode");
        let decoded: Instruction =
            risc0_zkvm::serde::from_slice(&words).expect("guest codec must decode plan");
        assert_eq!(
            risc0_zkvm::serde::to_vec(&decoded).expect("decoded instruction must encode"),
            words
        );
        assert_eq!(affected, plan.affected_account_ids());
    }
}

#[test]
fn exact_output_requires_funding_through_its_maximum_input_guard() {
    let fixture = Fixture::new();
    let funded = prepare_swap_exact_output_transaction(SwapExactOutputTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        pool_accounts: fixture.pool_accounts(),
        input_token_definition_id: lower_token_id(),
        output_token_definition_id: higher_token_id(),
        input_holding: &fixture.caller_first_holding,
        output_holding: &fixture.caller_second_holding,
        exact_amount_out: 100,
        slippage: Fixture::slippage(),
        expected_fee_bps: Some(FEE_TIER_BPS_30),
        deadline: DEADLINE,
    })
    .expect("funded exact-output request must prepare");
    let quoted_input = funded.quote().amount_in;
    let required = funded.wallet_prerequisites().funding()[0].required();
    assert!(required > quoted_input);

    let quote_only_balance = holding(AccountId::new([20; 32]), lower_token_id(), quoted_input);
    let result = prepare_swap_exact_output_transaction(SwapExactOutputTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        pool_accounts: fixture.pool_accounts(),
        input_token_definition_id: lower_token_id(),
        output_token_definition_id: higher_token_id(),
        input_holding: &quote_only_balance,
        output_holding: &fixture.caller_second_holding,
        exact_amount_out: 100,
        slippage: Fixture::slippage(),
        expected_fee_bps: Some(FEE_TIER_BPS_30),
        deadline: DEADLINE,
    });
    let Err(error) = result else {
        panic!("balance below maximum-input guard must fail");
    };
    assert!(matches!(
        error,
        TransactionError::Client(amm_client::ClientError::InsufficientBalance {
            available,
            required: actual_required,
            ..
        }) if available == quoted_input && actual_required == required
    ));
}

#[test]
fn commitment_is_stable_and_changes_with_bound_snapshot_or_deadline() {
    let fixture = Fixture::new();
    let missing = MissingPairFixture::new(&fixture);
    let fresh_lp = AccountSnapshot::new(AccountId::new([30; 32]), Account::default());
    let prepare = |first_holding: &AccountSnapshot, deadline| {
        prepare_create_pool_transaction(CreatePoolTransactionInput {
            amm_program_id: AMM_PROGRAM_ID,
            config: &fixture.config,
            pair: missing.pair(&fixture),
            first_token_definition_id: lower_token_id(),
            second_token_definition_id: higher_token_id(),
            first_token_holding: first_holding,
            second_token_holding: &fixture.caller_second_holding,
            liquidity_holding: &fresh_lp,
            first_amount: 4_000,
            second_amount: 9_000,
            fee_bps: FEE_TIER_BPS_30,
            deadline,
        })
        .expect("create request must prepare")
    };

    let first = prepare(&fixture.caller_first_holding, DEADLINE);
    let repeated = prepare(&fixture.caller_first_holding, DEADLINE);
    assert_eq!(first.quote_commitment(), repeated.quote_commitment());

    let changed_holding = holding(AccountId::new([20; 32]), lower_token_id(), 10_001);
    let changed_snapshot = prepare(&changed_holding, DEADLINE);
    assert_ne!(
        first.quote_commitment(),
        changed_snapshot.quote_commitment()
    );
    assert!(matches!(
        ensure_quote_unchanged(
            first.quote_commitment(),
            changed_snapshot.quote_commitment()
        ),
        Err(TransactionError::QuoteChanged { .. })
    ));

    let changed_deadline = prepare(&fixture.caller_first_holding, DEADLINE + 1);
    assert_ne!(
        first.quote_commitment(),
        changed_deadline.quote_commitment()
    );
}

#[test]
fn create_and_add_reject_underfunded_selected_holdings() {
    let fixture = Fixture::new();
    let missing = MissingPairFixture::new(&fixture);
    let fresh_lp = AccountSnapshot::new(AccountId::new([30; 32]), Account::default());
    let underfunded_first = holding(AccountId::new([20; 32]), lower_token_id(), 3_999);
    let error = prepare_create_pool_transaction(CreatePoolTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        config: &fixture.config,
        pair: missing.pair(&fixture),
        first_token_definition_id: lower_token_id(),
        second_token_definition_id: higher_token_id(),
        first_token_holding: &underfunded_first,
        second_token_holding: &fixture.caller_second_holding,
        liquidity_holding: &fresh_lp,
        first_amount: 4_000,
        second_amount: 9_000,
        fee_bps: FEE_TIER_BPS_30,
        deadline: DEADLINE,
    })
    .err()
    .expect("underfunded create must fail");
    assert!(matches!(
        error,
        TransactionError::Client(amm_client::ClientError::InsufficientBalance {
            required: 4_000,
            ..
        })
    ));

    // Expected transfer is 200, but the instruction may spend up to the caller's 400-unit cap.
    let underfunded_second = holding(AccountId::new([21; 32]), higher_token_id(), 399);
    let error = prepare_add_liquidity_transaction(AddLiquidityTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        pool_accounts: fixture.pool_accounts(),
        first_token_definition_id: lower_token_id(),
        second_token_definition_id: higher_token_id(),
        first_token_holding: &fixture.caller_first_holding,
        second_token_holding: &underfunded_second,
        liquidity_holding: &fixture.liquidity_holding,
        max_first_amount: 100,
        max_second_amount: 400,
        slippage: Fixture::slippage(),
        expected_fee_bps: Some(FEE_TIER_BPS_30),
        deadline: DEADLINE,
    })
    .err()
    .expect("holding below the add spend cap must fail");
    assert!(matches!(
        error,
        TransactionError::Client(amm_client::ClientError::InsufficientBalance {
            required: 400,
            ..
        })
    ));
}

#[test]
fn add_accepts_only_explicit_default_snapshot_as_fresh_lp_destination() {
    let fixture = Fixture::new();
    let fresh_lp = AccountSnapshot::new(AccountId::new([31; 32]), Account::default());
    let prepared = prepare_add_liquidity_transaction(AddLiquidityTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        pool_accounts: fixture.pool_accounts(),
        first_token_definition_id: lower_token_id(),
        second_token_definition_id: higher_token_id(),
        first_token_holding: &fixture.caller_first_holding,
        second_token_holding: &fixture.caller_second_holding,
        liquidity_holding: &fresh_lp,
        max_first_amount: 100,
        max_second_amount: 400,
        slippage: Fixture::slippage(),
        expected_fee_bps: Some(FEE_TIER_BPS_30),
        deadline: DEADLINE,
    })
    .expect("explicit default LP snapshot must be accepted");
    assert_eq!(
        prepared.wallet_prerequisites().fresh_account_ids(),
        &[fresh_lp.account_id()]
    );

    let wrong_lp = holding(AccountId::new([31; 32]), lower_token_id(), 0);
    let error = prepare_add_liquidity_transaction(AddLiquidityTransactionInput {
        liquidity_holding: &wrong_lp,
        ..AddLiquidityTransactionInput {
            amm_program_id: AMM_PROGRAM_ID,
            pool_accounts: fixture.pool_accounts(),
            first_token_definition_id: lower_token_id(),
            second_token_definition_id: higher_token_id(),
            first_token_holding: &fixture.caller_first_holding,
            second_token_holding: &fixture.caller_second_holding,
            liquidity_holding: &fresh_lp,
            max_first_amount: 100,
            max_second_amount: 400,
            slippage: Fixture::slippage(),
            expected_fee_bps: Some(FEE_TIER_BPS_30),
            deadline: DEADLINE,
        }
    })
    .err()
    .expect("initialized holding for wrong definition must fail");
    assert_eq!(error.code(), "token_definition_mismatch");
}

#[test]
fn lifecycle_tick_clock_and_expected_fee_are_validated_before_planning() {
    let fixture = Fixture::new();
    let fresh_lp = AccountSnapshot::new(AccountId::new([30; 32]), Account::default());
    let active_create = prepare_create_pool_transaction(CreatePoolTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        config: &fixture.config,
        pair: fixture.pool_accounts().pair,
        first_token_definition_id: lower_token_id(),
        second_token_definition_id: higher_token_id(),
        first_token_holding: &fixture.caller_first_holding,
        second_token_holding: &fixture.caller_second_holding,
        liquidity_holding: &fresh_lp,
        first_amount: 4_000,
        second_amount: 9_000,
        fee_bps: FEE_TIER_BPS_30,
        deadline: DEADLINE,
    })
    .err()
    .expect("active pool must not prepare as creation");
    assert_eq!(active_create.code(), "invalid_account_data");

    let wrong_tick = AccountSnapshot::new(
        AccountId::new([99; 32]),
        fixture.current_tick.account().clone(),
    );
    let error = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts_with(&fixture.config, &wrong_tick, &fixture.clock),
        &fixture.caller_first_holding,
        100,
        400,
        100,
        Some(FEE_TIER_BPS_30),
    ))
    .err()
    .expect("mismatched current tick must fail");
    assert_eq!(error.code(), "account_id_mismatch");

    let wrong_clock =
        AccountSnapshot::new(AccountId::new([98; 32]), fixture.clock.account().clone());
    let error = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts_with(&fixture.config, &fixture.current_tick, &wrong_clock),
        &fixture.caller_first_holding,
        100,
        400,
        100,
        Some(FEE_TIER_BPS_30),
    ))
    .err()
    .expect("mismatched clock must fail");
    assert_eq!(error.code(), "account_id_mismatch");

    let mismatch = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts(),
        &fixture.caller_first_holding,
        100,
        400,
        100,
        Some(100),
    ))
    .err()
    .expect("caller fee expectation must be checked");
    assert!(matches!(
        mismatch,
        TransactionError::FeeMismatch {
            expected: 100,
            actual: FEE_TIER_BPS_30,
        }
    ));

    let expected = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts(),
        &fixture.caller_first_holding,
        100,
        400,
        100,
        Some(FEE_TIER_BPS_30),
    ))
    .expect("matching expected fee must prepare");
    let unspecified = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts(),
        &fixture.caller_first_holding,
        100,
        400,
        100,
        None,
    ))
    .expect("unspecified expected fee must prepare from pool state");
    assert_eq!(
        expected.plan().instruction_data(),
        unspecified.plan().instruction_data()
    );
    assert_eq!(expected.quote(), unspecified.quote());
}

#[test]
fn commitment_binds_intent_order_selection_and_quote_sources_only() {
    let fixture = Fixture::new();
    let base = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts(),
        &fixture.caller_first_holding,
        100,
        400,
        1,
        Some(FEE_TIER_BPS_30),
    ))
    .expect("base add must prepare");

    // One- and two-basis-point tolerances both floor this quote's minimum LP to the same value.
    // The typed intent still distinguishes them.
    let changed_slippage = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts(),
        &fixture.caller_first_holding,
        100,
        400,
        2,
        Some(FEE_TIER_BPS_30),
    ))
    .expect("changed slippage must prepare");
    assert_eq!(
        base.plan().instruction_data(),
        changed_slippage.plan().instruction_data()
    );
    assert_ne!(base.quote_commitment(), changed_slippage.quote_commitment());

    let changed_cap = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts(),
        &fixture.caller_first_holding,
        101,
        400,
        1,
        Some(FEE_TIER_BPS_30),
    ))
    .expect("changed cap must prepare");
    assert_ne!(base.quote_commitment(), changed_cap.quote_commitment());

    let no_fee_expectation = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts(),
        &fixture.caller_first_holding,
        100,
        400,
        1,
        None,
    ))
    .expect("optional fee expectation must not alter quote logic");
    assert_eq!(
        base.plan().instruction_data(),
        no_fee_expectation.plan().instruction_data()
    );
    assert_ne!(
        base.quote_commitment(),
        no_fee_expectation.quote_commitment()
    );

    let stored_order = prepare_add_liquidity_transaction(AddLiquidityTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        pool_accounts: fixture.stored_order_pool_accounts(),
        first_token_definition_id: higher_token_id(),
        second_token_definition_id: lower_token_id(),
        first_token_holding: &fixture.caller_second_holding,
        second_token_holding: &fixture.caller_first_holding,
        liquidity_holding: &fixture.liquidity_holding,
        max_first_amount: 400,
        max_second_amount: 100,
        slippage: SlippageTolerance::new(1).expect("test slippage"),
        expected_fee_bps: Some(FEE_TIER_BPS_30),
        deadline: DEADLINE,
    })
    .expect("stored caller order must prepare");
    assert_eq!(
        base.plan().instruction_data(),
        stored_order.plan().instruction_data()
    );
    assert_ne!(base.quote_commitment(), stored_order.quote_commitment());

    let alternate_holding = holding(AccountId::new([24; 32]), lower_token_id(), 10_000);
    let changed_selection = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts(),
        &alternate_holding,
        100,
        400,
        1,
        Some(FEE_TIER_BPS_30),
    ))
    .expect("alternate funded holding must prepare");
    assert_ne!(
        base.quote_commitment(),
        changed_selection.quote_commitment()
    );

    let changed_config_data = AmmConfig {
        token_program_id: TOKEN_PROGRAM_ID,
        twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
        authority: AccountId::new([8; 32]),
    };
    let changed_config = AccountSnapshot::new(
        fixture.config.account_id(),
        account(AMM_PROGRAM_ID, Data::from(&changed_config_data)),
    );
    let changed_source = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts_with(&changed_config, &fixture.current_tick, &fixture.clock),
        &fixture.caller_first_holding,
        100,
        400,
        1,
        Some(FEE_TIER_BPS_30),
    ))
    .expect("non-economic config source change must prepare");
    assert_eq!(
        base.plan().instruction_data(),
        changed_source.plan().instruction_data()
    );
    assert_ne!(base.quote_commitment(), changed_source.quote_commitment());

    let changed_tick = AccountSnapshot::new(
        fixture.current_tick.account_id(),
        account(
            TWAP_ORACLE_PROGRAM_ID,
            Data::from(&CurrentTickAccount {
                tick: -1,
                last_updated: 401,
            }),
        ),
    );
    let changed_clock_data = ClockAccountData {
        block_id: 124,
        timestamp: 457,
    }
    .to_bytes();
    let changed_clock = AccountSnapshot::new(
        CLOCK_01_PROGRAM_ACCOUNT_ID,
        account(
            [88; 8],
            Data::try_from(changed_clock_data).expect("clock data must fit"),
        ),
    );
    let ephemeral_change = prepare_add_liquidity_transaction(add_input(
        &fixture,
        fixture.pool_accounts_with(&fixture.config, &changed_tick, &changed_clock),
        &fixture.caller_first_holding,
        100,
        400,
        1,
        Some(FEE_TIER_BPS_30),
    ))
    .expect("valid tick and clock refresh must prepare");
    assert_eq!(base.quote_commitment(), ephemeral_change.quote_commitment());
}

#[test]
fn rejects_account_aliases_that_make_the_runtime_plan_unexecutable() {
    let fixture = Fixture::new();
    let result = prepare_remove_liquidity_transaction(RemoveLiquidityTransactionInput {
        amm_program_id: AMM_PROGRAM_ID,
        pool_accounts: fixture.pool_accounts(),
        first_token_definition_id: lower_token_id(),
        second_token_definition_id: higher_token_id(),
        first_token_holding: &fixture.vault_b,
        second_token_holding: &fixture.caller_second_holding,
        liquidity_holding: &fixture.liquidity_holding,
        remove_liquidity_amount: 500,
        slippage: Fixture::slippage(),
        expected_fee_bps: Some(FEE_TIER_BPS_30),
        deadline: DEADLINE,
    });
    let Err(error) = result else {
        panic!("holding aliases must not produce duplicate planned account IDs");
    };

    assert_eq!(
        error,
        TransactionError::DuplicateAccountId {
            account_id: fixture.vault_b.account_id(),
        }
    );
    assert_eq!(error.code(), "duplicate_account_id");
}
