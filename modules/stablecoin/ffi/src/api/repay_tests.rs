//! Repayment contract tests spanning plans, native execution, and chained burns.

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use stablecoin_core::{
    compute_position_pda, compute_position_vault_pda, compute_protocol_parameters_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    math::FIXED_POINT_ONE, Instruction, Position, ProtocolParameters, StabilityFeeAccumulator,
};
use token_core::{TokenDefinition, TokenHolding};

use super::{repay_debt_plan, RepayDebtPlanRequest};
use crate::{
    account::{account_id_hex, account_read, decode_account, program_id_bytes},
    AccountRead,
};

const PROGRAM: ProgramId = [0x11; 8];
const TOKEN_PROGRAM: ProgramId = [0x22; 8];
const NOW: u64 = 1_000;
const NONCE: u64 = u64::MAX;

fn id(seed: u8) -> AccountId {
    AccountId::new([seed; 32])
}

fn read(account_id: AccountId, owner: ProgramId, data: Data) -> AccountRead {
    account_read(
        account_id,
        &Account {
            program_owner: owner,
            data,
            balance: 0,
            nonce: Nonce(0),
        },
    )
}

fn request(amount: Value) -> RepayDebtPlanRequest {
    let owner = id(20);
    let position_id = compute_position_pda(PROGRAM, owner, NONCE);
    let definition_id = compute_stablecoin_definition_pda(PROGRAM);
    let position = Position {
        owner_account_id: owner,
        position_nonce: NONCE,
        vault_account_id: compute_position_vault_pda(PROGRAM, position_id),
        collateral_amount: 1_000,
        normalized_debt_amount: 100,
        opened_at: 7,
    };
    let parameters = ProtocolParameters {
        admin_account_id: id(1),
        freeze_authority_account_id: id(2),
        stablecoin_definition_id: definition_id,
        collateral_definition_id: id(4),
        market_price_oracle_id: id(5),
        stability_fee_per_millisecond: FIXED_POINT_ONE,
        controller_proportional_gain: 0,
        controller_integral_gain: 0,
        minimum_collateralization_ratio: FIXED_POINT_ONE,
        minimum_milliseconds_between_rate_updates: 50,
        maximum_oracle_price_age_milliseconds: 50,
        is_frozen: true,
    };
    RepayDebtPlanRequest {
        stablecoin_program_id: hex::encode(program_id_bytes(PROGRAM)),
        owner_id: account_id_hex(owner),
        position_nonce: NONCE.to_string(),
        amount,
        user_stablecoin_holding_id: account_id_hex(id(21)),
        position: read(position_id, PROGRAM, Data::from(&position)),
        stablecoin_definition: read(
            definition_id,
            TOKEN_PROGRAM,
            Data::from(&TokenDefinition::Fungible {
                name: String::from("Stablecoin"),
                total_supply: u128::MAX,
                metadata_id: None,
                authority: None,
            }),
        ),
        user_stablecoin_holding: read(
            id(21),
            TOKEN_PROGRAM,
            Data::from(&TokenHolding::Fungible {
                definition_id,
                balance: u128::MAX,
            }),
        ),
        stability_fee_accumulator: read(
            compute_stability_fee_accumulator_pda(PROGRAM),
            PROGRAM,
            Data::from(&StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: FIXED_POINT_ONE * 3 / 2,
                last_accrued_at: NOW,
            }),
        ),
        protocol_parameters: read(
            compute_protocol_parameters_pda(PROGRAM),
            PROGRAM,
            Data::from(&parameters),
        ),
        clock: read(
            CLOCK_01_PROGRAM_ACCOUNT_ID,
            [0x44; 8],
            Data::try_from(
                ClockAccountData {
                    block_id: 1,
                    timestamp: NOW,
                }
                .to_bytes(),
            )
            .expect("clock fits"),
        ),
    }
}

fn replace_data(read: &mut AccountRead, data: Data) {
    read.account.as_mut().expect("fixture account exists").data = hex::encode(data.as_ref());
}

fn position(request: &RepayDebtPlanRequest) -> Position {
    Position::try_from(
        &decode_account(&request.position)
            .expect("valid fixture")
            .1
            .data,
    )
    .expect("valid position")
}

fn error(request: RepayDebtPlanRequest, expected: &str) {
    assert_eq!(
        repay_debt_plan(request)
            .expect_err("preflight must fail")
            .code(),
        expected
    );
}

#[test]
fn repay_plan_pins_instruction_pdas_signers_and_guest_idl() {
    let request = request(json!("151"));
    let plan = repay_debt_plan(request.clone()).expect("floor delta is 100, so 151 is allowed");
    assert_eq!(plan["programId"], request.stablecoin_program_id);
    assert_eq!(
        plan["accountIds"],
        json!([
            request.owner_id,
            request.position.id,
            request.stablecoin_definition.id,
            request.user_stablecoin_holding.id,
            request.stability_fee_accumulator.id,
            request.protocol_parameters.id,
            request.clock.id,
        ])
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([true, false, false, true, false, false, false])
    );
    let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
    let instruction: Instruction = risc0_zkvm::serde::from_slice(&words).expect("instruction");
    assert!(matches!(
        instruction,
        Instruction::RepayDebt { amount: 151 }
    ));

    let idl: Value =
        serde_json::from_str(include_str!("../../../../../artifacts/stablecoin-idl.json"))
            .expect("valid IDL");
    let instruction = idl["instructions"]
        .as_array()
        .expect("instructions")
        .iter()
        .find(|entry| entry["name"] == "repay_debt")
        .expect("repay instruction");
    let flags: Vec<_> = instruction["accounts"]
        .as_array()
        .expect("accounts")
        .iter()
        .map(|entry| {
            (
                entry["name"].clone(),
                entry["writable"].clone(),
                entry["signer"].clone(),
                entry["init"].clone(),
            )
        })
        .collect();
    assert_eq!(
        flags,
        vec![
            (json!("owner"), json!(false), json!(true), json!(false)),
            (json!("position"), json!(true), json!(false), json!(false)),
            (
                json!("stablecoin_definition"),
                json!(true),
                json!(false),
                json!(false)
            ),
            (
                json!("user_stablecoin_holding"),
                json!(true),
                json!(true),
                json!(false)
            ),
            (
                json!("stability_fee_accumulator"),
                json!(false),
                json!(false),
                json!(false)
            ),
            (
                json!("protocol_parameters"),
                json!(false),
                json!(false),
                json!(false)
            ),
            (json!("clock"), json!(false), json!(false), json!(false)),
        ]
    );
    assert_eq!(
        instruction["args"],
        json!([{"name":"amount", "type":"u128"}])
    );
}

#[test]
fn repay_plan_executes_exact_burn_and_floor_debt_change_while_frozen() {
    for (amount, remaining) in [(0, 100), (1, 100), (100, 34), (150, 0), (151, 0)] {
        let request = request(json!(amount.to_string()));
        let plan = repay_debt_plan(request.clone()).expect("valid repayment");
        let reads = [
            &request.position,
            &request.stablecoin_definition,
            &request.user_stablecoin_holding,
            &request.stability_fee_accumulator,
            &request.protocol_parameters,
            &request.clock,
        ];
        let account_ids: Vec<String> =
            serde_json::from_value(plan["accountIds"].clone()).expect("ids");
        let signers: Vec<bool> =
            serde_json::from_value(plan["signingRequirements"].clone()).expect("signers");
        let inputs: Vec<_> = account_ids
            .iter()
            .zip(signers)
            .map(|(account_id, signer)| {
                let (account_id, account) = if account_id == &request.owner_id {
                    (id(20), Account::default())
                } else {
                    decode_account(
                        reads
                            .iter()
                            .find(|read| &read.id == account_id)
                            .expect("planned account read"),
                    )
                    .expect("valid account")
                };
                AccountWithMetadata::new(account, signer, account_id)
            })
            .collect();
        let [owner, position_input, definition, holding, accumulator, parameters, clock]: [_; 7] =
            inputs.try_into().expect("seven accounts");
        let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
        let Instruction::RepayDebt { amount } =
            risc0_zkvm::serde::from_slice(&words).expect("instruction")
        else {
            panic!("expected repayment");
        };
        let (posts, calls) = stablecoin_program::repay_debt::repay_debt(
            owner,
            position_input,
            definition,
            holding,
            accumulator,
            parameters,
            clock,
            PROGRAM,
            amount,
        );
        let updated = Position::try_from(&posts.get(1).expect("position post").account().data)
            .expect("position");
        let mut expected = position(&request);
        expected.normalized_debt_amount = remaining;
        assert_eq!(updated, expected);
        let [call] = calls.as_slice() else {
            panic!("one chained burn");
        };
        assert_eq!(call.program_id, TOKEN_PROGRAM);
        let token_core::Instruction::Burn { amount_to_burn } =
            risc0_zkvm::serde::from_slice(&call.instruction_data).expect("burn instruction")
        else {
            panic!("expected burn");
        };
        assert_eq!(amount_to_burn, amount);
        let [definition, holding] = call.pre_states.as_slice() else {
            panic!("two burn accounts");
        };
        assert_eq!(
            account_id_hex(definition.account_id),
            request.stablecoin_definition.id
        );
        assert_eq!(
            account_id_hex(holding.account_id),
            request.user_stablecoin_holding.id
        );
        assert!(holding.is_authorized);
        let burned = token_program::burn::burn(definition.clone(), holding.clone(), amount_to_burn);
        let holding = TokenHolding::try_from(&burned.get(1).expect("holding post").account().data)
            .expect("holding");
        assert_eq!(
            holding,
            TokenHolding::Fungible {
                definition_id: compute_stablecoin_definition_pda(PROGRAM),
                balance: u128::MAX - amount,
            }
        );
        let definition =
            TokenDefinition::try_from(&burned.first().expect("definition post").account().data)
                .expect("definition");
        assert!(
            matches!(definition, TokenDefinition::Fungible { total_supply, .. } if total_supply == u128::MAX - amount)
        );
    }
    error(request(json!("152")), "repay_amount_exceeds_debt");
}

#[test]
fn repay_plan_preserves_full_width_integers_and_rejects_lossy_values() {
    for amount in [json!(u64::MAX), json!(u128::MAX.to_string())] {
        let mut request = request(amount.clone());
        let mut state = position(&request);
        state.normalized_debt_amount = u128::MAX;
        replace_data(&mut request.position, Data::from(&state));
        replace_data(
            &mut request.stability_fee_accumulator,
            Data::from(&StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
                last_accrued_at: NOW,
            }),
        );
        let plan = repay_debt_plan(request).expect("full width amount");
        let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
        let expected_amount = if let Some(number) = amount.as_u64() {
            u128::from(number)
        } else {
            amount.as_str().expect("decimal").parse().expect("u128")
        };
        let instruction =
            risc0_zkvm::serde::from_slice::<Instruction, u32>(&words).expect("instruction");
        assert!(
            matches!(instruction, Instruction::RepayDebt { amount } if amount == expected_amount)
        );
    }
    for value in [
        json!(1.5),
        json!(1.0),
        json!(-1),
        json!("-1"),
        json!("1e3"),
        json!(""),
        json!("340282366920938463463374607431768211456"),
        Value::Null,
    ] {
        error(request(value), "invalid_numeric_value");
    }
    for nonce in ["18446744073709551616", "-1", "1.0", ""] {
        let mut invalid = request(json!("0"));
        invalid.position_nonce = nonce.to_owned();
        error(invalid, "invalid_numeric_value");
    }
}

#[test]
fn repay_plan_projects_current_fees_and_reports_unrepresentable_arithmetic() {
    let mut request = request(json!("1500000000002"));
    let mut state = position(&request);
    state.normalized_debt_amount = 1_000_000_000_000;
    replace_data(&mut request.position, Data::from(&state));
    error(request.clone(), "repay_amount_exceeds_debt");
    // One millisecond at a permitted fee rate moves this large amount across
    // the overpayment boundary. Using the stored anchor would reject it.
    let mut parameters = ProtocolParameters::try_from(
        &decode_account(&request.protocol_parameters)
            .expect("account")
            .1
            .data,
    )
    .expect("parameters");
    parameters.stability_fee_per_millisecond = FIXED_POINT_ONE + 1_500_000_000_000_000;
    replace_data(&mut request.protocol_parameters, Data::from(&parameters));
    replace_data(
        &mut request.clock,
        Data::try_from(
            ClockAccountData {
                block_id: 1,
                timestamp: NOW + 1,
            }
            .to_bytes(),
        )
        .expect("clock"),
    );
    repay_debt_plan(request.clone()).expect("projected accumulator determines the boundary");

    // The upper representable anchor can arise from accumulated fees. Even a
    // rate just above one makes its next narrow projection unrepresentable.
    parameters.stability_fee_per_millisecond = FIXED_POINT_ONE + 1;
    replace_data(&mut request.protocol_parameters, Data::from(&parameters));
    replace_data(
        &mut request.stability_fee_accumulator,
        Data::from(&StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: u128::MAX,
            last_accrued_at: NOW,
        }),
    );
    error(request, "repayment_arithmetic_error");
}

#[test]
fn repay_plan_validates_position_token_bindings_and_balance_even_at_zero() {
    let base = request(json!("0"));
    let mut invalid = base.clone();
    invalid.position.id = account_id_hex(id(90));
    error(invalid, "position_pda_mismatch");
    let mut invalid = base.clone();
    invalid
        .position
        .account
        .as_mut()
        .expect("account")
        .program_owner = hex::encode(program_id_bytes(TOKEN_PROGRAM));
    error(invalid, "stablecoin_program_mismatch");
    let mut invalid = base.clone();
    let mut state = position(&invalid);
    state.owner_account_id = id(90);
    replace_data(&mut invalid.position, Data::from(&state));
    error(invalid, "position_owner_mismatch");
    let mut invalid = base.clone();
    let mut state = position(&invalid);
    state.position_nonce = 1;
    replace_data(&mut invalid.position, Data::from(&state));
    error(invalid, "position_nonce_mismatch");
    let mut invalid = base.clone();
    invalid.stablecoin_definition.id = account_id_hex(id(90));
    error(invalid, "stablecoin_definition_mismatch");
    let mut invalid = base.clone();
    invalid.user_stablecoin_holding.id = account_id_hex(id(90));
    error(invalid, "invalid_user_stablecoin_holding");
    let mut invalid = base.clone();
    invalid
        .user_stablecoin_holding
        .account
        .as_mut()
        .expect("account")
        .program_owner = hex::encode(program_id_bytes(PROGRAM));
    error(invalid, "token_program_mismatch");
    let mut invalid = base.clone();
    replace_data(
        &mut invalid.user_stablecoin_holding,
        Data::from(&TokenHolding::Fungible {
            definition_id: id(90),
            balance: 500,
        }),
    );
    error(invalid, "stablecoin_definition_mismatch");
    let mut invalid = request(json!("1"));
    replace_data(
        &mut invalid.user_stablecoin_holding,
        Data::from(&TokenHolding::Fungible {
            definition_id: compute_stablecoin_definition_pda(PROGRAM),
            balance: 0,
        }),
    );
    error(invalid, "insufficient_stablecoin_balance");
}

#[test]
fn repay_plan_rejects_each_malformed_or_missing_account_and_global_identity() {
    for index in 0..6 {
        for missing in [false, true] {
            let mut request = request(json!("0"));
            let reads = [
                &mut request.position,
                &mut request.stablecoin_definition,
                &mut request.user_stablecoin_holding,
                &mut request.stability_fee_accumulator,
                &mut request.protocol_parameters,
                &mut request.clock,
            ];
            let read = reads.into_iter().nth(index).expect("selected read");
            if missing {
                read.status = String::from("not_found");
                read.account = None;
            } else {
                read.account.as_mut().expect("account").data.push_str("ff");
            }
            let expected = if missing {
                "account_read_failed"
            } else {
                [
                    "invalid_position_data",
                    "invalid_stablecoin_definition",
                    "invalid_user_stablecoin_holding",
                    "invalid_stability_fee_accumulator_data",
                    "invalid_protocol_parameters_data",
                    "invalid_clock",
                ]
                .get(index)
                .expect("error code")
            };
            error(request, expected);
        }
    }
    let mut request = request(json!("0"));
    request.protocol_parameters.id = account_id_hex(id(90));
    error(request, "protocol_parameters_pda_mismatch");
    let mut request = self::request(json!("0"));
    request.stability_fee_accumulator.id = account_id_hex(id(90));
    error(request, "stability_fee_accumulator_pda_mismatch");
    let mut request = self::request(json!("0"));
    request.clock.id = account_id_hex(id(90));
    error(request, "invalid_clock");
}
