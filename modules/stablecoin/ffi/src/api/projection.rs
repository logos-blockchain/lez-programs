use borsh::from_slice;
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use serde_json::json;
use stablecoin_core::math::{compute_current_accumulated_rate, compute_current_redemption_price};

use super::{
    decode::{
        validated_protocol_parameters, validated_redemption_price_state,
        validated_stability_fee_accumulator,
    },
    parse_stablecoin_program_id, CurrentGlobalStateRequest, StablecoinApiError, StablecoinResult,
};
use crate::account::decode_account;

pub fn current_global_state(request: CurrentGlobalStateRequest) -> StablecoinResult {
    let stablecoin_program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let (_, parameters) =
        validated_protocol_parameters(stablecoin_program_id, &request.protocol_parameters)?;
    let (_, accumulator) = validated_stability_fee_accumulator(
        stablecoin_program_id,
        &request.stability_fee_accumulator,
    )?;
    let (_, redemption) =
        validated_redemption_price_state(stablecoin_program_id, &request.redemption_price_state)?;
    let projected_at = clock_timestamp(&request.clock)?;

    let current_accumulated_rate = compute_current_accumulated_rate(
        accumulator.accumulated_rate_at_last_accrual,
        parameters.stability_fee_per_millisecond,
        accumulator.last_accrued_at,
        projected_at,
    );
    let current_redemption_price = compute_current_redemption_price(
        redemption.redemption_price_at_last_update,
        redemption.redemption_rate_per_millisecond,
        redemption.last_updated_at,
        projected_at,
    );

    Ok(json!({
        "accumulatedRateAtLastAccrual":
            accumulator.accumulated_rate_at_last_accrual.to_string(),
        "lastAccruedAt": accumulator.last_accrued_at.to_string(),
        "redemptionPriceAtLastUpdate": redemption.redemption_price_at_last_update.to_string(),
        "lastUpdatedAt": redemption.last_updated_at.to_string(),
        "currentAccumulatedRate": current_accumulated_rate.to_string(),
        "currentRedemptionPrice": current_redemption_price.to_string(),
        "projectedAt": projected_at.to_string(),
    }))
}

pub(super) fn clock_timestamp(read: &crate::AccountRead) -> Result<u64, StablecoinApiError> {
    let (account_id, account) =
        decode_account(read).map_err(|_| StablecoinApiError::new("account_read_failed"))?;
    if account_id != CLOCK_01_PROGRAM_ACCOUNT_ID {
        return Err(StablecoinApiError::new("invalid_clock"));
    }
    let clock = from_slice::<ClockAccountData>(account.data.as_ref())
        .map_err(|_| StablecoinApiError::new("invalid_clock"))?;
    Ok(clock.timestamp)
}

#[cfg(test)]
mod tests {
    use clock_core::ClockAccountData;
    use lee_core::{
        account::{Account, AccountId, Data, Nonce},
        program::ProgramId,
    };
    use stablecoin_core::{
        compute_protocol_parameters_pda, compute_redemption_price_state_pda,
        compute_stability_fee_accumulator_pda,
        math::{FIXED_POINT_ONE, MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS},
        ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator,
    };

    use super::*;
    use crate::account::{account_id_hex, account_read, program_id_bytes};

    const STABLECOIN_PROGRAM_ID: ProgramId = [0x11_u32; 8];
    const OTHER_PROGRAM_ID: ProgramId = [0x22_u32; 8];

    fn id(seed: u8) -> AccountId {
        AccountId::new([seed; 32])
    }

    fn account(owner: ProgramId, data: Data) -> Account {
        Account {
            program_owner: owner,
            balance: 0,
            data,
            nonce: Nonce(0),
        }
    }

    fn request(
        accumulated_rate_at_last_accrual: u128,
        stability_fee_per_millisecond: u128,
        last_accrued_at: u64,
        redemption_price_at_last_update: u128,
        redemption_rate_per_millisecond: u128,
        last_updated_at: u64,
        projected_at: u64,
    ) -> CurrentGlobalStateRequest {
        let parameters = ProtocolParameters {
            admin_account_id: id(1),
            freeze_authority_account_id: id(2),
            stablecoin_definition_id: id(3),
            collateral_definition_id: id(4),
            market_price_oracle_id: id(5),
            stability_fee_per_millisecond,
            controller_proportional_gain: 0,
            controller_integral_gain: 0,
            minimum_collateralization_ratio: FIXED_POINT_ONE,
            minimum_milliseconds_between_rate_updates: 1,
            maximum_oracle_price_age_milliseconds: 1,
            is_frozen: false,
        };
        let accumulator = StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual,
            last_accrued_at,
        };
        let redemption = RedemptionPriceState {
            redemption_price_at_last_update,
            redemption_rate_per_millisecond,
            controller_integral_term: 0,
            last_updated_at,
        };
        let clock = ClockAccountData {
            block_id: 1,
            timestamp: projected_at,
        };

        CurrentGlobalStateRequest {
            stablecoin_program_id: hex::encode(program_id_bytes(STABLECOIN_PROGRAM_ID)),
            protocol_parameters: account_read(
                compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID),
                &account(STABLECOIN_PROGRAM_ID, Data::from(&parameters)),
            ),
            stability_fee_accumulator: account_read(
                compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID),
                &account(STABLECOIN_PROGRAM_ID, Data::from(&accumulator)),
            ),
            redemption_price_state: account_read(
                compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID),
                &account(STABLECOIN_PROGRAM_ID, Data::from(&redemption)),
            ),
            clock: account_read(
                CLOCK_01_PROGRAM_ACCOUNT_ID,
                &account(
                    OTHER_PROGRAM_ID,
                    Data::try_from(clock.to_bytes()).expect("clock data fits"),
                ),
            ),
        }
    }

    fn value(request: CurrentGlobalStateRequest) -> serde_json::Value {
        current_global_state(request).expect("projection succeeds")
    }

    fn assert_error(request: CurrentGlobalStateRequest, expected: &str) {
        let error = current_global_state(request).expect_err("projection must fail");
        assert_eq!(error.code(), expected);
    }

    #[test]
    fn identity_projection_preserves_maximum_anchors_as_exact_strings() {
        let projected = value(request(
            u128::MAX,
            FIXED_POINT_ONE,
            99,
            u128::MAX,
            FIXED_POINT_ONE,
            99,
            99,
        ));

        assert_eq!(
            projected["accumulatedRateAtLastAccrual"],
            u128::MAX.to_string()
        );
        assert_eq!(
            projected["redemptionPriceAtLastUpdate"],
            u128::MAX.to_string()
        );
        assert_eq!(projected["currentAccumulatedRate"], u128::MAX.to_string());
        assert_eq!(projected["currentRedemptionPrice"], u128::MAX.to_string());
        assert_eq!(projected["lastAccruedAt"], "99");
        assert_eq!(projected["lastUpdatedAt"], "99");
        assert_eq!(projected["projectedAt"], "99");
    }

    #[test]
    fn growth_and_decay_match_the_core_projection_helpers() {
        let growth_rate = FIXED_POINT_ONE + 10_u128.pow(20);
        let decay_rate = FIXED_POINT_ONE - 10_u128.pow(20);
        let anchor = FIXED_POINT_ONE * 2;
        let projected = value(request(
            anchor,
            growth_rate,
            100,
            anchor,
            decay_rate,
            100,
            110,
        ));

        assert_eq!(
            projected["currentAccumulatedRate"],
            compute_current_accumulated_rate(anchor, growth_rate, 100, 110).to_string()
        );
        assert_eq!(
            projected["currentRedemptionPrice"],
            compute_current_redemption_price(anchor, decay_rate, 100, 110).to_string()
        );
    }

    #[test]
    fn inverted_timestamps_collapse_both_projections_to_their_anchors() {
        let accumulator_anchor = FIXED_POINT_ONE * 3;
        let redemption_anchor = FIXED_POINT_ONE * 4;
        let projected = value(request(
            accumulator_anchor,
            FIXED_POINT_ONE + 10_u128.pow(20),
            200,
            redemption_anchor,
            FIXED_POINT_ONE - 10_u128.pow(20),
            300,
            100,
        ));

        assert_eq!(
            projected["currentAccumulatedRate"],
            accumulator_anchor.to_string()
        );
        assert_eq!(
            projected["currentRedemptionPrice"],
            redemption_anchor.to_string()
        );
        assert_eq!(projected["projectedAt"], "100");
    }

    #[test]
    fn beyond_the_window_matches_projection_at_the_exact_seven_day_clamp() {
        let rate = FIXED_POINT_ONE + 1;
        let at_clamp = value(request(
            FIXED_POINT_ONE,
            rate,
            0,
            FIXED_POINT_ONE,
            rate,
            0,
            MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
        ));
        let beyond_clamp = value(request(
            FIXED_POINT_ONE,
            rate,
            0,
            FIXED_POINT_ONE,
            rate,
            0,
            u64::MAX,
        ));

        assert_eq!(
            at_clamp["currentAccumulatedRate"],
            beyond_clamp["currentAccumulatedRate"]
        );
        assert_eq!(
            at_clamp["currentRedemptionPrice"],
            beyond_clamp["currentRedemptionPrice"]
        );
        assert_eq!(beyond_clamp["projectedAt"], u64::MAX.to_string());
    }

    #[test]
    fn projection_preserves_global_validation_errors() {
        let mut wrong_pda = request(1, FIXED_POINT_ONE, 1, 1, FIXED_POINT_ONE, 1, 1);
        wrong_pda.protocol_parameters.id = account_id_hex(id(0xAA));
        assert_error(wrong_pda, "protocol_parameters_pda_mismatch");

        let mut wrong_owner = request(1, FIXED_POINT_ONE, 1, 1, FIXED_POINT_ONE, 1, 1);
        wrong_owner
            .stability_fee_accumulator
            .account
            .as_mut()
            .expect("fixture account")
            .program_owner = hex::encode(program_id_bytes(OTHER_PROGRAM_ID));
        assert_error(wrong_owner, "stablecoin_program_mismatch");

        let mut malformed = request(1, FIXED_POINT_ONE, 1, 1, FIXED_POINT_ONE, 1, 1);
        malformed
            .redemption_price_state
            .account
            .as_mut()
            .expect("fixture account")
            .data = String::from("00");
        assert_error(malformed, "invalid_redemption_price_state_data");
    }

    #[test]
    fn projection_rejects_failed_or_noncanonical_clock_reads() {
        let mut failed_read = request(1, FIXED_POINT_ONE, 1, 1, FIXED_POINT_ONE, 1, 1);
        failed_read.clock.status = String::from("backend_error");
        assert_error(failed_read, "account_read_failed");

        let mut wrong_id = request(1, FIXED_POINT_ONE, 1, 1, FIXED_POINT_ONE, 1, 1);
        wrong_id.clock.id = account_id_hex(id(0xCC));
        assert_error(wrong_id, "invalid_clock");

        let mut malformed = request(1, FIXED_POINT_ONE, 1, 1, FIXED_POINT_ONE, 1, 1);
        malformed
            .clock
            .account
            .as_mut()
            .expect("fixture account")
            .data = String::from("00");
        assert_error(malformed, "invalid_clock");
    }
}
