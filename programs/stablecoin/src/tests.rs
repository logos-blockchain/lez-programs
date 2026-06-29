#![allow(
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests deliberately inspect fixed post-state slots and panic on invalid variants"
)]

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID, CLOCK_10_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::{AccountPostState, ChainedCall, Claim, ProgramId},
};
use stablecoin_core::{
    compound_rate, compute_position_pda, compute_position_pda_seed, compute_position_vault_pda,
    compute_protocol_parameters_pda, compute_protocol_parameters_pda_seed,
    compute_redemption_price_state_pda, compute_redemption_price_state_pda_seed,
    compute_stability_fee_accumulator_pda, compute_stability_fee_accumulator_pda_seed,
    compute_stablecoin_definition_pda, compute_stablecoin_definition_pda_seed,
    compute_stablecoin_master_holding_pda, compute_stablecoin_master_holding_pda_seed, Position,
    ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator, FIXED_POINT_ONE,
    MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS, MAX_STABILITY_FEE_PER_MILLISECOND,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

const STABLECOIN_PROGRAM_ID: ProgramId = [3u32; 8];
const TOKEN_PROGRAM_ID: ProgramId = [2u32; 8];
const CLOCK_PROGRAM_ID: ProgramId = [4u32; 8];
const ORACLE_PROGRAM_ID: ProgramId = [5u32; 8];
const POSITION_NONCE: u64 = 7;

fn owner_id() -> AccountId {
    AccountId::new([0x10u8; 32])
}

fn admin_id() -> AccountId {
    AccountId::new([0x11u8; 32])
}

fn freeze_authority_id() -> AccountId {
    AccountId::new([0x12u8; 32])
}

fn collateral_definition_id() -> AccountId {
    AccountId::new([0x20u8; 32])
}

fn other_collateral_definition_id() -> AccountId {
    AccountId::new([0x21u8; 32])
}

fn user_collateral_holding_id() -> AccountId {
    AccountId::new([0x30u8; 32])
}

fn destination_holding_id() -> AccountId {
    AccountId::new([0x40u8; 32])
}

fn user_stablecoin_holding_id() -> AccountId {
    AccountId::new([0x60u8; 32])
}

fn oracle_id() -> AccountId {
    AccountId::new([0x70u8; 32])
}

fn other_stablecoin_definition_id() -> AccountId {
    AccountId::new([0x80u8; 32])
}

fn position_id() -> AccountId {
    compute_position_pda(STABLECOIN_PROGRAM_ID, owner_id(), POSITION_NONCE)
}

fn vault_id() -> AccountId {
    compute_position_vault_pda(STABLECOIN_PROGRAM_ID, position_id())
}

fn account(
    account_id: AccountId,
    program_owner: ProgramId,
    data: Data,
    is_authorized: bool,
) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner,
            balance: 0,
            data,
            nonce: Nonce(0),
        },
        is_authorized,
        account_id,
    }
}

fn uninit(account_id: AccountId) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id,
    }
}

fn owner_account() -> AccountWithMetadata {
    let mut owner = uninit(owner_id());
    owner.is_authorized = true;
    owner
}

fn admin_account() -> AccountWithMetadata {
    let mut admin = uninit(admin_id());
    admin.is_authorized = true;
    admin
}

fn caller_account() -> AccountWithMetadata {
    let mut caller = uninit(AccountId::new([0x13u8; 32]));
    caller.is_authorized = true;
    caller
}

fn clock_account(timestamp: u64) -> AccountWithMetadata {
    clock_account_with_id(CLOCK_01_PROGRAM_ACCOUNT_ID, timestamp)
}

fn clock_account_with_id(account_id: AccountId, timestamp: u64) -> AccountWithMetadata {
    account(
        account_id,
        CLOCK_PROGRAM_ID,
        Data::try_from(
            ClockAccountData {
                block_id: 1,
                timestamp,
            }
            .to_bytes(),
        )
        .expect("clock data fits"),
        false,
    )
}

fn malformed_clock_account() -> AccountWithMetadata {
    account(
        CLOCK_01_PROGRAM_ACCOUNT_ID,
        CLOCK_PROGRAM_ID,
        Data::try_from(vec![0xff; 3]).expect("malformed clock bytes fit"),
        false,
    )
}

fn collateral_definition_account() -> AccountWithMetadata {
    collateral_definition_account_with_id(collateral_definition_id())
}

fn collateral_definition_account_with_id(account_id: AccountId) -> AccountWithMetadata {
    account(
        account_id,
        TOKEN_PROGRAM_ID,
        Data::from(&TokenDefinition::Fungible {
            name: "COL".to_owned(),
            total_supply: 1_000_000,
            metadata_id: None,
            authority: None,
        }),
        false,
    )
}

fn user_collateral_holding(balance: u128) -> AccountWithMetadata {
    account(
        user_collateral_holding_id(),
        TOKEN_PROGRAM_ID,
        Data::from(&TokenHolding::Fungible {
            definition_id: collateral_definition_id(),
            balance,
        }),
        true,
    )
}

fn destination_holding() -> AccountWithMetadata {
    account(
        destination_holding_id(),
        TOKEN_PROGRAM_ID,
        Data::from(&TokenHolding::Fungible {
            definition_id: collateral_definition_id(),
            balance: 0,
        }),
        false,
    )
}

fn stablecoin_definition_account(total_supply: u128) -> AccountWithMetadata {
    stablecoin_definition_account_with_id(
        compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID),
        total_supply,
    )
}

fn stablecoin_definition_account_with_id(
    account_id: AccountId,
    total_supply: u128,
) -> AccountWithMetadata {
    account(
        account_id,
        TOKEN_PROGRAM_ID,
        Data::from(&TokenDefinition::Fungible {
            name: "STBL".to_owned(),
            total_supply,
            metadata_id: None,
            authority: None,
        }),
        false,
    )
}

fn user_stablecoin_holding(balance: u128) -> AccountWithMetadata {
    user_stablecoin_holding_with_definition(
        compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID),
        balance,
    )
}

fn user_stablecoin_holding_with_definition(
    definition_id: AccountId,
    balance: u128,
) -> AccountWithMetadata {
    account(
        user_stablecoin_holding_id(),
        TOKEN_PROGRAM_ID,
        Data::from(&TokenHolding::Fungible {
            definition_id,
            balance,
        }),
        true,
    )
}

fn protocol_parameters(is_frozen: bool) -> ProtocolParameters {
    ProtocolParameters {
        admin_account_id: admin_id(),
        freeze_authority_account_id: freeze_authority_id(),
        stablecoin_definition_id: compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID),
        collateral_definition_id: collateral_definition_id(),
        market_price_oracle_id: oracle_id(),
        stability_fee_per_millisecond: FIXED_POINT_ONE,
        controller_proportional_gain: 0,
        controller_integral_gain: 0,
        minimum_collateralization_ratio: FIXED_POINT_ONE / 10 * 11,
        minimum_milliseconds_between_rate_updates: 1,
        maximum_oracle_price_age_milliseconds: 86_400_000,
        is_frozen,
    }
}

fn protocol_parameters_account(is_frozen: bool) -> AccountWithMetadata {
    protocol_parameters_account_with_id(
        compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID),
        is_frozen,
    )
}

fn protocol_parameters_account_with_id(
    account_id: AccountId,
    is_frozen: bool,
) -> AccountWithMetadata {
    account(
        account_id,
        STABLECOIN_PROGRAM_ID,
        Data::from(&protocol_parameters(is_frozen)),
        false,
    )
}

fn stability_fee_accumulator_account(anchor: u128, last_accrued_at: u64) -> AccountWithMetadata {
    account(
        compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID),
        STABLECOIN_PROGRAM_ID,
        Data::from(&StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: anchor,
            last_accrued_at,
        }),
        false,
    )
}

fn redemption_price_state_account(price: u128, last_updated_at: u64) -> AccountWithMetadata {
    account(
        compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID),
        STABLECOIN_PROGRAM_ID,
        Data::from(&RedemptionPriceState {
            redemption_price_at_last_update: price,
            redemption_rate_per_millisecond: FIXED_POINT_ONE,
            controller_integral_term: 0,
            last_updated_at,
        }),
        false,
    )
}

fn oracle_account(timestamp: u64) -> AccountWithMetadata {
    oracle_account_with_assets(
        compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID),
        collateral_definition_id(),
        timestamp,
    )
}

fn oracle_account_with_assets(
    base_asset: AccountId,
    quote_asset: AccountId,
    timestamp: u64,
) -> AccountWithMetadata {
    account(
        oracle_id(),
        ORACLE_PROGRAM_ID,
        Data::from(&OraclePriceAccount {
            base_asset,
            quote_asset,
            price: 1u128 << 64,
            timestamp,
            source_id: AccountId::new([0x71u8; 32]),
            confidence_interval: 0,
        }),
        false,
    )
}

fn position_account(collateral_amount: u128, normalized_debt_amount: u128) -> AccountWithMetadata {
    account(
        position_id(),
        STABLECOIN_PROGRAM_ID,
        Data::from(&Position {
            owner_account_id: owner_id(),
            position_nonce: POSITION_NONCE,
            vault_account_id: vault_id(),
            collateral_amount,
            normalized_debt_amount,
            opened_at: 1_000,
        }),
        false,
    )
}

fn vault_account() -> AccountWithMetadata {
    vault_account_with_definition(collateral_definition_id())
}

fn vault_account_with_definition(definition_id: AccountId) -> AccountWithMetadata {
    account(
        vault_id(),
        TOKEN_PROGRAM_ID,
        Data::from(&TokenHolding::Fungible {
            definition_id,
            balance: 0,
        }),
        false,
    )
}

fn decode_position(post_state: &AccountPostState) -> Position {
    Position::try_from(&post_state.account().data).expect("post state must hold Position")
}

fn decode_accumulator(post_state: &AccountPostState) -> StabilityFeeAccumulator {
    StabilityFeeAccumulator::try_from(&post_state.account().data)
        .expect("post state must hold StabilityFeeAccumulator")
}

#[test]
fn initialize_program_creates_globals_and_stablecoin_definition_call() {
    let (post_states, chained_calls) = crate::initialize_program::initialize_program(
        admin_account(),
        uninit(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        collateral_definition_account(),
        oracle_account(1_000),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        freeze_authority_id(),
        FIXED_POINT_ONE,
        0,
        0,
        FIXED_POINT_ONE / 10 * 11,
        1,
        86_400_000,
        FIXED_POINT_ONE,
        "STBL".to_owned(),
    );

    assert_eq!(post_states.len(), 9);
    assert_eq!(
        post_states[1].required_claim(),
        Some(Claim::Pda(compute_protocol_parameters_pda_seed()))
    );
    assert_eq!(
        post_states[2].required_claim(),
        Some(Claim::Pda(compute_stability_fee_accumulator_pda_seed()))
    );
    assert_eq!(
        post_states[3].required_claim(),
        Some(Claim::Pda(compute_redemption_price_state_pda_seed()))
    );
    let params = ProtocolParameters::try_from(&post_states[1].account().data)
        .expect("valid ProtocolParameters");
    assert_eq!(params.admin_account_id, admin_id());
    let accumulator = decode_accumulator(&post_states[2]);
    assert_eq!(
        accumulator.accumulated_rate_at_last_accrual,
        FIXED_POINT_ONE
    );
    assert_eq!(accumulator.last_accrued_at, 1_000);

    let mut stablecoin_definition =
        uninit(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID));
    stablecoin_definition.is_authorized = true;
    let stablecoin_definition_id = stablecoin_definition.account_id;
    let mut stablecoin_master =
        uninit(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID));
    stablecoin_master.is_authorized = true;
    let expected = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![stablecoin_definition, stablecoin_master],
        &token_core::Instruction::NewFungibleDefinition {
            name: "STBL".to_owned(),
            total_supply: 0,
            mint_authority: Some(stablecoin_definition_id),
        },
    )
    .with_pda_seeds(vec![
        compute_stablecoin_definition_pda_seed(),
        compute_stablecoin_master_holding_pda_seed(),
    ]);
    assert_eq!(chained_calls, vec![expected]);
}

#[test]
#[should_panic(expected = "Admin authorization is missing")]
fn initialize_program_rejects_unauthorized_admin() {
    let mut admin = admin_account();
    admin.is_authorized = false;

    crate::initialize_program::initialize_program(
        admin,
        uninit(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        collateral_definition_account(),
        oracle_account(1_000),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        freeze_authority_id(),
        FIXED_POINT_ONE,
        0,
        0,
        FIXED_POINT_ONE / 10 * 11,
        1,
        86_400_000,
        FIXED_POINT_ONE,
        "STBL".to_owned(),
    );
}

#[test]
#[should_panic(expected = "Clock account must be the canonical 1-block LEZ clock account")]
fn initialize_program_rejects_wrong_clock_account() {
    crate::initialize_program::initialize_program(
        admin_account(),
        uninit(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        collateral_definition_account(),
        oracle_account(1_000),
        clock_account_with_id(CLOCK_10_PROGRAM_ACCOUNT_ID, 1_000),
        STABLECOIN_PROGRAM_ID,
        freeze_authority_id(),
        FIXED_POINT_ONE,
        0,
        0,
        FIXED_POINT_ONE / 10 * 11,
        1,
        86_400_000,
        FIXED_POINT_ONE,
        "STBL".to_owned(),
    );
}

#[test]
#[should_panic(expected = "Stability fee per millisecond is out of bounds")]
fn initialize_program_rejects_stability_fee_below_one() {
    crate::initialize_program::initialize_program(
        admin_account(),
        uninit(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        collateral_definition_account(),
        oracle_account(1_000),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        freeze_authority_id(),
        FIXED_POINT_ONE - 1,
        0,
        0,
        FIXED_POINT_ONE / 10 * 11,
        1,
        86_400_000,
        FIXED_POINT_ONE,
        "STBL".to_owned(),
    );
}

#[test]
#[should_panic(expected = "Protocol parameters account must be uninitialized")]
fn initialize_program_rejects_initialized_protocol_parameters_pda() {
    crate::initialize_program::initialize_program(
        admin_account(),
        protocol_parameters_account(false),
        uninit(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        collateral_definition_account(),
        oracle_account(1_000),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        freeze_authority_id(),
        FIXED_POINT_ONE,
        0,
        0,
        FIXED_POINT_ONE / 10 * 11,
        1,
        86_400_000,
        FIXED_POINT_ONE,
        "STBL".to_owned(),
    );
}

#[test]
#[should_panic(expected = "Collateral definition account must be initialized")]
fn initialize_program_rejects_uninitialized_collateral_definition() {
    crate::initialize_program::initialize_program(
        admin_account(),
        uninit(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        uninit(collateral_definition_id()),
        oracle_account(1_000),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        freeze_authority_id(),
        FIXED_POINT_ONE,
        0,
        0,
        FIXED_POINT_ONE / 10 * 11,
        1,
        86_400_000,
        FIXED_POINT_ONE,
        "STBL".to_owned(),
    );
}

#[test]
#[should_panic(expected = "Market price oracle account must be initialized")]
fn initialize_program_rejects_uninitialized_market_price_oracle() {
    crate::initialize_program::initialize_program(
        admin_account(),
        uninit(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        collateral_definition_account(),
        uninit(oracle_id()),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        freeze_authority_id(),
        FIXED_POINT_ONE,
        0,
        0,
        FIXED_POINT_ONE / 10 * 11,
        1,
        86_400_000,
        FIXED_POINT_ONE,
        "STBL".to_owned(),
    );
}

#[test]
#[should_panic(expected = "Market price oracle quote asset must be the collateral definition")]
fn initialize_program_rejects_oracle_quote_mismatch() {
    crate::initialize_program::initialize_program(
        admin_account(),
        uninit(compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        uninit(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        collateral_definition_account(),
        oracle_account_with_assets(
            compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID),
            other_collateral_definition_id(),
            1_000,
        ),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        freeze_authority_id(),
        FIXED_POINT_ONE,
        0,
        0,
        FIXED_POINT_ONE / 10 * 11,
        1,
        86_400_000,
        FIXED_POINT_ONE,
        "STBL".to_owned(),
    );
}

#[test]
fn accrue_stability_fee_rolls_anchor_forward() {
    let mut params = protocol_parameters(false);
    params.stability_fee_per_millisecond = FIXED_POINT_ONE + FIXED_POINT_ONE / 10;
    let protocol = account(
        compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID),
        STABLECOIN_PROGRAM_ID,
        Data::from(&params),
        false,
    );
    let accumulator = stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000);

    let (post_states, chained_calls) = crate::accrue_stability_fee::accrue_stability_fee(
        caller_account(),
        protocol,
        accumulator,
        clock_account(1_002),
        STABLECOIN_PROGRAM_ID,
    );

    assert!(chained_calls.is_empty());
    let updated = decode_accumulator(&post_states[2]);
    let expected_factor = compound_rate(params.stability_fee_per_millisecond, 2);
    assert_eq!(updated.accumulated_rate_at_last_accrual, expected_factor);
    assert_eq!(updated.last_accrued_at, 1_002);
}

#[test]
fn accrue_stability_fee_clamps_elapsed_window() {
    let mut params = protocol_parameters(false);
    params.stability_fee_per_millisecond = FIXED_POINT_ONE + 1;
    let protocol = account(
        compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID),
        STABLECOIN_PROGRAM_ID,
        Data::from(&params),
        false,
    );

    let (post_states, chained_calls) = crate::accrue_stability_fee::accrue_stability_fee(
        caller_account(),
        protocol,
        stability_fee_accumulator_account(FIXED_POINT_ONE, 0),
        clock_account(MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS + 1),
        STABLECOIN_PROGRAM_ID,
    );

    assert!(chained_calls.is_empty());
    let updated = decode_accumulator(&post_states[2]);
    assert_eq!(
        updated.accumulated_rate_at_last_accrual,
        compound_rate(
            params.stability_fee_per_millisecond,
            MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS
        )
    );
    assert_eq!(
        updated.last_accrued_at,
        MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS + 1
    );
}

#[test]
#[should_panic(expected = "Caller authorization is missing")]
fn accrue_stability_fee_rejects_unauthorized_caller() {
    let mut caller = caller_account();
    caller.is_authorized = false;

    crate::accrue_stability_fee::accrue_stability_fee(
        caller,
        protocol_parameters_account(false),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Clock account must be initialized")]
fn accrue_stability_fee_rejects_uninitialized_clock() {
    crate::accrue_stability_fee::accrue_stability_fee(
        caller_account(),
        protocol_parameters_account(false),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        uninit(CLOCK_01_PROGRAM_ACCOUNT_ID),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Clock account must hold valid ClockAccountData")]
fn accrue_stability_fee_rejects_malformed_clock_data() {
    crate::accrue_stability_fee::accrue_stability_fee(
        caller_account(),
        protocol_parameters_account(false),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        malformed_clock_account(),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
fn set_stability_fee_accrues_old_rate_before_update() {
    let mut params = protocol_parameters(false);
    params.stability_fee_per_millisecond = FIXED_POINT_ONE + FIXED_POINT_ONE / 10;
    let protocol = account(
        compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID),
        STABLECOIN_PROGRAM_ID,
        Data::from(&params),
        false,
    );

    let (post_states, _chained_calls) =
        crate::set_stability_fee_per_millisecond::set_stability_fee_per_millisecond(
            admin_account(),
            protocol,
            stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
            clock_account(1_002),
            STABLECOIN_PROGRAM_ID,
            FIXED_POINT_ONE,
        );

    let updated_params = ProtocolParameters::try_from(&post_states[1].account().data)
        .expect("valid ProtocolParameters");
    assert_eq!(
        updated_params.stability_fee_per_millisecond,
        FIXED_POINT_ONE
    );
    let updated_accumulator = decode_accumulator(&post_states[2]);
    assert_eq!(
        updated_accumulator.accumulated_rate_at_last_accrual,
        compound_rate(FIXED_POINT_ONE + FIXED_POINT_ONE / 10, 2)
    );
}

#[test]
#[should_panic(expected = "Admin account does not match protocol admin")]
fn set_stability_fee_rejects_non_admin() {
    crate::set_stability_fee_per_millisecond::set_stability_fee_per_millisecond(
        owner_account(),
        protocol_parameters_account(false),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        FIXED_POINT_ONE,
    );
}

#[test]
#[should_panic(expected = "Stability fee per millisecond is out of bounds")]
fn set_stability_fee_rejects_rate_below_one() {
    crate::set_stability_fee_per_millisecond::set_stability_fee_per_millisecond(
        admin_account(),
        protocol_parameters_account(false),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        FIXED_POINT_ONE - 1,
    );
}

#[test]
#[should_panic(expected = "Stability fee per millisecond is out of bounds")]
fn set_stability_fee_rejects_rate_above_safe_maximum() {
    crate::set_stability_fee_per_millisecond::set_stability_fee_per_millisecond(
        admin_account(),
        protocol_parameters_account(false),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        MAX_STABILITY_FEE_PER_MILLISECOND + 1,
    );
}

#[test]
fn open_position_stores_normalized_position_and_emits_token_calls() {
    let (post_states, chained_calls) = crate::open_position::open_position(
        owner_account(),
        uninit(position_id()),
        uninit(vault_id()),
        user_collateral_holding(1_000),
        collateral_definition_account(),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        POSITION_NONCE,
        500,
    );

    let position = decode_position(&post_states[1]);
    assert_eq!(
        position,
        Position {
            owner_account_id: owner_id(),
            position_nonce: POSITION_NONCE,
            vault_account_id: vault_id(),
            collateral_amount: 500,
            normalized_debt_amount: 0,
            opened_at: 1_000,
        }
    );
    assert_eq!(
        post_states[1].required_claim(),
        Some(Claim::Pda(compute_position_pda_seed(
            owner_id(),
            POSITION_NONCE
        )))
    );
    assert_eq!(chained_calls.len(), 2);
}

#[test]
#[should_panic(expected = "Protocol is frozen; opening positions is disabled")]
fn open_position_rejects_frozen_protocol() {
    crate::open_position::open_position(
        owner_account(),
        uninit(position_id()),
        uninit(vault_id()),
        user_collateral_holding(1_000),
        collateral_definition_account(),
        protocol_parameters_account(true),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        POSITION_NONCE,
        500,
    );
}

#[test]
#[should_panic(expected = "Protocol parameters account ID does not match PDA")]
fn open_position_rejects_wrong_protocol_parameters_pda() {
    crate::open_position::open_position(
        owner_account(),
        uninit(position_id()),
        uninit(vault_id()),
        user_collateral_holding(1_000),
        collateral_definition_account(),
        protocol_parameters_account_with_id(AccountId::new([0x90u8; 32]), false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        POSITION_NONCE,
        500,
    );
}

#[test]
#[should_panic(expected = "Collateral definition does not match protocol parameters")]
fn open_position_rejects_wrong_collateral_definition() {
    crate::open_position::open_position(
        owner_account(),
        uninit(position_id()),
        uninit(vault_id()),
        user_collateral_holding(1_000),
        collateral_definition_account_with_id(other_collateral_definition_id()),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        POSITION_NONCE,
        500,
    );
}

#[test]
#[should_panic(expected = "Clock account must be the canonical 1-block LEZ clock account")]
fn open_position_rejects_wrong_clock_account() {
    crate::open_position::open_position(
        owner_account(),
        uninit(position_id()),
        uninit(vault_id()),
        user_collateral_holding(1_000),
        collateral_definition_account(),
        protocol_parameters_account(false),
        clock_account_with_id(CLOCK_10_PROGRAM_ACCOUNT_ID, 1_000),
        STABLECOIN_PROGRAM_ID,
        POSITION_NONCE,
        500,
    );
}

#[test]
fn generate_debt_increases_normalized_debt_and_mints_exact_amount() {
    let amount = 100;
    let (post_states, chained_calls) = crate::generate_debt::generate_debt(
        owner_account(),
        position_account(1_000, 0),
        stablecoin_definition_account(0),
        user_stablecoin_holding(0),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        redemption_price_state_account(FIXED_POINT_ONE, 1_000),
        oracle_account(1_000),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        amount,
    );

    let position = decode_position(&post_states[1]);
    assert_eq!(position.normalized_debt_amount, amount);
    let mut stablecoin_definition = stablecoin_definition_account(0);
    stablecoin_definition.is_authorized = true;
    let expected_mint = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![stablecoin_definition, user_stablecoin_holding(0)],
        &token_core::Instruction::Mint {
            amount_to_mint: amount,
        },
    )
    .with_pda_seeds(vec![compute_stablecoin_definition_pda_seed()]);
    assert_eq!(chained_calls, vec![expected_mint]);
}

#[test]
#[should_panic(expected = "Protocol is frozen; debt generation is disabled")]
fn generate_debt_rejects_frozen_protocol() {
    crate::generate_debt::generate_debt(
        owner_account(),
        position_account(1_000, 0),
        stablecoin_definition_account(0),
        user_stablecoin_holding(0),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        redemption_price_state_account(FIXED_POINT_ONE, 1_000),
        oracle_account(1_000),
        protocol_parameters_account(true),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Stablecoin definition does not match protocol parameters")]
fn generate_debt_rejects_wrong_stablecoin_definition() {
    crate::generate_debt::generate_debt(
        owner_account(),
        position_account(1_000, 0),
        stablecoin_definition_account_with_id(other_stablecoin_definition_id(), 0),
        user_stablecoin_holding(0),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        redemption_price_state_account(FIXED_POINT_ONE, 1_000),
        oracle_account(1_000),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Market price oracle account must be initialized")]
fn generate_debt_rejects_uninitialized_market_price_oracle() {
    crate::generate_debt::generate_debt(
        owner_account(),
        position_account(1_000, 0),
        stablecoin_definition_account(0),
        user_stablecoin_holding(0),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        redemption_price_state_account(FIXED_POINT_ONE, 1_000),
        uninit(oracle_id()),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Market price oracle timestamp is in the future")]
fn generate_debt_rejects_future_market_price_oracle() {
    crate::generate_debt::generate_debt(
        owner_account(),
        position_account(1_000, 0),
        stablecoin_definition_account(0),
        user_stablecoin_holding(0),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        redemption_price_state_account(FIXED_POINT_ONE, 1_000),
        oracle_account(1_001),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
fn repay_debt_uses_floor_rounding_against_current_accumulator() {
    let accumulator = FIXED_POINT_ONE + FIXED_POINT_ONE / 10;

    let (post_states, chained_calls) = crate::repay_debt::repay_debt(
        owner_account(),
        position_account(1_000, 100),
        stablecoin_definition_account(100),
        user_stablecoin_holding(100),
        stability_fee_accumulator_account(accumulator, 1_000),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        11,
    );

    let position = decode_position(&post_states[1]);
    assert_eq!(position.normalized_debt_amount, 90);
    assert_eq!(chained_calls.len(), 1);
}

#[test]
#[should_panic(expected = "Repay amount is too small to reduce outstanding debt")]
fn repay_debt_rejects_nonzero_amount_that_rounds_to_zero() {
    crate::repay_debt::repay_debt(
        owner_account(),
        position_account(1_000, 100),
        stablecoin_definition_account(100),
        user_stablecoin_holding(100),
        stability_fee_accumulator_account(FIXED_POINT_ONE * 2, 1_000),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        1,
    );
}

#[test]
#[should_panic(expected = "Repay amount exceeds outstanding debt")]
fn repay_debt_rejects_amount_above_current_debt_ceiling() {
    let accumulator = FIXED_POINT_ONE + FIXED_POINT_ONE / 10;

    crate::repay_debt::repay_debt(
        owner_account(),
        position_account(1_000, 100),
        stablecoin_definition_account(100),
        user_stablecoin_holding(111),
        stability_fee_accumulator_account(accumulator, 1_000),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        111,
    );
}

#[test]
#[should_panic(expected = "Stablecoin holding does not match the provided stablecoin definition")]
fn repay_debt_rejects_wrong_stablecoin_holding_definition() {
    crate::repay_debt::repay_debt(
        owner_account(),
        position_account(1_000, 100),
        stablecoin_definition_account(100),
        user_stablecoin_holding_with_definition(other_stablecoin_definition_id(), 100),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        10,
    );
}

#[test]
fn withdraw_collateral_allows_safe_withdrawal_and_rejects_unsafe_withdrawal() {
    let safe = crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        position_account(600, 500),
        vault_account(),
        destination_holding(),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        redemption_price_state_account(FIXED_POINT_ONE, 1_000),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        50,
    );
    let position = decode_position(&safe.0[1]);
    assert_eq!(position.collateral_amount, 550);

    let result = std::panic::catch_unwind(|| {
        crate::withdraw_collateral::withdraw_collateral(
            owner_account(),
            position_account(600, 500),
            vault_account(),
            destination_holding(),
            stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
            redemption_price_state_account(FIXED_POINT_ONE, 1_000),
            protocol_parameters_account(false),
            clock_account(1_000),
            STABLECOIN_PROGRAM_ID,
            51,
        );
    });
    assert!(result.is_err());
}

#[test]
#[should_panic(expected = "Protocol is frozen; collateral withdrawal is disabled")]
fn withdraw_collateral_rejects_frozen_protocol() {
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        position_account(600, 500),
        vault_account(),
        destination_holding(),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        redemption_price_state_account(FIXED_POINT_ONE, 1_000),
        protocol_parameters_account(true),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        50,
    );
}

#[test]
#[should_panic(expected = "Vault token holding does not match protocol collateral definition")]
fn withdraw_collateral_rejects_wrong_vault_collateral_definition() {
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        position_account(600, 500),
        vault_account_with_definition(other_collateral_definition_id()),
        destination_holding(),
        stability_fee_accumulator_account(FIXED_POINT_ONE, 1_000),
        redemption_price_state_account(FIXED_POINT_ONE, 1_000),
        protocol_parameters_account(false),
        clock_account(1_000),
        STABLECOIN_PROGRAM_ID,
        50,
    );
}
