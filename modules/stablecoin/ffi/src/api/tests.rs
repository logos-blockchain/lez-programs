use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, Data, Nonce},
    program::{ProgramHeader, PROGRAM_LOADER_ACCOUNT_ID},
};
use risc0_binfmt::ProgramBinary;
use serde_json::{json, Value};
use stablecoin_core::{
    compute_position_pda, compute_position_vault_pda, compute_protocol_parameters_pda,
    compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
    compute_stablecoin_definition_pda, compute_stablecoin_master_holding_pda,
    math::FIXED_POINT_ONE, Instruction, Position, ProtocolParameters, RedemptionPriceState,
    StabilityFeeAccumulator,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

use super::{
    accrue_stability_fee_plan, decode_position, decode_protocol_parameters,
    decode_redemption_price_state, decode_stability_fee_accumulator, deposit_collateral_plan,
    initialize_program_plan, open_position_plan, position_addresses, position_info, program_info,
    refresh_globals_plan, update_redemption_rate_plan, AccrueStabilityFeePlanRequest,
    DecodePositionRequest, DecodeProtocolParametersRequest, DecodeRedemptionPriceStateRequest,
    DecodeStabilityFeeAccumulatorRequest, DepositCollateralPlanRequest,
    InitializeProgramPlanRequest, OpenPositionPlanRequest, PositionAddressesRequest,
    PositionInfoRequest, ProgramInfoRequest, RefreshGlobalsPlanRequest, StablecoinResult,
    UpdateRedemptionRatePlanRequest,
};
use crate::account::{account_id_hex, account_read, AccountRead};

const STABLECOIN_PROGRAM_ID: AccountId = AccountId::new([0x11_u8; 32]);
const TOKEN_PROGRAM_ID: AccountId = AccountId::new([0x22_u8; 32]);
const ORACLE_PROGRAM_ID: AccountId = AccountId::new([0x33_u8; 32]);
const CLOCK_PROGRAM_ID: AccountId = AccountId::new([0x44_u8; 32]);

fn account(owner: AccountId, data: Data) -> Account {
    Account {
        program_owner: owner,
        balance: 0,
        data,
        nonce: Nonce(0),
    }
}

fn id(seed: u8) -> AccountId {
    AccountId::new([seed; 32])
}

fn program_id_hex() -> String {
    account_id_hex(STABLECOIN_PROGRAM_ID)
}

fn deployable_program_binary() -> (String, AccountId) {
    let encoded = stablecoin_methods::STABLECOIN_ELF;
    let binary = ok(ProgramBinary::decode(encoded));
    let image_id: lee_core::program::ProgramId = ok(binary.compute_image_id()).into();
    let image_id = AccountId::from(image_id);
    (hex::encode(encoded), image_id)
}

fn ok<T, E: core::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{error}"),
    }
}

fn assert_error(result: StablecoinResult, expected: &str) {
    match result {
        Ok(value) => panic!("expected {expected}, got {value}"),
        Err(error) => assert_eq!(error.code(), expected),
    }
}

fn protocol_parameters() -> ProtocolParameters {
    ProtocolParameters {
        admin_account_id: id(1),
        freeze_authority_account_id: id(2),
        stablecoin_definition_id: id(3),
        collateral_definition_id: id(4),
        market_price_oracle_id: id(5),
        stability_fee_per_millisecond: u128::MAX,
        controller_proportional_gain: i128::MIN,
        controller_integral_gain: i128::MAX,
        minimum_collateralization_ratio: u128::MAX - 1,
        minimum_milliseconds_between_rate_updates: u64::MAX,
        maximum_oracle_price_age_milliseconds: u64::MAX - 1,
        is_frozen: true,
    }
}

fn protocol_request(parameters: &ProtocolParameters) -> DecodeProtocolParametersRequest {
    let account_id = compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID);
    DecodeProtocolParametersRequest {
        stablecoin_program_id: program_id_hex(),
        protocol_parameters: account_read(
            account_id,
            &account(STABLECOIN_PROGRAM_ID, Data::from(parameters)),
        ),
    }
}

fn stability_fee_accumulator() -> StabilityFeeAccumulator {
    StabilityFeeAccumulator {
        accumulated_rate_at_last_accrual: u128::MAX,
        last_accrued_at: u64::MAX,
    }
}

fn accumulator_request(
    accumulator: &StabilityFeeAccumulator,
) -> DecodeStabilityFeeAccumulatorRequest {
    let account_id = compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID);
    DecodeStabilityFeeAccumulatorRequest {
        stablecoin_program_id: program_id_hex(),
        stability_fee_accumulator: account_read(
            account_id,
            &account(STABLECOIN_PROGRAM_ID, Data::from(accumulator)),
        ),
    }
}

fn position_info_request(owner_id: AccountId, position_nonce: &str) -> PositionInfoRequest {
    PositionInfoRequest {
        stablecoin_program_id: program_id_hex(),
        owner_id: account_id_hex(owner_id),
        position_nonce: String::from(position_nonce),
    }
}

fn position(owner_id: AccountId, position_nonce: u64) -> Position {
    let position_id = compute_position_pda(STABLECOIN_PROGRAM_ID, owner_id, position_nonce);
    Position {
        owner_account_id: owner_id,
        position_nonce,
        vault_account_id: compute_position_vault_pda(STABLECOIN_PROGRAM_ID, position_id),
        collateral_amount: u128::MAX,
        normalized_debt_amount: u128::MAX,
        opened_at: u64::MAX,
    }
}

fn position_decode_request(
    owner_id: AccountId,
    position_nonce: u64,
    stored_position: &Position,
) -> DecodePositionRequest {
    let position_id = compute_position_pda(STABLECOIN_PROGRAM_ID, owner_id, position_nonce);
    DecodePositionRequest {
        stablecoin_program_id: program_id_hex(),
        owner_id: account_id_hex(owner_id),
        position_nonce: position_nonce.to_string(),
        position: account_read(
            position_id,
            &account(STABLECOIN_PROGRAM_ID, Data::from(stored_position)),
        ),
    }
}

fn redemption_price_state(controller_integral_term: i128) -> RedemptionPriceState {
    RedemptionPriceState {
        redemption_price_at_last_update: u128::MAX,
        redemption_rate_per_millisecond: u128::MAX,
        controller_integral_term,
        last_updated_at: u64::MAX,
    }
}

fn redemption_price_state_request(
    state: &RedemptionPriceState,
) -> DecodeRedemptionPriceStateRequest {
    let account_id = compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID);
    DecodeRedemptionPriceStateRequest {
        stablecoin_program_id: program_id_hex(),
        redemption_price_state: account_read(
            account_id,
            &account(STABLECOIN_PROGRAM_ID, Data::from(state)),
        ),
    }
}

fn initialize_request() -> InitializeProgramPlanRequest {
    let collateral_id = id(10);
    let stablecoin_definition_id = compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID);
    let collateral_definition = TokenDefinition::Fungible {
        name: String::from("Collateral"),
        total_supply: u128::MAX,
        metadata_id: None,
        authority: None,
    };
    let oracle_id = id(11);
    let oracle = OraclePriceAccount {
        base_asset: stablecoin_definition_id,
        quote_asset: collateral_id,
        price: 1,
        timestamp: 2,
        source_id: id(12),
        confidence_interval: 0,
    };
    let clock = ClockAccountData {
        block_id: 3,
        timestamp: 4,
    };

    InitializeProgramPlanRequest {
        stablecoin_program_id: program_id_hex(),
        admin_id: account_id_hex(id(13)),
        freeze_authority_id: account_id_hex(id(14)),
        collateral_definition: account_read(
            collateral_id,
            &account(TOKEN_PROGRAM_ID, Data::from(&collateral_definition)),
        ),
        market_price_oracle: account_read(
            oracle_id,
            &account(ORACLE_PROGRAM_ID, Data::from(&oracle)),
        ),
        clock: account_read(
            CLOCK_01_PROGRAM_ACCOUNT_ID,
            &account(CLOCK_PROGRAM_ID, ok(Data::try_from(clock.to_bytes()))),
        ),
        initial_stability_fee_per_millisecond: json!(u128::MAX.to_string()),
        initial_controller_proportional_gain: json!(i128::MIN.to_string()),
        initial_controller_integral_gain: json!(i128::MAX.to_string()),
        initial_minimum_collateralization_ratio: json!((u128::MAX - 1).to_string()),
        minimum_milliseconds_between_rate_updates: json!(u64::MAX),
        maximum_oracle_price_age_milliseconds: json!(u64::MAX.to_string()),
        initial_redemption_price: json!("\"340282366920938463463374607431768211455\""),
        stablecoin_name: String::from("Exact Stablecoin"),
    }
}

const POKE_NOW: u64 = 1_000;
const POKE_LAST_UPDATE: u64 = 900;

fn poke_parameters(is_frozen: bool) -> ProtocolParameters {
    ProtocolParameters {
        admin_account_id: id(1),
        freeze_authority_account_id: id(2),
        stablecoin_definition_id: id(3),
        collateral_definition_id: id(4),
        market_price_oracle_id: id(5),
        stability_fee_per_millisecond: FIXED_POINT_ONE,
        controller_proportional_gain: FIXED_POINT_ONE as i128,
        controller_integral_gain: 0,
        minimum_collateralization_ratio: FIXED_POINT_ONE,
        minimum_milliseconds_between_rate_updates: 50,
        maximum_oracle_price_age_milliseconds: 50,
        is_frozen,
    }
}

fn poke_parameters_read(parameters: &ProtocolParameters) -> crate::AccountRead {
    account_read(
        compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID),
        &account(STABLECOIN_PROGRAM_ID, Data::from(parameters)),
    )
}

fn poke_accumulator_read() -> crate::AccountRead {
    account_read(
        compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID),
        &account(
            STABLECOIN_PROGRAM_ID,
            Data::from(&StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
                last_accrued_at: POKE_LAST_UPDATE,
            }),
        ),
    )
}

fn poke_redemption_read(last_updated_at: u64) -> crate::AccountRead {
    account_read(
        compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID),
        &account(
            STABLECOIN_PROGRAM_ID,
            Data::from(&RedemptionPriceState {
                redemption_price_at_last_update: FIXED_POINT_ONE,
                redemption_rate_per_millisecond: FIXED_POINT_ONE,
                controller_integral_term: 0,
                last_updated_at,
            }),
        ),
    )
}

fn poke_oracle_read(price: u128, timestamp: u64) -> crate::AccountRead {
    account_read(
        id(5),
        &account(
            ORACLE_PROGRAM_ID,
            Data::from(&OraclePriceAccount {
                base_asset: id(3),
                quote_asset: id(4),
                price,
                timestamp,
                source_id: id(6),
                confidence_interval: 0,
            }),
        ),
    )
}

fn poke_clock_read(timestamp: u64) -> crate::AccountRead {
    let clock = ClockAccountData {
        block_id: 1,
        timestamp,
    };
    account_read(
        CLOCK_01_PROGRAM_ACCOUNT_ID,
        &account(CLOCK_PROGRAM_ID, ok(Data::try_from(clock.to_bytes()))),
    )
}

fn accrue_request(is_frozen: bool) -> AccrueStabilityFeePlanRequest {
    AccrueStabilityFeePlanRequest {
        stablecoin_program_id: program_id_hex(),
        caller_id: account_id_hex(id(15)),
        protocol_parameters: poke_parameters_read(&poke_parameters(is_frozen)),
        stability_fee_accumulator: poke_accumulator_read(),
        clock: poke_clock_read(POKE_NOW),
    }
}

fn update_request(is_frozen: bool) -> UpdateRedemptionRatePlanRequest {
    UpdateRedemptionRatePlanRequest {
        stablecoin_program_id: program_id_hex(),
        caller_id: account_id_hex(id(15)),
        protocol_parameters: poke_parameters_read(&poke_parameters(is_frozen)),
        redemption_price_state: poke_redemption_read(POKE_LAST_UPDATE),
        market_price_oracle: poke_oracle_read(1 << 64, POKE_NOW),
        clock: poke_clock_read(POKE_NOW),
    }
}

fn refresh_request(is_frozen: bool) -> RefreshGlobalsPlanRequest {
    RefreshGlobalsPlanRequest {
        stablecoin_program_id: program_id_hex(),
        caller_id: account_id_hex(id(15)),
        protocol_parameters: poke_parameters_read(&poke_parameters(is_frozen)),
        stability_fee_accumulator: poke_accumulator_read(),
        redemption_price_state: poke_redemption_read(POKE_LAST_UPDATE),
        market_price_oracle: poke_oracle_read(1 << 64, POKE_NOW),
        clock: poke_clock_read(POKE_NOW),
    }
}

fn open_position_request(is_frozen: bool) -> OpenPositionPlanRequest {
    let collateral_id = id(4);
    let collateral_definition = TokenDefinition::Fungible {
        name: String::from("Collateral"),
        total_supply: u128::MAX,
        metadata_id: None,
        authority: None,
    };
    OpenPositionPlanRequest {
        stablecoin_program_id: program_id_hex(),
        owner_id: account_id_hex(id(20)),
        position_nonce: u64::MAX.to_string(),
        initial_collateral_amount: u128::MAX.to_string(),
        user_collateral_holding_id: account_id_hex(id(21)),
        user_collateral_holding: account_read(
            id(21),
            &account(
                TOKEN_PROGRAM_ID,
                Data::from(&TokenHolding::Fungible {
                    definition_id: collateral_id,
                    balance: u128::MAX,
                }),
            ),
        ),
        collateral_definition: account_read(
            collateral_id,
            &account(TOKEN_PROGRAM_ID, Data::from(&collateral_definition)),
        ),
        protocol_parameters: poke_parameters_read(&poke_parameters(is_frozen)),
        clock: poke_clock_read(POKE_NOW),
    }
}

fn deposit_collateral_request(is_frozen: bool, amount: Value) -> DepositCollateralPlanRequest {
    let owner = id(20);
    let position_nonce = u64::MAX;
    let position_id = compute_position_pda(STABLECOIN_PROGRAM_ID, owner, position_nonce);
    let vault_id = compute_position_vault_pda(STABLECOIN_PROGRAM_ID, position_id);
    let collateral_id = id(4);
    let holding_id = id(21);
    let position = Position {
        owner_account_id: owner,
        position_nonce,
        vault_account_id: vault_id,
        // Deliberately differs from the live vault balance to model a donation.
        collateral_amount: 5,
        normalized_debt_amount: 42,
        opened_at: 7,
    };

    DepositCollateralPlanRequest {
        stablecoin_program_id: program_id_hex(),
        owner_id: account_id_hex(owner),
        position_nonce: position_nonce.to_string(),
        amount,
        user_collateral_holding_id: account_id_hex(holding_id),
        position: account_read(
            position_id,
            &account(STABLECOIN_PROGRAM_ID, Data::from(&position)),
        ),
        vault: account_read(
            vault_id,
            &account(
                TOKEN_PROGRAM_ID,
                Data::from(&TokenHolding::Fungible {
                    definition_id: collateral_id,
                    balance: 75,
                }),
            ),
        ),
        user_collateral_holding: account_read(
            holding_id,
            &account(
                TOKEN_PROGRAM_ID,
                Data::from(&TokenHolding::Fungible {
                    definition_id: collateral_id,
                    balance: u128::MAX,
                }),
            ),
        ),
        protocol_parameters: poke_parameters_read(&poke_parameters(is_frozen)),
    }
}

fn decode_instruction(value: &Value) -> Instruction {
    let bytes: Vec<u8> = ok(serde_json::from_value(value.clone()));
    ok(borsh::from_slice::<Instruction>(&bytes))
}

#[test]
fn program_info_derives_all_singleton_ids_from_program_id() {
    let value = ok(program_info(ProgramInfoRequest {
        stablecoin_program_id: Some(program_id_hex()),
        elf: None,
        program_header: None,
    }));
    assert_eq!(value["imageIdHex"], "");

    let program_account = STABLECOIN_PROGRAM_ID;
    assert_eq!(value["programId"], program_account.to_string());
    assert_eq!(value["programIdHex"], account_id_hex(program_account));
    assert_eq!(
        value["protocolParametersIdHex"],
        account_id_hex(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID))
    );
    assert_eq!(
        value["stabilityFeeAccumulatorIdHex"],
        account_id_hex(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID))
    );
    assert_eq!(
        value["redemptionPriceStateIdHex"],
        account_id_hex(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID))
    );
    assert_eq!(
        value["stablecoinDefinitionIdHex"],
        account_id_hex(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID))
    );
    assert_eq!(
        value["stablecoinMasterHoldingIdHex"],
        account_id_hex(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID))
    );
    assert_eq!(
        value["clockIdHex"],
        account_id_hex(CLOCK_01_PROGRAM_ACCOUNT_ID)
    );
}

fn header_data(image_id: AccountId) -> Data {
    let header = ProgramHeader {
        image_id: image_id.into(),
        program_first_segment: id(0x77),
        immutable: true,
    };
    ok(Data::try_from(header.to_bytes()))
}

/// The header a deployer created at `program_id` for bytecode with `image_id`.
fn program_header(program_id: AccountId, image_id: AccountId) -> AccountRead {
    account_read(
        program_id,
        &account(PROGRAM_LOADER_ACCOUNT_ID, header_data(image_id)),
    )
}

#[test]
fn program_info_checks_a_binary_against_the_deployed_header() {
    // The program lives at an address unrelated to its ImageID, as any deploy may pick.
    let (binary, image_id) = deployable_program_binary();
    assert_ne!(image_id, STABLECOIN_PROGRAM_ID);
    let value = ok(program_info(ProgramInfoRequest {
        stablecoin_program_id: Some(program_id_hex()),
        elf: Some(binary),
        program_header: Some(program_header(STABLECOIN_PROGRAM_ID, image_id)),
    }));
    assert_eq!(value["programIdHex"], program_id_hex());
    assert_eq!(value["imageIdHex"], account_id_hex(image_id));
    assert_eq!(
        value["protocolParametersIdHex"],
        account_id_hex(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID))
    );
}

#[test]
fn program_info_rejects_a_binary_that_is_not_deployed_there() {
    let (binary, image_id) = deployable_program_binary();
    let request = |header: Option<AccountRead>| ProgramInfoRequest {
        stablecoin_program_id: Some(program_id_hex()),
        elf: Some(binary.clone()),
        program_header: header,
    };

    assert_error(
        program_info(request(Some(program_header(
            STABLECOIN_PROGRAM_ID,
            id(0x99),
        )))),
        "program_binary_mismatch",
    );
    assert_error(program_info(request(None)), "program_header_unavailable");
    // A header read for another account, one not owned by the program loader, or
    // undecodable data does not describe the configured program.
    assert_error(
        program_info(request(Some(program_header(id(0x55), image_id)))),
        "program_header_unavailable",
    );
    let forged = account_read(
        STABLECOIN_PROGRAM_ID,
        &account(TOKEN_PROGRAM_ID, header_data(image_id)),
    );
    assert_error(
        program_info(request(Some(forged))),
        "program_header_unavailable",
    );
    let garbage = account_read(
        STABLECOIN_PROGRAM_ID,
        &account(
            PROGRAM_LOADER_ACCOUNT_ID,
            ok(Data::try_from(vec![1_u8, 2, 3])),
        ),
    );
    assert_error(
        program_info(request(Some(garbage))),
        "program_header_unavailable",
    );
}

#[test]
fn program_info_rejects_missing_and_invalid_inputs() {
    assert_error(
        program_info(ProgramInfoRequest {
            stablecoin_program_id: None,
            elf: None,
            program_header: None,
        }),
        "config_missing",
    );
    // The ImageID is not an address: a binary alone cannot name the program.
    let (binary, _) = deployable_program_binary();
    assert_error(
        program_info(ProgramInfoRequest {
            stablecoin_program_id: None,
            elf: Some(binary),
            program_header: None,
        }),
        "config_missing",
    );
    assert_error(
        program_info(ProgramInfoRequest {
            stablecoin_program_id: Some(String::from("not-an-id")),
            elf: None,
            program_header: None,
        }),
        "invalid_program_id",
    );
    assert_error(
        program_info(ProgramInfoRequest {
            stablecoin_program_id: Some(program_id_hex()),
            elf: Some(String::from("00")),
            program_header: None,
        }),
        "invalid_program_binary",
    );
}

#[test]
fn protocol_parameters_decode_preserves_exact_numeric_and_id_fields() {
    let parameters = protocol_parameters();
    let value = ok(decode_protocol_parameters(protocol_request(&parameters)));

    assert_eq!(
        value["adminIdHex"],
        account_id_hex(parameters.admin_account_id)
    );
    assert_eq!(
        value["freezeAuthorityIdHex"],
        account_id_hex(parameters.freeze_authority_account_id)
    );
    assert_eq!(value["stabilityFeePerMillisecond"], u128::MAX.to_string());
    assert_eq!(value["controllerProportionalGain"], i128::MIN.to_string());
    assert_eq!(value["controllerIntegralGain"], i128::MAX.to_string());
    assert_eq!(
        value["minimumMillisecondsBetweenRateUpdates"],
        u64::MAX.to_string()
    );
    assert_eq!(value["isFrozen"], true);
}

#[test]
fn protocol_parameters_decode_rejects_wrong_pda_owner_and_non_exact_data() {
    let parameters = protocol_parameters();
    let mut wrong_pda = protocol_request(&parameters);
    wrong_pda.protocol_parameters.id = account_id_hex(id(20));
    assert_error(
        decode_protocol_parameters(wrong_pda),
        "protocol_parameters_pda_mismatch",
    );

    let mut wrong_owner = protocol_request(&parameters);
    if let Some(account) = &mut wrong_owner.protocol_parameters.account {
        account.program_owner = account_id_hex(TOKEN_PROGRAM_ID);
    }
    assert_error(
        decode_protocol_parameters(wrong_owner),
        "stablecoin_program_mismatch",
    );

    let mut trailing = Data::from(&parameters).as_ref().to_vec();
    trailing.push(0);
    let malformed = DecodeProtocolParametersRequest {
        stablecoin_program_id: program_id_hex(),
        protocol_parameters: account_read(
            compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID),
            &account(STABLECOIN_PROGRAM_ID, ok(Data::try_from(trailing))),
        ),
    };
    assert_error(
        decode_protocol_parameters(malformed),
        "invalid_protocol_parameters_data",
    );
}

#[test]
fn stability_fee_accumulator_decode_preserves_fixed_id_and_boundary_values() {
    let accumulator = stability_fee_accumulator();
    let value = ok(decode_stability_fee_accumulator(accumulator_request(
        &accumulator,
    )));

    assert_eq!(
        value["accountId"],
        "4gA2zUQxomXfgMaPv7WF4YhjNmQHjM3bNNC1sRyF4uQq"
    );
    assert_eq!(
        value["accountIdHex"],
        "369aed6f31e8b5a47faba313c65a343b67425e7fa9d4cee387f8e29fb3310e0e"
    );
    assert_eq!(value["accumulatedRateAtLastAccrual"], u128::MAX.to_string());
    assert_eq!(value["lastAccruedAt"], u64::MAX.to_string());
}

#[test]
fn stability_fee_accumulator_decode_rejects_failed_reads_and_wrong_identity() {
    let accumulator = stability_fee_accumulator();

    let mut missing = accumulator_request(&accumulator);
    missing.stability_fee_accumulator.status = String::from("not_found");
    missing.stability_fee_accumulator.account = None;
    assert_error(
        decode_stability_fee_accumulator(missing),
        "account_read_failed",
    );

    let mut wrong_pda = accumulator_request(&accumulator);
    wrong_pda.stability_fee_accumulator.id = account_id_hex(id(20));
    assert_error(
        decode_stability_fee_accumulator(wrong_pda),
        "stability_fee_accumulator_pda_mismatch",
    );

    let mut wrong_owner = accumulator_request(&accumulator);
    if let Some(account) = &mut wrong_owner.stability_fee_accumulator.account {
        account.program_owner = account_id_hex(TOKEN_PROGRAM_ID);
    }
    assert_error(
        decode_stability_fee_accumulator(wrong_owner),
        "stablecoin_program_mismatch",
    );
}

#[test]
fn stability_fee_accumulator_decode_rejects_truncated_and_trailing_data() {
    let accumulator = stability_fee_accumulator();
    let account_id = compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID);
    let encoded = Data::from(&accumulator).as_ref().to_vec();

    for malformed in [encoded[..encoded.len() - 1].to_vec(), {
        let mut trailing = encoded.clone();
        trailing.push(0);
        trailing
    }] {
        let request = DecodeStabilityFeeAccumulatorRequest {
            stablecoin_program_id: program_id_hex(),
            stability_fee_accumulator: account_read(
                account_id,
                &account(STABLECOIN_PROGRAM_ID, ok(Data::try_from(malformed))),
            ),
        };
        assert_error(
            decode_stability_fee_accumulator(request),
            "invalid_stability_fee_accumulator_data",
        );
    }
}

#[test]
fn position_info_has_fixed_owner_nonce_and_domain_separated_vectors() {
    let value = ok(position_info(position_info_request(
        id(0x2a),
        "18446744073709551615",
    )));

    assert_eq!(
        value["ownerId"],
        "3qbR1eZRqXUWroWKKYhbDmR3FfqTHfqSU8zZSxtANzYh"
    );
    assert_eq!(
        value["ownerIdHex"],
        "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a"
    );
    assert_eq!(value["positionNonce"], "18446744073709551615");
    assert_eq!(
        value["positionId"],
        "5b4G1Dy6YZ11Beitx96kJeWVFUbexCeqKoEnRy64Qg6b"
    );
    assert_eq!(
        value["positionIdHex"],
        "44282e08e5ce89a1e6d509efffb5b03294b2b5b8bcc242a46520584c12840fa8"
    );
    assert_eq!(
        value["vaultId"],
        "HCssgv4hQr8Ss4FxrZ9soMUUkSJTNLhcHJnLHoM44eqj"
    );
    assert_eq!(
        value["vaultIdHex"],
        "f0c5e72f17d09fe157f332bc69e3645504f6244fbf9f2b9622c685a83c05d7e6"
    );
    assert_ne!(value["positionIdHex"], value["vaultIdHex"]);
}

#[test]
fn position_info_rejects_invalid_owner_and_non_exact_nonce() {
    for owner_id in [String::from("not-an-id"), "0".repeat(64)] {
        let mut request = position_info_request(id(0x2a), "1");
        request.owner_id = owner_id;
        assert_error(position_info(request), "invalid_account_id");
    }

    for position_nonce in ["", "-1", "+1", "1.0", "1e3", " 1", "18446744073709551616"] {
        assert_error(
            position_info(position_info_request(id(0x2a), position_nonce)),
            "invalid_numeric_value",
        );
    }
}

#[test]
fn position_decode_preserves_full_integer_range_and_exact_ids() {
    let owner_id = id(0x2a);
    let stored_position = position(owner_id, u64::MAX);
    let value = ok(decode_position(position_decode_request(
        owner_id,
        u64::MAX,
        &stored_position,
    )));

    assert_eq!(value["ownerIdHex"], account_id_hex(owner_id));
    assert_eq!(value["positionNonce"], u64::MAX.to_string());
    assert_eq!(value["collateralAmount"], u128::MAX.to_string());
    assert_eq!(value["normalizedDebtAmount"], u128::MAX.to_string());
    assert_eq!(value["openedAt"], u64::MAX.to_string());
    assert_eq!(
        value["positionIdHex"],
        account_id_hex(compute_position_pda(
            STABLECOIN_PROGRAM_ID,
            owner_id,
            u64::MAX
        ))
    );
    assert_eq!(
        value["vaultIdHex"],
        account_id_hex(stored_position.vault_account_id)
    );
}

#[test]
fn position_decode_rejects_wrong_address_program_and_stored_identity() {
    let owner_id = id(0x2a);
    let stored_position = position(owner_id, 7);

    let mut wrong_address = position_decode_request(owner_id, 7, &stored_position);
    wrong_address.position.id = account_id_hex(id(0x40));
    assert_error(decode_position(wrong_address), "position_pda_mismatch");

    let mut wrong_program = position_decode_request(owner_id, 7, &stored_position);
    if let Some(account) = &mut wrong_program.position.account {
        account.program_owner = account_id_hex(TOKEN_PROGRAM_ID);
    }
    assert_error(
        decode_position(wrong_program),
        "stablecoin_program_mismatch",
    );

    let wrong_owner = Position {
        owner_account_id: id(0x41),
        ..stored_position.clone()
    };
    assert_error(
        decode_position(position_decode_request(owner_id, 7, &wrong_owner)),
        "position_owner_mismatch",
    );

    let wrong_nonce = Position {
        position_nonce: 8,
        ..stored_position.clone()
    };
    assert_error(
        decode_position(position_decode_request(owner_id, 7, &wrong_nonce)),
        "position_nonce_mismatch",
    );

    let wrong_vault = Position {
        vault_account_id: id(0x42),
        ..stored_position
    };
    assert_error(
        decode_position(position_decode_request(owner_id, 7, &wrong_vault)),
        "position_vault_mismatch",
    );
}

#[test]
fn position_decode_rejects_failed_reads_and_non_exact_data() {
    let owner_id = id(0x2a);
    let stored_position = position(owner_id, 7);

    let mut failed_read = position_decode_request(owner_id, 7, &stored_position);
    failed_read.position.status = String::from("not_found");
    failed_read.position.account = None;
    assert_error(decode_position(failed_read), "account_read_failed");

    let mut truncated = Data::from(&stored_position).as_ref().to_vec();
    let _ = truncated.pop();
    let truncated_request = DecodePositionRequest {
        stablecoin_program_id: program_id_hex(),
        owner_id: account_id_hex(owner_id),
        position_nonce: String::from("7"),
        position: account_read(
            compute_position_pda(STABLECOIN_PROGRAM_ID, owner_id, 7),
            &account(STABLECOIN_PROGRAM_ID, ok(Data::try_from(truncated))),
        ),
    };
    assert_error(decode_position(truncated_request), "invalid_position_data");

    let mut trailing = Data::from(&stored_position).as_ref().to_vec();
    trailing.push(0);
    let trailing_request = DecodePositionRequest {
        stablecoin_program_id: program_id_hex(),
        owner_id: account_id_hex(owner_id),
        position_nonce: String::from("7"),
        position: account_read(
            compute_position_pda(STABLECOIN_PROGRAM_ID, owner_id, 7),
            &account(STABLECOIN_PROGRAM_ID, ok(Data::try_from(trailing))),
        ),
    };
    assert_error(decode_position(trailing_request), "invalid_position_data");
}

#[test]
fn redemption_price_state_decode_preserves_fixed_id_and_boundary_values() {
    let expected_id = compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID);
    assert_eq!(
        account_id_hex(expected_id),
        "0236c97aef1dd1f3b9b98ccbbfe97065f0c1a8fa54ac46e9d7fc1b2c89f6bfea"
    );
    assert_eq!(
        expected_id.to_string(),
        "9eGd1RnZscJZTvPEp1fKLE8TW1xbQ2pev1aDEKop5H3"
    );

    for controller_integral_term in [1, 0, -1, i128::MIN, i128::MAX] {
        let state = redemption_price_state(controller_integral_term);
        let value = ok(decode_redemption_price_state(
            redemption_price_state_request(&state),
        ));

        assert_eq!(value["accountId"], expected_id.to_string());
        assert_eq!(value["accountIdHex"], account_id_hex(expected_id));
        assert_eq!(value["redemptionPriceAtLastUpdate"], u128::MAX.to_string());
        assert_eq!(value["redemptionRatePerMillisecond"], u128::MAX.to_string());
        assert_eq!(
            value["controllerIntegralTerm"],
            controller_integral_term.to_string()
        );
        assert_eq!(value["lastUpdatedAt"], u64::MAX.to_string());
    }
}

#[test]
fn redemption_price_state_decode_rejects_failed_reads_and_wrong_identity() {
    let state = redemption_price_state(0);

    for status in ["not_found", "backend_error"] {
        let mut failed = redemption_price_state_request(&state);
        failed.redemption_price_state.status = String::from(status);
        failed.redemption_price_state.account = None;
        assert_error(decode_redemption_price_state(failed), "account_read_failed");
    }

    let mut wrong_pda = redemption_price_state_request(&state);
    wrong_pda.redemption_price_state.id = account_id_hex(id(20));
    assert_error(
        decode_redemption_price_state(wrong_pda),
        "redemption_price_state_pda_mismatch",
    );

    let mut wrong_owner = redemption_price_state_request(&state);
    if let Some(account) = &mut wrong_owner.redemption_price_state.account {
        account.program_owner = account_id_hex(TOKEN_PROGRAM_ID);
    }
    assert_error(
        decode_redemption_price_state(wrong_owner),
        "stablecoin_program_mismatch",
    );
}

#[test]
fn redemption_price_state_decode_rejects_truncated_and_trailing_data() {
    let state = redemption_price_state(i128::MIN);
    let account_id = compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID);
    let encoded = Data::from(&state).as_ref().to_vec();

    for malformed in [encoded[..encoded.len() - 1].to_vec(), {
        let mut trailing = encoded.clone();
        trailing.push(0);
        trailing
    }] {
        let request = DecodeRedemptionPriceStateRequest {
            stablecoin_program_id: program_id_hex(),
            redemption_price_state: account_read(
                account_id,
                &account(STABLECOIN_PROGRAM_ID, ok(Data::try_from(malformed))),
            ),
        };
        assert_error(
            decode_redemption_price_state(request),
            "invalid_redemption_price_state_data",
        );
    }
}

#[test]
fn initialize_plan_round_trips_all_boundary_values_and_exact_account_contract() {
    let request = initialize_request();
    let admin = request.admin_id.clone();
    let freeze_authority = request.freeze_authority_id.clone();
    let collateral = request.collateral_definition.id.clone();
    let oracle = request.market_price_oracle.id.clone();
    let plan = ok(initialize_program_plan(request));

    assert_eq!(plan["programId"], program_id_hex());
    assert_eq!(
        plan["accountIds"],
        json!([
            admin,
            account_id_hex(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)),
            account_id_hex(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)),
            account_id_hex(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
            account_id_hex(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
            account_id_hex(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
            collateral,
            oracle,
            account_id_hex(CLOCK_01_PROGRAM_ACCOUNT_ID),
        ])
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([true, false, false, false, false, false, false, false, false])
    );

    let Instruction::InitializeProgram {
        freeze_authority_account_id,
        initial_stability_fee_per_millisecond,
        initial_controller_proportional_gain,
        initial_controller_integral_gain,
        initial_minimum_collateralization_ratio,
        minimum_milliseconds_between_rate_updates,
        maximum_oracle_price_age_milliseconds,
        initial_redemption_price,
        stablecoin_name,
    } = decode_instruction(&plan["instruction"])
    else {
        panic!("expected InitializeProgram");
    };
    assert_eq!(
        account_id_hex(freeze_authority_account_id),
        freeze_authority
    );
    assert_eq!(initial_stability_fee_per_millisecond, u128::MAX);
    assert_eq!(initial_controller_proportional_gain, i128::MIN);
    assert_eq!(initial_controller_integral_gain, i128::MAX);
    assert_eq!(initial_minimum_collateralization_ratio, u128::MAX - 1);
    assert_eq!(minimum_milliseconds_between_rate_updates, u64::MAX);
    assert_eq!(maximum_oracle_price_age_milliseconds, u64::MAX);
    assert_eq!(initial_redemption_price, u128::MAX);
    assert_eq!(stablecoin_name, "Exact Stablecoin");
}

#[test]
fn initialize_plan_rejects_lossy_or_out_of_range_numeric_values() {
    let exponent: Value = ok(serde_json::from_str("1e3"));
    for invalid in [
        json!(1.5),
        exponent,
        json!(-1),
        json!("340282366920938463463374607431768211456"),
        json!("1e3"),
        json!(""),
    ] {
        let mut request = initialize_request();
        request.initial_redemption_price = invalid;
        assert_error(initialize_program_plan(request), "invalid_numeric_value");
    }

    let mut signed_overflow = initialize_request();
    signed_overflow.initial_controller_integral_gain =
        json!("170141183460469231731687303715884105728");
    assert_error(
        initialize_program_plan(signed_overflow),
        "invalid_numeric_value",
    );
}

#[test]
fn initialize_plan_validates_required_account_shapes_and_assets() {
    let mut nft_collateral = initialize_request();
    let collateral_id = id(10);
    nft_collateral.collateral_definition = account_read(
        collateral_id,
        &account(
            TOKEN_PROGRAM_ID,
            Data::from(&TokenDefinition::NonFungible {
                name: String::from("NFT"),
                printable_supply: 1,
                metadata_id: id(30),
            }),
        ),
    );
    assert_error(
        initialize_program_plan(nft_collateral),
        "invalid_collateral_definition",
    );

    let mut wrong_oracle = initialize_request();
    wrong_oracle.market_price_oracle = account_read(
        id(11),
        &account(
            ORACLE_PROGRAM_ID,
            Data::from(&OraclePriceAccount {
                base_asset: id(31),
                quote_asset: id(10),
                price: 1,
                timestamp: 2,
                source_id: id(12),
                confidence_interval: 0,
            }),
        ),
    );
    assert_error(
        initialize_program_plan(wrong_oracle),
        "oracle_asset_mismatch",
    );

    let mut wrong_clock = initialize_request();
    wrong_clock.clock.id = account_id_hex(id(32));
    assert_error(initialize_program_plan(wrong_clock), "invalid_clock");

    let mut failed_read = initialize_request();
    failed_read.collateral_definition.status = String::from("read_failed");
    failed_read.collateral_definition.account = None;
    assert_error(initialize_program_plan(failed_read), "account_read_failed");
}

#[test]
fn poke_plans_pin_instruction_words_accounts_and_caller_only_signing() {
    let caller = account_id_hex(id(15));
    let parameters = account_id_hex(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID));
    let accumulator = account_id_hex(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID));
    let redemption = account_id_hex(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID));
    let oracle = account_id_hex(id(5));
    let clock = account_id_hex(CLOCK_01_PROGRAM_ACCOUNT_ID);

    let accrue = ok(accrue_stability_fee_plan(accrue_request(false)));
    assert_eq!(accrue["programId"], program_id_hex());
    assert_eq!(
        accrue["accountIds"],
        json!([caller, parameters, accumulator, clock])
    );
    assert_eq!(
        accrue["signingRequirements"],
        json!([true, false, false, false])
    );
    assert_eq!(accrue["instruction"], json!([1]));
    assert!(matches!(
        decode_instruction(&accrue["instruction"]),
        Instruction::AccrueStabilityFee
    ));

    let update = ok(update_redemption_rate_plan(update_request(false)));
    assert_eq!(update["programId"], program_id_hex());
    assert_eq!(
        update["accountIds"],
        json!([caller, parameters, redemption, oracle, clock])
    );
    assert_eq!(
        update["signingRequirements"],
        json!([true, false, false, false, false])
    );
    assert_eq!(update["instruction"], json!([2]));
    assert!(matches!(
        decode_instruction(&update["instruction"]),
        Instruction::UpdateRedemptionRate
    ));

    let refresh = ok(refresh_globals_plan(refresh_request(false)));
    assert_eq!(refresh["programId"], program_id_hex());
    assert_eq!(
        refresh["accountIds"],
        json!([caller, parameters, accumulator, redemption, oracle, clock])
    );
    assert_eq!(
        refresh["signingRequirements"],
        json!([true, false, false, false, false, false])
    );
    assert_eq!(refresh["instruction"], json!([3]));
    assert!(matches!(
        decode_instruction(&refresh["instruction"]),
        Instruction::RefreshGlobals
    ));
}

#[test]
fn strict_update_plan_reuses_quote_gate_order() {
    let mut stale = update_request(false);
    stale.market_price_oracle = poke_oracle_read(1 << 64, POKE_NOW - 51);
    assert_error(update_redemption_rate_plan(stale), "oracle_stale");

    let mut zero = update_request(false);
    zero.market_price_oracle = poke_oracle_read(0, POKE_NOW);
    assert_error(update_redemption_rate_plan(zero), "oracle_price_zero");

    let mut too_soon = update_request(false);
    too_soon.redemption_price_state = poke_redemption_read(POKE_NOW - 49);
    assert_error(
        update_redemption_rate_plan(too_soon),
        "rate_update_too_soon",
    );

    let mut combined = update_request(false);
    combined.market_price_oracle = poke_oracle_read(0, POKE_NOW - 51);
    combined.redemption_price_state = poke_redemption_read(POKE_NOW - 49);
    assert_error(update_redemption_rate_plan(combined), "oracle_stale");
}

#[test]
fn refresh_plan_keeps_controller_quote_gates_soft() {
    let mut stale = refresh_request(false);
    stale.market_price_oracle = poke_oracle_read(1 << 64, POKE_NOW - 51);
    assert!(refresh_globals_plan(stale).is_ok());

    let mut zero = refresh_request(false);
    zero.market_price_oracle = poke_oracle_read(0, POKE_NOW);
    assert!(refresh_globals_plan(zero).is_ok());

    let mut too_soon = refresh_request(false);
    too_soon.redemption_price_state = poke_redemption_read(POKE_NOW - 49);
    assert!(refresh_globals_plan(too_soon).is_ok());
}

#[test]
fn all_poke_plans_submit_while_protocol_is_frozen() {
    assert!(accrue_stability_fee_plan(accrue_request(true)).is_ok());
    assert!(update_redemption_rate_plan(update_request(true)).is_ok());
    assert!(refresh_globals_plan(refresh_request(true)).is_ok());
}

#[test]
fn poke_plans_reject_invalid_callers_and_hard_account_mismatches() {
    let mut invalid_caller = accrue_request(false);
    invalid_caller.caller_id = String::from("not-an-account");
    assert_error(
        accrue_stability_fee_plan(invalid_caller),
        "invalid_account_id",
    );

    let mut wrong_accumulator = accrue_request(false);
    wrong_accumulator.stability_fee_accumulator.id = account_id_hex(id(30));
    assert_error(
        accrue_stability_fee_plan(wrong_accumulator),
        "stability_fee_accumulator_pda_mismatch",
    );

    let mut wrong_redemption = update_request(false);
    wrong_redemption.redemption_price_state.id = account_id_hex(id(31));
    assert_error(
        update_redemption_rate_plan(wrong_redemption),
        "redemption_price_state_pda_mismatch",
    );

    let mut wrong_oracle = refresh_request(false);
    wrong_oracle.market_price_oracle.id = account_id_hex(id(32));
    assert_error(
        refresh_globals_plan(wrong_oracle),
        "market_price_oracle_mismatch",
    );

    let mut wrong_clock = refresh_request(false);
    wrong_clock.clock.id = account_id_hex(id(33));
    assert_error(refresh_globals_plan(wrong_clock), "invalid_clock");
}

#[test]
fn open_position_plan_derives_all_accounts_and_preserves_boundary_values() {
    let request = open_position_request(false);
    let owner = account_id_hex(id(20));
    let holding = request.user_collateral_holding_id.clone();
    let position = String::from("18dde7962af579e2e9bb804d0b76dd6e3cd16f906f795a8d8c048533b391256a");
    let vault = String::from("96cc8eae9ce458e0e6ed728a4e2364853b0a7f16829eb5223a52d6ceba76d4e0");
    let collateral = request.collateral_definition.id.clone();
    let plan = ok(open_position_plan(request));

    assert_eq!(plan["programId"], program_id_hex());
    assert_eq!(
        plan["accountIds"],
        json!([
            owner,
            position,
            vault,
            holding,
            collateral,
            account_id_hex(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)),
            account_id_hex(CLOCK_01_PROGRAM_ACCOUNT_ID),
        ])
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([true, false, false, true, false, false, false])
    );
    assert!(matches!(
        decode_instruction(&plan["instruction"]),
        Instruction::OpenPosition {
            position_nonce: u64::MAX,
            initial_collateral_amount: u128::MAX,
        }
    ));
}

#[test]
fn open_position_idl_pins_account_flags_and_numeric_argument_types() {
    let idl: Value = ok(serde_json::from_str(include_str!(
        "../../../../../artifacts/stablecoin-idl.json"
    )));
    let instruction = idl["instructions"]
        .as_array()
        .and_then(|instructions| {
            instructions
                .iter()
                .find(|instruction| instruction["name"] == "open_position")
        })
        .expect("open_position IDL instruction exists");
    let accounts = instruction["accounts"]
        .as_array()
        .expect("open_position account metadata exists");
    let contract = accounts
        .iter()
        .map(|account| {
            (
                account["name"].as_str().expect("account name"),
                account["writable"].as_bool().expect("writable flag"),
                account["signer"].as_bool().expect("signer flag"),
                account["init"].as_bool().expect("init flag"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        contract,
        vec![
            ("owner", false, true, false),
            ("position", true, false, true),
            ("vault", true, false, true),
            ("user_collateral_holding", true, true, false),
            ("collateral_definition", false, false, false),
            ("protocol_parameters", false, false, false),
            ("clock", false, false, false),
        ]
    );
    assert_eq!(
        instruction["args"],
        json!([
            {"name": "position_nonce", "type": "u64"},
            {"name": "initial_collateral_amount", "type": "u128"},
        ])
    );
}

#[test]
fn open_position_plan_rejects_malformed_or_out_of_range_decimal_strings() {
    for invalid in ["", "-1", "+1", "1.0", "1e3", "18446744073709551616"] {
        let mut request = open_position_request(false);
        request.position_nonce = String::from(invalid);
        assert_error(open_position_plan(request), "invalid_numeric_value");
    }

    for invalid in [
        "",
        "-1",
        "+1",
        "1.5",
        "1e3",
        "340282366920938463463374607431768211456",
    ] {
        let mut request = open_position_request(false);
        request.initial_collateral_amount = String::from(invalid);
        assert_error(open_position_plan(request), "invalid_numeric_value");
    }
}

#[test]
fn open_position_plan_checks_freeze_and_all_collateral_bindings() {
    assert_error(
        open_position_plan(open_position_request(true)),
        "protocol_frozen",
    );

    let mut wrong_collateral_id = open_position_request(false);
    let collateral_definition = TokenDefinition::Fungible {
        name: String::from("Other"),
        total_supply: 1,
        metadata_id: None,
        authority: None,
    };
    wrong_collateral_id.collateral_definition = account_read(
        id(22),
        &account(TOKEN_PROGRAM_ID, Data::from(&collateral_definition)),
    );
    assert_error(
        open_position_plan(wrong_collateral_id),
        "collateral_definition_mismatch",
    );

    let mut wrong_holding_definition = open_position_request(false);
    wrong_holding_definition.user_collateral_holding = account_read(
        id(21),
        &account(
            TOKEN_PROGRAM_ID,
            Data::from(&TokenHolding::Fungible {
                definition_id: id(22),
                balance: 10,
            }),
        ),
    );
    assert_error(
        open_position_plan(wrong_holding_definition),
        "invalid_user_collateral_holding",
    );

    let mut mismatched_token_program = open_position_request(false);
    mismatched_token_program.user_collateral_holding = account_read(
        id(21),
        &account(
            ORACLE_PROGRAM_ID,
            Data::from(&TokenHolding::Fungible {
                definition_id: id(4),
                balance: 10,
            }),
        ),
    );
    assert_error(
        open_position_plan(mismatched_token_program),
        "token_program_mismatch",
    );
}

#[test]
fn open_position_plan_requires_valid_public_account_reads_and_clock() {
    let mut missing_holding = open_position_request(false);
    missing_holding.user_collateral_holding.status = String::from("not_found");
    missing_holding.user_collateral_holding.account = None;
    assert_error(open_position_plan(missing_holding), "account_read_failed");

    let mut wrong_holding_read_id = open_position_request(false);
    wrong_holding_read_id.user_collateral_holding.id = account_id_hex(id(22));
    assert_error(
        open_position_plan(wrong_holding_read_id),
        "invalid_user_collateral_holding",
    );

    let mut wrong_clock = open_position_request(false);
    wrong_clock.clock.id = account_id_hex(id(23));
    assert_error(open_position_plan(wrong_clock), "invalid_clock");

    let mut wrong_parameters_pda = open_position_request(false);
    wrong_parameters_pda.protocol_parameters.id = account_id_hex(id(24));
    assert_error(
        open_position_plan(wrong_parameters_pda),
        "protocol_parameters_pda_mismatch",
    );

    let mut malformed_parameters = open_position_request(false);
    malformed_parameters.protocol_parameters = account_read(
        compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID),
        &account(STABLECOIN_PROGRAM_ID, ok(Data::try_from(vec![0xff_u8]))),
    );
    assert_error(
        open_position_plan(malformed_parameters),
        "invalid_protocol_parameters_data",
    );
}

#[test]
fn position_addresses_match_the_open_position_pdas() {
    let value = ok(position_addresses(PositionAddressesRequest {
        stablecoin_program_id: program_id_hex(),
        owner_id: account_id_hex(id(20)),
        position_nonce: u64::MAX.to_string(),
    }));

    assert_eq!(
        value["positionIdHex"],
        "18dde7962af579e2e9bb804d0b76dd6e3cd16f906f795a8d8c048533b391256a"
    );
    assert_eq!(
        value["vaultIdHex"],
        "96cc8eae9ce458e0e6ed728a4e2364853b0a7f16829eb5223a52d6ceba76d4e0"
    );
    assert_error(
        position_addresses(PositionAddressesRequest {
            stablecoin_program_id: program_id_hex(),
            owner_id: account_id_hex(id(20)),
            position_nonce: "18446744073709551616".to_owned(),
        }),
        "invalid_numeric_value",
    );
}

#[test]
fn deposit_collateral_plan_derives_accounts_and_allows_frozen_zero_reconciliation() {
    let request = deposit_collateral_request(true, json!("0"));
    let owner = request.owner_id.clone();
    let holding = request.user_collateral_holding_id.clone();
    let position_id = request.position.id.clone();
    let vault_id = request.vault.id.clone();
    let plan = ok(deposit_collateral_plan(request));

    assert_eq!(plan["programId"], program_id_hex());
    assert_eq!(
        plan["accountIds"],
        json!([
            owner,
            position_id,
            vault_id,
            holding,
            account_id_hex(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)),
        ])
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([true, false, false, true, false])
    );
    assert!(matches!(
        decode_instruction(&plan["instruction"]),
        Instruction::DepositCollateral { amount: 0 }
    ));
}

#[test]
fn deposit_collateral_plan_preserves_full_u128_amount_and_pins_idl_contract() {
    let mut request = deposit_collateral_request(true, json!(u128::MAX.to_string()));
    let vault_id = account_id_hex(compute_position_vault_pda(
        STABLECOIN_PROGRAM_ID,
        compute_position_pda(STABLECOIN_PROGRAM_ID, id(20), u64::MAX),
    ));
    request.vault = account_read(
        compute_position_vault_pda(
            STABLECOIN_PROGRAM_ID,
            compute_position_pda(STABLECOIN_PROGRAM_ID, id(20), u64::MAX),
        ),
        &account(
            TOKEN_PROGRAM_ID,
            Data::from(&TokenHolding::Fungible {
                definition_id: id(4),
                balance: 0,
            }),
        ),
    );
    let plan = ok(deposit_collateral_plan(request));
    assert_eq!(plan["accountIds"][2], vault_id);
    assert!(matches!(
        decode_instruction(&plan["instruction"]),
        Instruction::DepositCollateral { amount: u128::MAX }
    ));

    let mut large_json_integer = deposit_collateral_request(false, json!(u64::MAX));
    large_json_integer.vault = account_read(
        compute_position_vault_pda(
            STABLECOIN_PROGRAM_ID,
            compute_position_pda(STABLECOIN_PROGRAM_ID, id(20), u64::MAX),
        ),
        &account(
            TOKEN_PROGRAM_ID,
            Data::from(&TokenHolding::Fungible {
                definition_id: id(4),
                balance: 0,
            }),
        ),
    );
    let large_integer_plan = ok(deposit_collateral_plan(large_json_integer));
    assert!(matches!(
        decode_instruction(&large_integer_plan["instruction"]),
        Instruction::DepositCollateral { amount } if amount == u64::MAX as u128
    ));

    let idl: Value = ok(serde_json::from_str(include_str!(
        "../../../../../artifacts/stablecoin-idl.json"
    )));
    let instruction = idl["instructions"]
        .as_array()
        .and_then(|instructions| {
            instructions
                .iter()
                .find(|instruction| instruction["name"] == "deposit_collateral")
        })
        .expect("deposit_collateral IDL instruction exists");
    let accounts = instruction["accounts"]
        .as_array()
        .expect("deposit_collateral account metadata exists");
    let contract = accounts
        .iter()
        .map(|account| {
            (
                account["name"].as_str().expect("account name"),
                account["writable"].as_bool().expect("writable flag"),
                account["signer"].as_bool().expect("signer flag"),
                account["init"].as_bool().expect("init flag"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        contract,
        vec![
            ("owner", false, true, false),
            ("position", true, false, false),
            ("vault", true, false, false),
            ("user_collateral_holding", true, true, false),
            ("protocol_parameters", false, false, false),
        ]
    );
    assert_eq!(
        instruction["args"],
        json!([{"name": "amount", "type": "u128"}])
    );
}

#[test]
fn deposit_collateral_plan_rejects_lossy_amounts_and_inadequate_balances() {
    for invalid in [
        json!(1.5),
        json!(1e3),
        json!(-1),
        json!("340282366920938463463374607431768211456"),
        json!(""),
    ] {
        let request = deposit_collateral_request(false, invalid);
        assert_error(deposit_collateral_plan(request), "invalid_numeric_value");
    }

    let mut insufficient = deposit_collateral_request(false, json!("1"));
    let holding_id = id(21);
    insufficient.user_collateral_holding = account_read(
        holding_id,
        &account(
            TOKEN_PROGRAM_ID,
            Data::from(&TokenHolding::Fungible {
                definition_id: id(4),
                balance: 0,
            }),
        ),
    );
    assert_error(
        deposit_collateral_plan(insufficient),
        "insufficient_collateral_balance",
    );

    let overflow = deposit_collateral_request(false, json!(u128::MAX.to_string()));
    assert_error(
        deposit_collateral_plan(overflow),
        "collateral_amount_overflow",
    );
}

#[test]
fn deposit_collateral_plan_checks_position_vault_parameters_and_token_bindings() {
    let mut wrong_position = deposit_collateral_request(false, json!("1"));
    wrong_position.position.id = account_id_hex(id(22));
    assert_error(
        deposit_collateral_plan(wrong_position),
        "position_pda_mismatch",
    );

    let mut wrong_owner = deposit_collateral_request(false, json!("1"));
    let position_id = compute_position_pda(STABLECOIN_PROGRAM_ID, id(20), u64::MAX);
    wrong_owner.position = account_read(
        position_id,
        &account(
            STABLECOIN_PROGRAM_ID,
            Data::from(&Position {
                owner_account_id: id(22),
                position_nonce: u64::MAX,
                vault_account_id: compute_position_vault_pda(STABLECOIN_PROGRAM_ID, position_id),
                collateral_amount: 5,
                normalized_debt_amount: 42,
                opened_at: 7,
            }),
        ),
    );
    assert_error(
        deposit_collateral_plan(wrong_owner),
        "position_owner_mismatch",
    );

    let mut wrong_nonce = deposit_collateral_request(false, json!("1"));
    wrong_nonce.position = account_read(
        position_id,
        &account(
            STABLECOIN_PROGRAM_ID,
            Data::from(&Position {
                owner_account_id: id(20),
                position_nonce: u64::MAX - 1,
                vault_account_id: compute_position_vault_pda(STABLECOIN_PROGRAM_ID, position_id),
                collateral_amount: 5,
                normalized_debt_amount: 42,
                opened_at: 7,
            }),
        ),
    );
    assert_error(
        deposit_collateral_plan(wrong_nonce),
        "position_nonce_mismatch",
    );

    let mut wrong_vault = deposit_collateral_request(false, json!("1"));
    wrong_vault.vault.id = account_id_hex(id(22));
    assert_error(deposit_collateral_plan(wrong_vault), "vault_pda_mismatch");

    let mut wrong_token_program = deposit_collateral_request(false, json!("1"));
    wrong_token_program.user_collateral_holding = account_read(
        id(21),
        &account(
            ORACLE_PROGRAM_ID,
            Data::from(&TokenHolding::Fungible {
                definition_id: id(4),
                balance: u128::MAX,
            }),
        ),
    );
    assert_error(
        deposit_collateral_plan(wrong_token_program),
        "token_program_mismatch",
    );

    let mut wrong_definition = deposit_collateral_request(false, json!("1"));
    wrong_definition.vault = account_read(
        compute_position_vault_pda(STABLECOIN_PROGRAM_ID, position_id),
        &account(
            TOKEN_PROGRAM_ID,
            Data::from(&TokenHolding::Fungible {
                definition_id: id(22),
                balance: 75,
            }),
        ),
    );
    assert_error(
        deposit_collateral_plan(wrong_definition),
        "collateral_definition_mismatch",
    );

    let mut wrong_parameters = deposit_collateral_request(false, json!("1"));
    wrong_parameters.protocol_parameters.id = account_id_hex(id(22));
    assert_error(
        deposit_collateral_plan(wrong_parameters),
        "protocol_parameters_pda_mismatch",
    );
}
