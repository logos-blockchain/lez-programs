use amm_client::{
    encode_instruction, plan_add_liquidity, plan_create_oracle_price_account, plan_create_pool,
    plan_create_price_observations, plan_initialize, plan_remove_liquidity, plan_swap_exact_input,
    plan_swap_exact_output, plan_sync_reserves, plan_update_config, AccountRole,
    AddLiquidityPlanInput, AmmContext, ClientError, CreateOraclePriceAccountPlanInput,
    CreatePoolPlanInput, CreatePriceObservationsPlanInput, InitializePlanInput, PoolContext,
    RemoveLiquidityPlanInput, SwapExactInputPlanInput, SwapExactOutputPlanInput,
    SyncReservesPlanInput, TransactionPlan, UpdateConfigPlanInput,
};
use amm_core::{AmmConfig, Instruction, PoolDefinition};
use amm_program::quote as program_quote;
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{account::AccountId, program::ProgramId};
use serde_json::Value;
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda,
};

const LARGE_EXACT_INTEGER: u128 = 9_007_199_254_740_993;
const WINDOW_DURATION: u64 = 86_400_000;

fn account(byte: u8) -> AccountId {
    AccountId::new([byte; 32])
}

const fn program(word: u32) -> ProgramId {
    [word; 8]
}

fn context() -> AmmContext {
    AmmContext::new(
        program(42),
        AmmConfig {
            token_program_id: program(15),
            twap_oracle_program_id: program(77),
            authority: account(9),
        },
    )
}

fn pool_fixture(context: &AmmContext) -> (AccountId, PoolDefinition) {
    let definition_a = account(3);
    let definition_b = account(4);
    let pool_id = amm_core::compute_pool_pda(context.amm_program_id, definition_a, definition_b);
    (
        pool_id,
        PoolDefinition {
            definition_token_a_id: definition_a,
            definition_token_b_id: definition_b,
            vault_a_id: amm_core::compute_vault_pda(context.amm_program_id, pool_id, definition_a),
            vault_b_id: amm_core::compute_vault_pda(context.amm_program_id, pool_id, definition_b),
            liquidity_pool_id: amm_core::compute_liquidity_token_pda(
                context.amm_program_id,
                pool_id,
            ),
            liquidity_pool_supply: 10_000,
            reserve_a: 20_000,
            reserve_b: 30_000,
            fees: amm_core::FEE_TIER_BPS_30,
        },
    )
}

fn all_plans() -> Vec<TransactionPlan> {
    let context = context();
    let (pool_id, pool) = pool_fixture(&context);
    let pool = PoolContext::new(&context, pool_id, &pool).expect("valid pool fixture");

    vec![
        plan_initialize(InitializePlanInput {
            amm_program_id: context.amm_program_id,
            token_program_id: context.token_program_id(),
            twap_oracle_program_id: context.twap_oracle_program_id(),
            authority: context.config.authority,
        }),
        plan_update_config(UpdateConfigPlanInput {
            context: &context,
            token_program_id: Some(program(16)),
            twap_oracle_program_id: Some(program(78)),
            new_authority: Some(account(10)),
        }),
        plan_create_price_observations(CreatePriceObservationsPlanInput {
            context: &context,
            pool_id,
            window_duration: WINDOW_DURATION,
        }),
        plan_create_oracle_price_account(CreateOraclePriceAccountPlanInput {
            context: &context,
            pool_id,
            window_duration: WINDOW_DURATION,
        }),
        plan_create_pool(CreatePoolPlanInput {
            context: &context,
            token_a_definition_id: account(3),
            token_b_definition_id: account(4),
            user_holding_a: account(31),
            user_holding_b: account(32),
            user_holding_lp: account(33),
            token_a_amount: 20_000,
            token_b_amount: 30_000,
            fees: amm_core::FEE_TIER_BPS_30,
            deadline: u64::MAX,
        })
        .expect("distinct pool definitions"),
        plan_add_liquidity(AddLiquidityPlanInput {
            context: &context,
            pool,
            user_holding_a: account(31),
            user_holding_b: account(32),
            user_holding_lp: account(33),
            min_amount_liquidity: 1,
            max_amount_to_add_token_a: 200,
            max_amount_to_add_token_b: 300,
            deadline: u64::MAX,
        }),
        plan_remove_liquidity(RemoveLiquidityPlanInput {
            context: &context,
            pool,
            user_holding_a: account(31),
            user_holding_b: account(32),
            user_holding_lp: account(33),
            remove_liquidity_amount: 100,
            min_amount_to_remove_token_a: 1,
            min_amount_to_remove_token_b: 1,
            deadline: u64::MAX,
        }),
        plan_swap_exact_input(SwapExactInputPlanInput {
            context: &context,
            pool,
            user_input_holding: account(31),
            user_output_holding: account(32),
            swap_amount_in: LARGE_EXACT_INTEGER,
            min_amount_out: 1,
            deadline: u64::MAX,
        }),
        plan_swap_exact_output(SwapExactOutputPlanInput {
            context: &context,
            pool,
            user_input_holding: account(32),
            user_output_holding: account(31),
            exact_amount_out: 10,
            max_amount_in: LARGE_EXACT_INTEGER,
            deadline: u64::MAX,
        }),
        plan_sync_reserves(SyncReservesPlanInput {
            context: &context,
            pool,
        }),
    ]
}

#[test]
fn every_instruction_round_trips_through_guest_codec() {
    let expected_indices = [0_u32, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    let plans = all_plans();
    assert_eq!(plans.len(), expected_indices.len());

    for (plan, expected_index) in plans.iter().zip(expected_indices) {
        let words = plan.instruction_data().expect("instruction must serialize");
        assert_eq!(
            words,
            encode_instruction(plan.instruction()).expect("direct encoding")
        );
        assert_eq!(words.first().copied(), Some(expected_index));

        let decoded: Instruction =
            risc0_zkvm::serde::from_slice(&words).expect("guest codec must decode words");
        assert_eq!(variant_index(&decoded), expected_index);
        assert_eq!(
            encode_instruction(&decoded).expect("decoded instruction must serialize"),
            words
        );
    }
}

#[test]
fn update_config_none_options_round_trip() {
    let instruction = Instruction::UpdateConfig {
        token_program_id: None,
        twap_oracle_program_id: None,
        new_authority: None,
    };
    let words = encode_instruction(&instruction).expect("instruction must serialize");
    let decoded: Instruction =
        risc0_zkvm::serde::from_slice(&words).expect("instruction must deserialize");

    assert!(matches!(
        decoded,
        Instruction::UpdateConfig {
            token_program_id: None,
            twap_oracle_program_id: None,
            new_authority: None,
        }
    ));
}

#[test]
fn u128_above_javascript_integer_range_is_exact() {
    let instruction = Instruction::SwapExactInput {
        swap_amount_in: LARGE_EXACT_INTEGER,
        min_amount_out: LARGE_EXACT_INTEGER,
        deadline: u64::MAX,
    };
    let words = encode_instruction(&instruction).expect("instruction must serialize");
    let decoded: Instruction =
        risc0_zkvm::serde::from_slice(&words).expect("instruction must deserialize");

    let Instruction::SwapExactInput {
        swap_amount_in,
        min_amount_out,
        deadline,
    } = decoded
    else {
        panic!("decoded wrong instruction variant");
    };
    assert_eq!(swap_amount_in, LARGE_EXACT_INTEGER);
    assert_eq!(min_amount_out, LARGE_EXACT_INTEGER);
    assert_eq!(deadline, u64::MAX);
}

#[test]
fn planner_account_contract_matches_checked_in_idl() {
    let idl: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../artifacts/amm-idl.json"
    )))
    .expect("checked-in AMM IDL must be JSON");
    assert_eq!(
        idl.get("instruction_type").and_then(Value::as_str),
        Some("amm_core::Instruction")
    );
    let idl_instructions = idl
        .get("instructions")
        .and_then(Value::as_array)
        .expect("IDL instructions array");
    let plans = all_plans();
    assert_eq!(idl_instructions.len(), plans.len());

    for (idl_instruction, plan) in idl_instructions.iter().zip(plans.iter()) {
        assert_eq!(
            string_field(idl_instruction, "name"),
            plan.instruction_name()
        );
        let idl_accounts = idl_instruction
            .get("accounts")
            .and_then(Value::as_array)
            .expect("IDL accounts array");
        assert_eq!(idl_accounts.len(), plan.accounts().len());

        for (idl_account, planned_account) in idl_accounts.iter().zip(plan.accounts()) {
            assert_eq!(
                string_field(idl_account, "name"),
                planned_account.role().as_str()
            );
            assert_eq!(
                bool_field(idl_account, "writable"),
                planned_account.writable()
            );
            assert_eq!(bool_field(idl_account, "signer"), planned_account.signer());
            assert_eq!(bool_field(idl_account, "init"), planned_account.init());
        }
    }
}

#[test]
fn signer_sets_follow_guest_account_order() {
    let plans = all_plans();
    let expected = vec![
        vec![],
        vec![account(9)],
        vec![],
        vec![],
        vec![account(31), account(32), account(33)],
        vec![account(31), account(32)],
        vec![account(33)],
        vec![account(31)],
        vec![account(32)],
        vec![],
    ];
    assert_eq!(plans.len(), expected.len());

    for (plan, expected_signers) in plans.iter().zip(expected) {
        assert_eq!(plan.signer_account_ids(), expected_signers);
    }
}

#[test]
fn account_ids_and_signer_flags_stay_positionally_aligned() {
    for plan in all_plans() {
        let account_ids = plan.account_ids();
        let signer_flags = plan.signer_flags();
        assert_eq!(account_ids.len(), signer_flags.len());

        let filtered_ids: Vec<AccountId> = account_ids
            .into_iter()
            .zip(signer_flags)
            .filter_map(|(account_id, signer)| signer.then_some(account_id))
            .collect();
        assert_eq!(filtered_ids, plan.signer_account_ids());
    }
}

#[test]
fn quote_results_feed_instruction_amounts_and_guards_without_recalculation() {
    let context = context();
    let (pool_id, pool_definition) = pool_fixture(&context);
    let pool = PoolContext::new(&context, pool_id, &pool_definition).expect("valid pool fixture");

    let create_quote =
        program_quote::create_pool(20_000, 30_000, amm_core::FEE_TIER_BPS_30).expect("pool quote");
    let create_plan = plan_create_pool(CreatePoolPlanInput {
        context: &context,
        token_a_definition_id: pool_definition.definition_token_a_id,
        token_b_definition_id: pool_definition.definition_token_b_id,
        user_holding_a: account(31),
        user_holding_b: account(32),
        user_holding_lp: account(33),
        token_a_amount: create_quote.pool.reserve_a,
        token_b_amount: create_quote.pool.reserve_b,
        fees: amm_core::FEE_TIER_BPS_30,
        deadline: u64::MAX,
    })
    .expect("create plan");
    let Instruction::NewDefinition {
        token_a_amount,
        token_b_amount,
        fees,
        ..
    } = create_plan.instruction()
    else {
        panic!("create planner emitted wrong instruction");
    };
    assert_eq!(*token_a_amount, create_quote.pool.reserve_a);
    assert_eq!(*token_b_amount, create_quote.pool.reserve_b);
    assert_eq!(*fees, amm_core::FEE_TIER_BPS_30);

    let add_preview = program_quote::preview_add_liquidity(
        &pool_definition,
        pool_definition.reserve_a,
        pool_definition.reserve_b,
        200,
        300,
    )
    .expect("add preview");
    let add_quote = program_quote::add_liquidity(
        &pool_definition,
        pool_definition.reserve_a,
        pool_definition.reserve_b,
        add_preview.actual_amount_a,
        add_preview.actual_amount_b,
        add_preview.liquidity_to_mint,
    )
    .expect("exact add quote");
    let add_plan = plan_add_liquidity(AddLiquidityPlanInput {
        context: &context,
        pool,
        user_holding_a: account(31),
        user_holding_b: account(32),
        user_holding_lp: account(33),
        min_amount_liquidity: add_quote.liquidity_to_mint,
        max_amount_to_add_token_a: add_quote.actual_amount_a,
        max_amount_to_add_token_b: add_quote.actual_amount_b,
        deadline: u64::MAX,
    });
    let Instruction::AddLiquidity {
        min_amount_liquidity,
        max_amount_to_add_token_a,
        max_amount_to_add_token_b,
        ..
    } = add_plan.instruction()
    else {
        panic!("add planner emitted wrong instruction");
    };
    assert_eq!(*min_amount_liquidity, add_quote.liquidity_to_mint);
    assert_eq!(*max_amount_to_add_token_a, add_quote.actual_amount_a);
    assert_eq!(*max_amount_to_add_token_b, add_quote.actual_amount_b);

    let remove_preview = program_quote::preview_remove_liquidity(&pool_definition, 500, 100)
        .expect("remove preview");
    let remove_quote = program_quote::remove_liquidity(
        &pool_definition,
        500,
        remove_preview.liquidity_to_burn,
        remove_preview.withdraw_amount_a,
        remove_preview.withdraw_amount_b,
    )
    .expect("exact remove quote");
    let remove_plan = plan_remove_liquidity(RemoveLiquidityPlanInput {
        context: &context,
        pool,
        user_holding_a: account(31),
        user_holding_b: account(32),
        user_holding_lp: account(33),
        remove_liquidity_amount: remove_quote.liquidity_to_burn,
        min_amount_to_remove_token_a: remove_quote.withdraw_amount_a,
        min_amount_to_remove_token_b: remove_quote.withdraw_amount_b,
        deadline: u64::MAX,
    });
    let Instruction::RemoveLiquidity {
        remove_liquidity_amount,
        min_amount_to_remove_token_a,
        min_amount_to_remove_token_b,
        ..
    } = remove_plan.instruction()
    else {
        panic!("remove planner emitted wrong instruction");
    };
    assert_eq!(*remove_liquidity_amount, remove_quote.liquidity_to_burn);
    assert_eq!(
        *min_amount_to_remove_token_a,
        remove_quote.withdraw_amount_a
    );
    assert_eq!(
        *min_amount_to_remove_token_b,
        remove_quote.withdraw_amount_b
    );

    let swap_input_preview = program_quote::preview_swap_exact_input(
        &pool_definition,
        pool_definition.reserve_a,
        pool_definition.reserve_b,
        program_quote::SwapDirection::AToB,
        100,
    )
    .expect("exact-input preview");
    let swap_input_quote = program_quote::swap_exact_input(
        &pool_definition,
        pool_definition.reserve_a,
        pool_definition.reserve_b,
        program_quote::SwapDirection::AToB,
        swap_input_preview.amount_in,
        swap_input_preview.amount_out,
    )
    .expect("exact-input quote");
    let swap_input_plan = plan_swap_exact_input(SwapExactInputPlanInput {
        context: &context,
        pool,
        user_input_holding: account(31),
        user_output_holding: account(32),
        swap_amount_in: swap_input_quote.amount_in,
        min_amount_out: swap_input_quote.amount_out,
        deadline: u64::MAX,
    });
    let Instruction::SwapExactInput {
        swap_amount_in,
        min_amount_out,
        ..
    } = swap_input_plan.instruction()
    else {
        panic!("exact-input planner emitted wrong instruction");
    };
    assert_eq!(*swap_amount_in, swap_input_quote.amount_in);
    assert_eq!(*min_amount_out, swap_input_quote.amount_out);

    let swap_output_preview = program_quote::preview_swap_exact_output(
        &pool_definition,
        pool_definition.reserve_a,
        pool_definition.reserve_b,
        program_quote::SwapDirection::BToA,
        100,
    )
    .expect("exact-output preview");
    let swap_output_quote = program_quote::swap_exact_output(
        &pool_definition,
        pool_definition.reserve_a,
        pool_definition.reserve_b,
        program_quote::SwapDirection::BToA,
        swap_output_preview.amount_out,
        swap_output_preview.amount_in,
    )
    .expect("exact-output quote");
    let swap_output_plan = plan_swap_exact_output(SwapExactOutputPlanInput {
        context: &context,
        pool,
        user_input_holding: account(32),
        user_output_holding: account(31),
        exact_amount_out: swap_output_quote.amount_out,
        max_amount_in: swap_output_quote.amount_in,
        deadline: u64::MAX,
    });
    let Instruction::SwapExactOutput {
        exact_amount_out,
        max_amount_in,
        ..
    } = swap_output_plan.instruction()
    else {
        panic!("exact-output planner emitted wrong instruction");
    };
    assert_eq!(*exact_amount_out, swap_output_quote.amount_out);
    assert_eq!(*max_amount_in, swap_output_quote.amount_in);
}

#[test]
fn planners_derive_protocol_accounts_from_canonical_helpers() {
    let context = context();
    let (pool_id, pool) = pool_fixture(&context);

    let observations = plan_create_price_observations(CreatePriceObservationsPlanInput {
        context: &context,
        pool_id,
        window_duration: WINDOW_DURATION,
    });
    assert_eq!(
        account_for_role(&observations, AccountRole::CurrentTickAccount),
        compute_current_tick_account_pda(context.twap_oracle_program_id(), pool_id)
    );
    assert_eq!(
        account_for_role(&observations, AccountRole::PriceObservations),
        compute_price_observations_pda(context.twap_oracle_program_id(), pool_id, WINDOW_DURATION)
    );
    assert_eq!(
        account_for_role(&observations, AccountRole::Clock),
        CLOCK_01_PROGRAM_ACCOUNT_ID
    );

    let oracle = plan_create_oracle_price_account(CreateOraclePriceAccountPlanInput {
        context: &context,
        pool_id,
        window_duration: WINDOW_DURATION,
    });
    assert_eq!(
        account_for_role(&oracle, AccountRole::OraclePriceAccount),
        compute_oracle_price_account_pda(
            context.twap_oracle_program_id(),
            pool_id,
            WINDOW_DURATION
        )
    );

    let create = plan_create_pool(CreatePoolPlanInput {
        context: &context,
        token_a_definition_id: pool.definition_token_a_id,
        token_b_definition_id: pool.definition_token_b_id,
        user_holding_a: account(31),
        user_holding_b: account(32),
        user_holding_lp: account(33),
        token_a_amount: 20_000,
        token_b_amount: 30_000,
        fees: amm_core::FEE_TIER_BPS_30,
        deadline: u64::MAX,
    })
    .expect("distinct definitions");
    assert_eq!(account_for_role(&create, AccountRole::Pool), pool_id);
    assert_eq!(
        account_for_role(&create, AccountRole::VaultA),
        pool.vault_a_id
    );
    assert_eq!(
        account_for_role(&create, AccountRole::VaultB),
        pool.vault_b_id
    );
    assert_eq!(
        account_for_role(&create, AccountRole::PoolDefinitionLp),
        pool.liquidity_pool_id
    );
    assert_eq!(
        account_for_role(&create, AccountRole::LpLockHolding),
        amm_core::compute_lp_lock_holding_pda(context.amm_program_id, pool_id)
    );
}

#[test]
fn equal_token_pool_returns_error_without_panicking() {
    let context = context();
    let result = plan_create_pool(CreatePoolPlanInput {
        context: &context,
        token_a_definition_id: account(3),
        token_b_definition_id: account(3),
        user_holding_a: account(31),
        user_holding_b: account(32),
        user_holding_lp: account(33),
        token_a_amount: 20_000,
        token_b_amount: 30_000,
        fees: amm_core::FEE_TIER_BPS_30,
        deadline: u64::MAX,
    });

    assert!(matches!(
        result,
        Err(ClientError::IdenticalTokenDefinitions)
    ));
}

#[test]
fn pool_context_rejects_noncanonical_identity_fields() {
    let context = context();
    let (pool_id, mut pool) = pool_fixture(&context);
    pool.vault_a_id = account(200);

    let result = PoolContext::new(&context, pool_id, &pool);
    assert!(matches!(
        result,
        Err(ClientError::AccountIdMismatch {
            account: "vault_a",
            ..
        })
    ));
}

fn account_for_role(plan: &TransactionPlan, role: AccountRole) -> AccountId {
    plan.accounts()
        .iter()
        .find(|account| account.role() == role)
        .map(|account| account.id())
        .expect("plan must contain requested role")
}

fn string_field<'a>(value: &'a Value, field: &str) -> &'a str {
    value
        .get(field)
        .and_then(Value::as_str)
        .expect("IDL string field")
}

fn bool_field(value: &Value, field: &str) -> bool {
    value
        .get(field)
        .and_then(Value::as_bool)
        .expect("IDL boolean field")
}

const fn variant_index(instruction: &Instruction) -> u32 {
    match instruction {
        Instruction::Initialize { .. } => 0,
        Instruction::UpdateConfig { .. } => 1,
        Instruction::CreatePriceObservations { .. } => 2,
        Instruction::CreateOraclePriceAccount { .. } => 3,
        Instruction::NewDefinition { .. } => 4,
        Instruction::AddLiquidity { .. } => 5,
        Instruction::RemoveLiquidity { .. } => 6,
        Instruction::SwapExactInput { .. } => 7,
        Instruction::SwapExactOutput { .. } => 8,
        Instruction::SyncReserves => 9,
    }
}
