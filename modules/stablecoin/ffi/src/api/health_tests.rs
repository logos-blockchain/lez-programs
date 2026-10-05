use alloy_primitives::U512;
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use stablecoin_core::{
    compute_position_pda, compute_position_vault_pda, compute_protocol_parameters_pda,
    compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
    math::{
        compute_current_accumulated_rate_wide, compute_current_redemption_price_wide,
        try_project_rate_wide, FIXED_POINT_ONE, MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
    },
    Position, ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator, RATE_DELTA_CLAMP,
};

use super::{position_health, PositionHealthRequest};
use crate::{
    account::{account_id_hex, account_read, program_id_bytes},
    AccountRead,
};

const PROGRAM: ProgramId = [0x11; 8];
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
                opened_at: u64::MAX,
            },
            parameters: ProtocolParameters {
                admin_account_id: id(1),
                freeze_authority_account_id: id(2),
                stablecoin_definition_id: id(3),
                collateral_definition_id: id(4),
                market_price_oracle_id: id(5),
                stability_fee_per_millisecond: FIXED_POINT_ONE,
                controller_proportional_gain: 0,
                controller_integral_gain: 0,
                minimum_collateralization_ratio: FIXED_POINT_ONE * 11 / 10,
                minimum_milliseconds_between_rate_updates: 50,
                maximum_oracle_price_age_milliseconds: 50,
                is_frozen: true,
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
            now: START,
        }
    }

    fn request(&self) -> PositionHealthRequest {
        PositionHealthRequest {
            stablecoin_program_id: hex::encode(program_id_bytes(PROGRAM)),
            owner_id: account_id_hex(id(20)),
            position_nonce: NONCE.to_string(),
            position: read(
                compute_position_pda(PROGRAM, id(20), NONCE),
                PROGRAM,
                Data::from(&self.position),
            ),
            protocol_parameters: read(
                compute_protocol_parameters_pda(PROGRAM),
                PROGRAM,
                Data::from(&self.parameters),
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

    fn native_is_healthy(&self) -> bool {
        // Catching the assertion is confined to this test oracle. Production
        // quote flow uses the shared non-panicking comparison directly.
        std::panic::catch_unwind(|| {
            stablecoin_program::checks::assert_position_is_collateralized(
                &self.position,
                compute_current_accumulated_rate_wide(
                    self.accumulator.accumulated_rate_at_last_accrual,
                    self.parameters.stability_fee_per_millisecond,
                    self.accumulator.last_accrued_at,
                    self.now,
                ),
                compute_current_redemption_price_wide(
                    self.redemption.redemption_price_at_last_update,
                    self.redemption.redemption_rate_per_millisecond,
                    self.redemption.last_updated_at,
                    self.now,
                ),
                self.parameters.minimum_collateralization_ratio,
            )
        })
        .is_ok()
    }
}

fn error(request: PositionHealthRequest, expected: &str) {
    assert_eq!(
        position_health(request).expect_err("invalid quote").code(),
        expected
    );
}

#[test]
fn health_matches_native_zero_surplus_equality_and_one_below_boundary() {
    for (collateral, debt, healthy) in [
        (0, 0, true),
        (1_000, 100, true),
        (110, 100, true),
        (109, 100, false),
    ] {
        let fixture = Fixture::new(collateral, debt);
        let quote = position_health(fixture.request()).expect("quote");
        assert_eq!(quote["isCollateralized"], healthy);
        assert_eq!(fixture.native_is_healthy(), healthy);
        assert_eq!(
            quote["collateralValue"],
            (U512::from(collateral) * U512::from(FIXED_POINT_ONE).pow(U512::from(3))).to_string()
        );
        assert_eq!(quote["requirementSaturated"], false);
    }
}

#[test]
fn fractional_debt_display_rounding_never_changes_health() {
    let mut fixture = Fixture::new(2, 1);
    fixture.accumulator.accumulated_rate_at_last_accrual = FIXED_POINT_ONE * 19 / 10;
    let quote = position_health(fixture.request()).expect("quote");
    assert_eq!(quote["nominalDebt"], "1");
    assert_eq!(quote["isCollateralized"], false);
    assert!(!fixture.native_is_healthy());
    // The old floor-first comparison would report 2 >= 1 * 1.1 as healthy.
    assert_eq!(
        quote["requiredCollateralValue"],
        (U512::from(FIXED_POINT_ONE * 19 / 10)
            * U512::from(FIXED_POINT_ONE)
            * U512::from(FIXED_POINT_ONE * 11 / 10))
        .to_string()
    );
}

#[test]
fn health_preserves_maximum_fields_and_base58_owner_without_float_values() {
    let fixture = Fixture::new(u128::MAX, u128::MAX);
    let mut request = fixture.request();
    request.owner_id = id(20).to_string();
    let quote = position_health(request).expect("quote");
    assert_eq!(quote["positionNonce"], NONCE.to_string());
    assert_eq!(quote["openedAt"], u64::MAX.to_string());
    assert_eq!(quote["collateralAmount"], u128::MAX.to_string());
    assert_eq!(quote["normalizedDebtAmount"], u128::MAX.to_string());
    assert_eq!(quote["nominalDebt"], u128::MAX.to_string());
    assert_eq!(quote["ownerIdHex"], account_id_hex(id(20)));
    assert_eq!(
        quote["positionIdHex"],
        account_id_hex(compute_position_pda(PROGRAM, id(20), NONCE))
    );
    assert_eq!(
        quote["vaultIdHex"],
        account_id_hex(fixture.position.vault_account_id)
    );
    for value in quote.as_object().expect("response object").values() {
        assert!(value.is_string() || value.is_boolean());
    }
}

#[test]
fn health_reads_live_fees_and_allows_exact_projections_above_u128() {
    let mut fixture = Fixture::new(110, 100);
    assert_eq!(
        position_health(fixture.request()).expect("quote")["isCollateralized"],
        true
    );
    fixture.parameters.stability_fee_per_millisecond = FIXED_POINT_ONE + 1_500_000_000_000_000;
    fixture.now = START + 1;
    assert_eq!(
        position_health(fixture.request()).expect("quote")["isCollateralized"],
        false
    );
    assert!(!fixture.native_is_healthy());

    let mut fixture = Fixture::new(u128::MAX, 1);
    fixture.redemption.redemption_price_at_last_update = u128::MAX;
    fixture.redemption.redemption_rate_per_millisecond =
        FIXED_POINT_ONE + RATE_DELTA_CLAMP.unsigned_abs();
    fixture.now = START + 1;
    let quote = position_health(fixture.request()).expect("wide projection");
    let price: U512 = quote["currentRedemptionPrice"]
        .as_str()
        .expect("decimal")
        .parse()
        .expect("wide integer");
    assert!(price > U512::from(u128::MAX));
    assert_eq!(quote["isCollateralized"], fixture.native_is_healthy());
}

#[test]
fn saturated_requirement_is_explicit_and_conservatively_unhealthy() {
    let mut fixture = Fixture::new(u128::MAX, 10);
    fixture.redemption.redemption_rate_per_millisecond =
        FIXED_POINT_ONE + RATE_DELTA_CLAMP.unsigned_abs();
    fixture.now = START + 18_000_000;
    let quote = position_health(fixture.request())
        .expect("exact wide projection with saturated requirement");
    assert_eq!(quote["requiredCollateralValue"], U512::MAX.to_string());
    assert_eq!(quote["requirementSaturated"], true);
    assert_eq!(quote["isCollateralized"], false);
    assert!(!fixture.native_is_healthy());
}

#[test]
fn health_projection_and_display_overflow_return_stable_errors_without_panics() {
    let mut fixture = Fixture::new(u128::MAX, 10);
    fixture.redemption.redemption_rate_per_millisecond =
        FIXED_POINT_ONE + RATE_DELTA_CLAMP.unsigned_abs();
    fixture.now = START + 60_000_000;
    error(fixture.request(), "health_projection_overflow");
    let mut fixture = Fixture::new(u128::MAX, u128::MAX);
    fixture.parameters.stability_fee_per_millisecond = FIXED_POINT_ONE * 2;
    fixture.now = START + 310;
    error(fixture.request(), "health_arithmetic_overflow");
}

#[test]
fn health_clamps_long_gaps_and_saturates_inverted_timestamps_like_the_program() {
    let mut fixture = Fixture::new(110, 100);
    fixture.parameters.stability_fee_per_millisecond = FIXED_POINT_ONE + 1_500_000_000_000_000;
    fixture.now = START - 1;
    assert_eq!(
        position_health(fixture.request()).expect("inverted clock")["currentAccumulatedRate"],
        FIXED_POINT_ONE.to_string()
    );
    fixture.now = START + MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS;
    let at_clamp = position_health(fixture.request()).expect("clamp");
    fixture.now += 1;
    let beyond = position_health(fixture.request()).expect("clamp");
    assert_eq!(
        at_clamp["currentAccumulatedRate"],
        beyond["currentAccumulatedRate"]
    );
    assert_eq!(beyond["isCollateralized"], fixture.native_is_healthy());
    assert_eq!(
        try_project_rate_wide(
            FIXED_POINT_ONE,
            fixture.parameters.stability_fee_per_millisecond,
            START,
            fixture.now
        ),
        Some(compute_current_accumulated_rate_wide(
            FIXED_POINT_ONE,
            fixture.parameters.stability_fee_per_millisecond,
            START,
            fixture.now
        ))
    );
}

#[test]
fn health_checks_position_identity_and_rejects_malformed_nonce_strings() {
    let fixture = Fixture::new(110, 100);
    let mut invalid = fixture.request();
    invalid.position.id = account_id_hex(id(90));
    error(invalid, "position_pda_mismatch");
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
        let mut invalid = fixture.request();
        invalid.position.account.as_mut().expect("account").data =
            hex::encode(Data::from(&state).as_ref());
        error(invalid, expected);
    }
    for nonce in ["", "1.0", "1e3", "-1", "18446744073709551616"] {
        let mut invalid = fixture.request();
        invalid.position_nonce = nonce.to_owned();
        error(invalid, "invalid_numeric_value");
    }
    let mut invalid = fixture.request();
    invalid.owner_id = "0".repeat(64);
    error(invalid, "invalid_account_id");
}

#[test]
fn health_validates_every_live_account_identity_owner_and_exact_data() {
    let fixture = Fixture::new(110, 100);
    let expected_data_errors = [
        "invalid_position_data",
        "invalid_protocol_parameters_data",
        "invalid_stability_fee_accumulator_data",
        "invalid_redemption_price_state_data",
        "invalid_clock",
    ];
    let expected_pda_errors = [
        "position_pda_mismatch",
        "protocol_parameters_pda_mismatch",
        "stability_fee_accumulator_pda_mismatch",
        "redemption_price_state_pda_mismatch",
        "invalid_clock",
    ];
    for index in 0..5 {
        for mutation in 0..5 {
            if index == 4 && mutation == 4 {
                continue;
            } // The canonical clock is identified by ID, not program owner.
            let mut request = fixture.request();
            let reads = [
                &mut request.position,
                &mut request.protocol_parameters,
                &mut request.stability_fee_accumulator,
                &mut request.redemption_price_state,
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
                    read.account.as_mut().expect("account").data.pop();
                    expected_data_errors[index]
                }
                2 => {
                    read.account.as_mut().expect("account").data.push_str("ff");
                    expected_data_errors[index]
                }
                3 => {
                    read.id = account_id_hex(id(90));
                    expected_pda_errors[index]
                }
                _ => {
                    read.account.as_mut().expect("account").program_owner =
                        hex::encode(program_id_bytes([0x99; 8]));
                    "stablecoin_program_mismatch"
                }
            };
            // Removing an entire byte keeps the transport valid and makes only
            // the typed account data truncated, rather than malformed hex.
            if mutation == 1 {
                read.account.as_mut().expect("account").data.pop();
            }
            error(request, expected);
        }
    }
}
