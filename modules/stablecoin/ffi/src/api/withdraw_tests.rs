use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use stablecoin_core::{
    compute_position_pda, compute_position_vault_pda, compute_position_vault_pda_seed,
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    math::FIXED_POINT_ONE, Instruction, Position, ProtocolParameters, RedemptionPriceState,
    StabilityFeeAccumulator, RATE_DELTA_CLAMP,
};
use token_core::TokenHolding;

use super::{withdraw_collateral_plan, WithdrawCollateralPlanRequest};
use crate::{
    account::{account_id_hex, account_read, decode_account, program_id_bytes},
    AccountRead,
};

const PROGRAM: ProgramId = [0x11; 8];
const TOKEN_PROGRAM: ProgramId = [0x22; 8];
const START: u64 = 1_000;
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

struct Fixture {
    position: Position,
    parameters: ProtocolParameters,
    accumulator: StabilityFeeAccumulator,
    redemption: RedemptionPriceState,
    vault_balance: u128,
    destination_balance: u128,
    now: u64,
}
impl Fixture {
    fn new(collateral: u128, debt: u128) -> Self {
        let position_id = compute_position_pda(PROGRAM, id(20), NONCE);
        Self {
            position: Position {
                owner_account_id: id(20),
                position_nonce: NONCE,
                vault_account_id: compute_position_vault_pda(PROGRAM, position_id),
                collateral_amount: collateral,
                normalized_debt_amount: debt,
                opened_at: START,
            },
            parameters: ProtocolParameters {
                admin_account_id: id(1),
                freeze_authority_account_id: id(2),
                stablecoin_definition_id: compute_stablecoin_definition_pda(PROGRAM),
                collateral_definition_id: id(4),
                market_price_oracle_id: id(5),
                stability_fee_per_millisecond: FIXED_POINT_ONE,
                controller_proportional_gain: 0,
                controller_integral_gain: 0,
                minimum_collateralization_ratio: FIXED_POINT_ONE * 11 / 10,
                minimum_milliseconds_between_rate_updates: 50,
                maximum_oracle_price_age_milliseconds: 50,
                is_frozen: false,
            },
            accumulator: StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
                last_accrued_at: START,
            },
            redemption: RedemptionPriceState {
                redemption_price_at_last_update: FIXED_POINT_ONE,
                redemption_rate_per_millisecond: FIXED_POINT_ONE,
                controller_integral_term: 0,
                last_updated_at: START,
            },
            vault_balance: collateral,
            destination_balance: 0,
            now: START,
        }
    }

    fn request(&self, amount: Value) -> WithdrawCollateralPlanRequest {
        WithdrawCollateralPlanRequest {
            stablecoin_program_id: hex::encode(program_id_bytes(PROGRAM)),
            owner_id: account_id_hex(id(20)),
            position_nonce: NONCE.to_string(),
            amount,
            user_collateral_holding_id: account_id_hex(id(21)),
            position: read(
                compute_position_pda(PROGRAM, id(20), NONCE),
                PROGRAM,
                Data::from(&self.position),
            ),
            vault: read(
                self.position.vault_account_id,
                TOKEN_PROGRAM,
                Data::from(&TokenHolding::Fungible {
                    definition_id: id(4),
                    balance: self.vault_balance,
                }),
            ),
            user_collateral_holding: read(
                id(21),
                TOKEN_PROGRAM,
                Data::from(&TokenHolding::Fungible {
                    definition_id: id(4),
                    balance: self.destination_balance,
                }),
            ),
            stability_fee_accumulator: read(
                compute_stability_fee_accumulator_pda(PROGRAM),
                PROGRAM,
                Data::from(&self.accumulator),
            ),
            redemption_price_state: read(
                compute_redemption_price_state_pda(PROGRAM),
                PROGRAM,
                Data::from(&self.redemption),
            ),
            protocol_parameters: read(
                compute_protocol_parameters_pda(PROGRAM),
                PROGRAM,
                Data::from(&self.parameters),
            ),
            clock: read(
                CLOCK_01_PROGRAM_ACCOUNT_ID,
                [0x44; 8],
                Data::try_from(
                    ClockAccountData {
                        block_id: 1,
                        timestamp: self.now,
                    }
                    .to_bytes(),
                )
                .expect("clock fits"),
            ),
        }
    }
}

fn error(request: WithdrawCollateralPlanRequest, expected: &str) {
    assert_eq!(
        withdraw_collateral_plan(request)
            .expect_err("preflight must reject")
            .code(),
        expected
    );
}

fn execute(fixture: &Fixture, amount: u128) {
    let request = fixture.request(json!(amount.to_string()));
    let plan = withdraw_collateral_plan(request.clone()).expect("accepted plan");
    let reads = [
        &request.position,
        &request.vault,
        &request.user_collateral_holding,
        &request.stability_fee_accumulator,
        &request.redemption_price_state,
        &request.protocol_parameters,
        &request.clock,
    ];
    let ids: Vec<String> = serde_json::from_value(plan["accountIds"].clone()).expect("account ids");
    let signers: Vec<bool> =
        serde_json::from_value(plan["signingRequirements"].clone()).expect("signers");
    let inputs: Vec<_> = ids
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
                        .expect("planned read"),
                )
                .expect("account")
            };
            AccountWithMetadata::new(account, signer, account_id)
        })
        .collect();
    let [owner, position, vault, destination, accumulator, redemption, parameters, clock]: [_; 8] =
        inputs.try_into().expect("eight inputs");
    assert!(!vault.is_authorized);
    assert!(!destination.is_authorized);
    let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
    let Instruction::WithdrawCollateral { amount } =
        risc0_zkvm::serde::from_slice(&words).expect("instruction")
    else {
        panic!("expected withdrawal");
    };
    let (posts, calls) = stablecoin_program::withdraw_collateral::withdraw_collateral(
        owner,
        position,
        vault,
        destination,
        accumulator,
        redemption,
        parameters,
        clock,
        PROGRAM,
        amount,
    );
    let actual =
        Position::try_from(&posts.get(1).expect("position post").account().data).expect("position");
    let mut expected = fixture.position.clone();
    expected.collateral_amount -= amount;
    assert_eq!(actual, expected);
    let [call] = calls.as_slice() else {
        panic!("one chained transfer");
    };
    assert_eq!(call.program_id, TOKEN_PROGRAM);
    assert_eq!(
        call.pda_seeds,
        vec![compute_position_vault_pda_seed(compute_position_pda(
            PROGRAM,
            id(20),
            NONCE
        ))]
    );
    let token_core::Instruction::Transfer { amount_to_transfer } =
        risc0_zkvm::serde::from_slice(&call.instruction_data).expect("token instruction")
    else {
        panic!("expected transfer");
    };
    assert_eq!(amount_to_transfer, amount);
    let [vault, destination] = call.pre_states.as_slice() else {
        panic!("two transfer inputs");
    };
    assert!(vault.is_authorized);
    assert!(!destination.is_authorized);
    assert_eq!(vault.account_id, fixture.position.vault_account_id);
    assert_eq!(destination.account_id, id(21));
    let transferred =
        token_program::transfer::transfer(vault.clone(), destination.clone(), amount_to_transfer);
    assert_eq!(
        TokenHolding::try_from(&transferred.first().expect("vault post").account().data)
            .expect("vault"),
        TokenHolding::Fungible {
            definition_id: id(4),
            balance: fixture.vault_balance - amount
        }
    );
    assert_eq!(
        TokenHolding::try_from(&transferred.get(1).expect("destination post").account().data)
            .expect("destination"),
        TokenHolding::Fungible {
            definition_id: id(4),
            balance: fixture.destination_balance + amount
        }
    );
}

#[test]
fn withdrawal_pins_serialization_pdas_guest_flags_and_owner_only_signing() {
    let fixture = Fixture::new(130, 100);
    let request = fixture.request(json!("20"));
    let plan = withdraw_collateral_plan(request.clone()).expect("equality is healthy");
    assert_eq!(plan["programId"], request.stablecoin_program_id);
    assert_eq!(
        plan["accountIds"],
        json!([
            request.owner_id,
            request.position.id,
            request.vault.id,
            request.user_collateral_holding.id,
            request.stability_fee_accumulator.id,
            request.redemption_price_state.id,
            request.protocol_parameters.id,
            request.clock.id
        ])
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([true, false, false, false, false, false, false, false])
    );
    let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
    assert!(matches!(
        risc0_zkvm::serde::from_slice::<Instruction, u32>(&words).expect("instruction"),
        Instruction::WithdrawCollateral { amount: 20 }
    ));
    let idl: Value =
        serde_json::from_str(include_str!("../../../../../artifacts/stablecoin-idl.json"))
            .expect("IDL");
    let instruction = idl["instructions"]
        .as_array()
        .expect("instructions")
        .iter()
        .find(|entry| entry["name"] == "withdraw_collateral")
        .expect("withdraw entry");
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
            (json!("vault"), json!(true), json!(false), json!(false)),
            (
                json!("user_collateral_holding"),
                json!(true),
                json!(false),
                json!(false)
            ),
            (
                json!("stability_fee_accumulator"),
                json!(false),
                json!(false),
                json!(false)
            ),
            (
                json!("redemption_price_state"),
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
fn accepted_plan_executes_pda_transfer_and_keeps_debt_unchanged() {
    let mut fixture = Fixture::new(130, 100);
    fixture.vault_balance = 140; // Donation is not spendable until Position reconciliation.
    fixture.destination_balance = 5;
    execute(&fixture, 20);
    error(fixture.request(json!("21")), "position_undercollateralized");
    error(
        fixture.request(json!("131")),
        "withdraw_amount_exceeds_collateral",
    );
}

#[test]
fn withdrawal_does_not_floor_fractional_debt_or_narrow_wide_projections() {
    let mut fractional = Fixture::new(3, 1);
    fractional.accumulator.accumulated_rate_at_last_accrual = FIXED_POINT_ONE * 19 / 10;
    error(
        fractional.request(json!("1")),
        "position_undercollateralized",
    );
    execute(&fractional, 0);
    let mut wide = Fixture::new(1_000_000_000_000, 1);
    wide.redemption.redemption_rate_per_millisecond =
        FIXED_POINT_ONE + RATE_DELTA_CLAMP.unsigned_abs();
    wide.now = START + 2_700_000;
    execute(&wide, 100_000_000_000);
    let mut saturated = Fixture::new(u128::MAX, 10);
    saturated.redemption.redemption_rate_per_millisecond =
        FIXED_POINT_ONE + RATE_DELTA_CLAMP.unsigned_abs();
    saturated.now = START + 18_000_000;
    error(
        saturated.request(json!("1")),
        "position_undercollateralized",
    );
}

#[test]
fn zero_and_zero_debt_skip_projections_but_not_frozen_or_structural_guards() {
    let mut zero = Fixture::new(1, 100);
    zero.now = START + 7_000_000;
    zero.redemption.redemption_rate_per_millisecond =
        FIXED_POINT_ONE - RATE_DELTA_CLAMP.unsigned_abs();
    execute(&zero, 0);
    error(zero.request(json!("1")), "redemption_price_zero");
    zero.parameters.is_frozen = true;
    error(zero.request(json!("0")), "protocol_frozen");
    let mut settled = Fixture::new(50, 0);
    settled.parameters.stability_fee_per_millisecond = FIXED_POINT_ONE * 2;
    settled.redemption.redemption_rate_per_millisecond =
        FIXED_POINT_ONE + RATE_DELTA_CLAMP.unsigned_abs();
    settled.now = START + 60_000_000;
    execute(&settled, 50);
    let mut invalid = settled.request(json!("0"));
    invalid.clock.id = account_id_hex(id(90));
    error(invalid, "invalid_clock");
}

#[test]
fn withdrawal_preserves_maximum_amounts_lossless_json_integers_and_base58_ids() {
    let fixture = Fixture::new(u128::MAX, 0);
    execute(&fixture, u128::MAX);
    let mut request = fixture.request(json!(u64::MAX));
    request.owner_id = id(20).to_string();
    request.user_collateral_holding_id = id(21).to_string();
    let plan = withdraw_collateral_plan(request).expect("lossless integer and base58");
    let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
    assert!(
        matches!(risc0_zkvm::serde::from_slice::<Instruction,u32>(&words).expect("instruction"),
        Instruction::WithdrawCollateral { amount } if amount == u128::from(u64::MAX))
    );
    for amount in [
        json!(1.5),
        json!(1.0),
        json!(-1),
        json!("-1"),
        json!(""),
        json!("1e3"),
        json!("340282366920938463463374607431768211456"),
        Value::Null,
    ] {
        error(fixture.request(amount), "invalid_numeric_value");
    }
    for nonce in ["", "-1", "1.0", "18446744073709551616"] {
        let mut invalid = fixture.request(json!("0"));
        invalid.position_nonce = nonce.to_owned();
        error(invalid, "invalid_numeric_value");
    }
}

#[test]
fn withdrawal_rejects_inconsistent_live_balance_snapshots_before_submission() {
    // Each observation can be valid at its read height: read Position/vault
    // before another withdrawal, then destination after that withdrawal. These
    // cases deliberately violate a single-snapshot balance invariant to model
    // the live adapter's independent RPC reads, not an impossible ledger state.
    let mut stale_vault = Fixture::new(1, 0);
    stale_vault.vault_balance = 0;
    stale_vault.destination_balance = 1;
    error(
        stale_vault.request(json!("1")),
        "insufficient_vault_balance",
    );
    let mut stale_destination = Fixture::new(1, 0);
    stale_destination.destination_balance = u128::MAX;
    error(
        stale_destination.request(json!("1")),
        "collateral_amount_overflow",
    );
}

#[test]
fn withdrawal_rejects_wrong_position_vault_and_destination_bindings() {
    let fixture = Fixture::new(130, 100);
    for alias in [id(20), fixture.position.vault_account_id] {
        let mut invalid = fixture.request(json!("0"));
        invalid.user_collateral_holding_id = account_id_hex(alias);
        invalid.user_collateral_holding.id = account_id_hex(alias);
        error(invalid, "invalid_user_collateral_holding");
    }
    for (state, expected) in [
        (
            Position {
                owner_account_id: id(90),
                ..fixture.position.clone()
            },
            "position_owner_mismatch",
        ),
        (
            Position {
                position_nonce: 1,
                ..fixture.position.clone()
            },
            "position_nonce_mismatch",
        ),
        (
            Position {
                vault_account_id: id(90),
                ..fixture.position.clone()
            },
            "position_vault_mismatch",
        ),
    ] {
        let mut invalid = fixture.request(json!("0"));
        invalid.position.account.as_mut().expect("account").data =
            hex::encode(Data::from(&state).as_ref());
        error(invalid, expected);
    }
    let mut invalid = fixture.request(json!("0"));
    invalid.user_collateral_holding.id = account_id_hex(id(90));
    error(invalid, "invalid_user_collateral_holding");
    let mut invalid = fixture.request(json!("0"));
    invalid
        .user_collateral_holding
        .account
        .as_mut()
        .expect("account")
        .program_owner = hex::encode(program_id_bytes(PROGRAM));
    error(invalid, "token_program_mismatch");
    for vault in [false, true] {
        let mut invalid = fixture.request(json!("0"));
        let read = if vault {
            &mut invalid.vault
        } else {
            &mut invalid.user_collateral_holding
        };
        read.account.as_mut().expect("account").data = hex::encode(
            Data::from(&TokenHolding::Fungible {
                definition_id: id(90),
                balance: 1,
            })
            .as_ref(),
        );
        error(invalid, "collateral_definition_mismatch");
    }
}

#[test]
fn withdrawal_validates_all_required_reads_even_without_health_projections() {
    let fixture = Fixture::new(130, 0);
    let data_errors = [
        "invalid_position_data",
        "invalid_position_vault",
        "invalid_user_collateral_holding",
        "invalid_stability_fee_accumulator_data",
        "invalid_redemption_price_state_data",
        "invalid_protocol_parameters_data",
        "invalid_clock",
    ];
    let pda_errors = [
        "position_pda_mismatch",
        "vault_pda_mismatch",
        "invalid_user_collateral_holding",
        "stability_fee_accumulator_pda_mismatch",
        "redemption_price_state_pda_mismatch",
        "protocol_parameters_pda_mismatch",
        "invalid_clock",
    ];
    for index in 0..7 {
        for mutation in 0..4 {
            let mut request = fixture.request(json!("0"));
            let reads = [
                &mut request.position,
                &mut request.vault,
                &mut request.user_collateral_holding,
                &mut request.stability_fee_accumulator,
                &mut request.redemption_price_state,
                &mut request.protocol_parameters,
                &mut request.clock,
            ];
            let read = reads.into_iter().nth(index).expect("read");
            let expected = match mutation {
                0 => {
                    read.status = String::from("not_found");
                    read.account = None;
                    "account_read_failed"
                }
                1 => {
                    let data = &mut read.account.as_mut().expect("account").data;
                    data.truncate(data.len() - 2);
                    data_errors[index]
                }
                2 => {
                    read.account.as_mut().expect("account").data.push_str("ff");
                    data_errors[index]
                }
                _ => {
                    read.id = account_id_hex(id(90));
                    pda_errors[index]
                }
            };
            error(request, expected);
        }
    }
}
