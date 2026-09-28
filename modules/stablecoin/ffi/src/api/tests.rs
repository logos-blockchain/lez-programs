use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use risc0_binfmt::ProgramBinary;
use serde_json::{json, Value};
use stablecoin_core::{
    compute_position_pda, compute_position_vault_pda, compute_protocol_parameters_pda,
    compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
    compute_stablecoin_definition_pda, compute_stablecoin_master_holding_pda, Instruction,
    Position, ProtocolParameters,
};
use token_core::TokenDefinition;
use twap_oracle_core::OraclePriceAccount;

use super::{
    decode_position, decode_protocol_parameters, initialize_program_plan, position_info,
    program_info, DecodePositionRequest, DecodeProtocolParametersRequest,
    InitializeProgramPlanRequest, PositionInfoRequest, ProgramInfoRequest, StablecoinResult,
};
use crate::account::{account_id_hex, account_read, program_id_bytes};

const STABLECOIN_PROGRAM_ID: ProgramId = [0x11_u32; 8];
const TOKEN_PROGRAM_ID: ProgramId = [0x22_u32; 8];
const ORACLE_PROGRAM_ID: ProgramId = [0x33_u32; 8];
const CLOCK_PROGRAM_ID: ProgramId = [0x44_u32; 8];

fn account(owner: ProgramId, data: Data) -> Account {
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
    hex::encode(program_id_bytes(STABLECOIN_PROGRAM_ID))
}

fn deployable_program_binary() -> (String, ProgramId) {
    let encoded = stablecoin_methods::STABLECOIN_ELF;
    let binary = ok(ProgramBinary::decode(encoded));
    let image_id = ok(binary.compute_image_id()).into();
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

fn decode_instruction(value: &Value) -> Instruction {
    let words: Vec<u32> = ok(serde_json::from_value(value.clone()));
    ok(risc0_zkvm::serde::from_slice::<Instruction, u32>(&words))
}

#[test]
fn program_info_derives_all_singleton_ids_from_program_id() {
    let value = ok(program_info(ProgramInfoRequest {
        stablecoin_program_id: Some(program_id_hex()),
        elf: None,
    }));

    let program_account = AccountId::new(program_id_bytes(STABLECOIN_PROGRAM_ID));
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

#[test]
fn program_info_derives_from_binary_and_rejects_mismatched_inputs() {
    let (binary, derived_program_id) = deployable_program_binary();
    let derived_program_id_hex = hex::encode(program_id_bytes(derived_program_id));
    let value = ok(program_info(ProgramInfoRequest {
        stablecoin_program_id: None,
        elf: Some(binary.clone()),
    }));
    assert_eq!(value["programIdHex"], derived_program_id_hex);

    assert_error(
        program_info(ProgramInfoRequest {
            stablecoin_program_id: Some(program_id_hex()),
            elf: Some(binary.clone()),
        }),
        "program_id_mismatch",
    );

    let value = ok(program_info(ProgramInfoRequest {
        stablecoin_program_id: Some(derived_program_id_hex),
        elf: Some(binary),
    }));
    assert_eq!(
        value["programIdHex"],
        hex::encode(program_id_bytes(derived_program_id))
    );
}

#[test]
fn program_info_rejects_missing_and_invalid_inputs() {
    assert_error(
        program_info(ProgramInfoRequest {
            stablecoin_program_id: None,
            elf: None,
        }),
        "config_missing",
    );
    assert_error(
        program_info(ProgramInfoRequest {
            stablecoin_program_id: Some(String::from("not-an-id")),
            elf: None,
        }),
        "invalid_program_id",
    );
    assert_error(
        program_info(ProgramInfoRequest {
            stablecoin_program_id: None,
            elf: Some(String::from("00")),
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
        account.program_owner = hex::encode(program_id_bytes(TOKEN_PROGRAM_ID));
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
        "DrRDBHo3NRFzMjT75JFiHoL1tXCdfCmv86sAvfq4bJar"
    );
    assert_eq!(
        value["positionIdHex"],
        "bef513b932022feee48d0ab25aa2ffc935395fc473f5fc4148fa8f7d721e888f"
    );
    assert_eq!(
        value["vaultId"],
        "2iAjtRxV42s9SFN8aLwJ4MTzo4BAxBhXqbkEbkWd9YWV"
    );
    assert_eq!(
        value["vaultIdHex"],
        "19678333644b6bad9ced47c8dbb5424611069d5a93bd6f5840904a1e89e9bd6a"
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
        account.program_owner = hex::encode(program_id_bytes(TOKEN_PROGRAM_ID));
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
