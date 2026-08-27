use serde_json::{json, Value};
use stablecoin_core::{
    math::compute_current_redemption_price, run_controller_tick, INTEGRAL_CLAMP, RATE_DELTA_CLAMP,
};
use twap_oracle_core::OraclePriceAccount;

use super::{
    decode::{validated_protocol_parameters, validated_redemption_price_state},
    parse_stablecoin_program_id,
    projection::clock_timestamp,
    RedemptionRateUpdateQuoteRequest, StablecoinApiError, StablecoinResult,
};
use crate::account::decode_account;

pub fn redemption_rate_update_quote(request: RedemptionRateUpdateQuoteRequest) -> StablecoinResult {
    let stablecoin_program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let (_, parameters) =
        validated_protocol_parameters(stablecoin_program_id, &request.protocol_parameters)?;
    let (_, redemption) =
        validated_redemption_price_state(stablecoin_program_id, &request.redemption_price_state)?;
    let oracle = validated_market_price_oracle(
        &request.market_price_oracle,
        parameters.market_price_oracle_id,
    )?;
    let now = clock_timestamp(&request.clock)?;

    let elapsed_milliseconds = now.saturating_sub(redemption.last_updated_at);
    let oracle_age_milliseconds = now.saturating_sub(oracle.timestamp);
    let current_redemption_price = compute_current_redemption_price(
        redemption.redemption_price_at_last_update,
        redemption.redemption_rate_per_millisecond,
        redemption.last_updated_at,
        now,
    );

    let mut errors = Vec::new();
    if oracle_age_milliseconds > parameters.maximum_oracle_price_age_milliseconds {
        errors.push(blocker(
            "oracle_stale",
            json!({
                "oracleAgeMilliseconds": oracle_age_milliseconds.to_string(),
                "maximumOraclePriceAgeMilliseconds":
                    parameters.maximum_oracle_price_age_milliseconds.to_string(),
            }),
        ));
    }
    if oracle.price == 0 {
        errors.push(blocker("oracle_price_zero", json!({"marketPrice": "0"})));
    }
    if elapsed_milliseconds < parameters.minimum_milliseconds_between_rate_updates {
        errors.push(blocker(
            "rate_update_too_soon",
            json!({
                "elapsedMilliseconds": elapsed_milliseconds.to_string(),
                "minimumMillisecondsBetweenRateUpdates":
                    parameters.minimum_milliseconds_between_rate_updates.to_string(),
            }),
        ));
    }

    let can_submit = errors.is_empty();
    let (next_rate, next_integral) = if can_submit {
        let output = run_controller_tick(
            current_redemption_price,
            oracle.price,
            redemption.controller_integral_term,
            parameters.controller_proportional_gain,
            parameters.controller_integral_gain,
            elapsed_milliseconds,
        );
        (
            Value::String(output.redemption_rate_per_millisecond.to_string()),
            Value::String(output.controller_integral_term.to_string()),
        )
    } else {
        (Value::Null, Value::Null)
    };

    Ok(json!({
        "canSubmit": can_submit,
        "code": if can_submit { "ready" } else { "blocked" },
        "currentRedemptionPrice": current_redemption_price.to_string(),
        "marketPrice": oracle.price.to_string(),
        "elapsedMilliseconds": elapsed_milliseconds.to_string(),
        "nextRedemptionRatePerMillisecond": next_rate,
        "nextControllerIntegralTerm": next_integral,
        "clampMetadata": {
            "integralMinimum": (-INTEGRAL_CLAMP).to_string(),
            "integralMaximum": INTEGRAL_CLAMP.to_string(),
            "rateDeltaMinimum": (-RATE_DELTA_CLAMP).to_string(),
            "rateDeltaMaximum": RATE_DELTA_CLAMP.to_string(),
        },
        "errors": errors,
        "warnings": [],
    }))
}

pub(super) fn validated_market_price_oracle(
    read: &crate::AccountRead,
    expected_id: lee_core::account::AccountId,
) -> Result<OraclePriceAccount, StablecoinApiError> {
    let (account_id, account) =
        decode_account(read).map_err(|_| StablecoinApiError::new("account_read_failed"))?;
    if account_id != expected_id {
        return Err(StablecoinApiError::new("market_price_oracle_mismatch"));
    }
    OraclePriceAccount::try_from(&account.data)
        .map_err(|_| StablecoinApiError::new("invalid_market_price_oracle"))
}

fn blocker(code: &'static str, details: Value) -> Value {
    json!({
        "code": code,
        "recoverable": true,
        "blockingFields": [],
        "details": details,
    })
}

#[cfg(test)]
mod tests {
    use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
    use lee_core::{
        account::{Account, AccountId, Data, Nonce},
        program::ProgramId,
    };
    use stablecoin_core::{
        compute_protocol_parameters_pda, compute_redemption_price_state_pda,
        math::{FIXED_POINT_ONE, MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS},
        ProtocolParameters, RedemptionPriceState,
    };

    use super::*;
    use crate::account::{account_id_hex, account_read, program_id_bytes};

    const STABLECOIN_PROGRAM_ID: ProgramId = [0x11_u32; 8];
    const ORACLE_PROGRAM_ID: ProgramId = [0x22_u32; 8];
    const CLOCK_PROGRAM_ID: ProgramId = [0x33_u32; 8];

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

    fn parameters(oracle_id: AccountId) -> ProtocolParameters {
        ProtocolParameters {
            admin_account_id: id(1),
            freeze_authority_account_id: id(2),
            stablecoin_definition_id: id(3),
            collateral_definition_id: id(4),
            market_price_oracle_id: oracle_id,
            stability_fee_per_millisecond: FIXED_POINT_ONE,
            controller_proportional_gain: FIXED_POINT_ONE as i128,
            controller_integral_gain: 0,
            minimum_collateralization_ratio: FIXED_POINT_ONE,
            minimum_milliseconds_between_rate_updates: 1,
            maximum_oracle_price_age_milliseconds: 1,
            is_frozen: false,
        }
    }

    fn redemption(price: u128, integral: i128, last_updated_at: u64) -> RedemptionPriceState {
        RedemptionPriceState {
            redemption_price_at_last_update: price,
            redemption_rate_per_millisecond: FIXED_POINT_ONE,
            controller_integral_term: integral,
            last_updated_at,
        }
    }

    fn oracle(oracle_id: AccountId, price: u128, timestamp: u64) -> (AccountId, Account) {
        (
            oracle_id,
            account(
                ORACLE_PROGRAM_ID,
                Data::from(&OraclePriceAccount {
                    base_asset: id(8),
                    quote_asset: id(9),
                    price,
                    timestamp,
                    source_id: id(10),
                    confidence_interval: 0,
                }),
            ),
        )
    }

    fn request(
        parameters: &ProtocolParameters,
        redemption: &RedemptionPriceState,
        oracle: &(AccountId, Account),
        now: u64,
    ) -> RedemptionRateUpdateQuoteRequest {
        let clock = ClockAccountData {
            block_id: 1,
            timestamp: now,
        };
        RedemptionRateUpdateQuoteRequest {
            stablecoin_program_id: hex::encode(program_id_bytes(STABLECOIN_PROGRAM_ID)),
            protocol_parameters: account_read(
                compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID),
                &account(STABLECOIN_PROGRAM_ID, Data::from(parameters)),
            ),
            redemption_price_state: account_read(
                compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID),
                &account(STABLECOIN_PROGRAM_ID, Data::from(redemption)),
            ),
            market_price_oracle: account_read(oracle.0, &oracle.1),
            clock: account_read(
                CLOCK_01_PROGRAM_ACCOUNT_ID,
                &account(
                    CLOCK_PROGRAM_ID,
                    Data::try_from(clock.to_bytes()).expect("clock data fits"),
                ),
            ),
        }
    }

    fn value(request: RedemptionRateUpdateQuoteRequest) -> Value {
        redemption_rate_update_quote(request).expect("quote succeeds")
    }

    fn assert_error(request: RedemptionRateUpdateQuoteRequest, expected: &str) {
        let error = redemption_rate_update_quote(request).expect_err("quote must fail");
        assert_eq!(error.code(), expected);
    }

    #[test]
    fn ready_quotes_match_controller_for_all_error_and_gain_modes() {
        let oracle_id = id(5);
        let cases = [
            (
                FIXED_POINT_ONE * 2,
                FIXED_POINT_ONE,
                0,
                FIXED_POINT_ONE as i128,
                0,
                10,
            ),
            (
                FIXED_POINT_ONE,
                FIXED_POINT_ONE,
                0,
                FIXED_POINT_ONE as i128,
                0,
                10,
            ),
            (
                FIXED_POINT_ONE,
                FIXED_POINT_ONE * 2,
                0,
                FIXED_POINT_ONE as i128,
                0,
                10,
            ),
            (
                FIXED_POINT_ONE * 2,
                FIXED_POINT_ONE,
                17,
                (FIXED_POINT_ONE / 10) as i128,
                (FIXED_POINT_ONE / 100) as i128,
                10,
            ),
            (
                FIXED_POINT_ONE,
                FIXED_POINT_ONE,
                INTEGRAL_CLAMP / 4,
                0,
                FIXED_POINT_ONE as i128,
                10,
            ),
        ];

        for (price, market_price, integral, proportional_gain, integral_gain, elapsed) in cases {
            let mut params = parameters(oracle_id);
            params.controller_proportional_gain = proportional_gain;
            params.controller_integral_gain = integral_gain;
            let state = redemption(price, integral, 100);
            let observation = oracle(oracle_id, market_price, 100 + elapsed);
            let quoted = value(request(&params, &state, &observation, 100 + elapsed));
            let expected = run_controller_tick(
                price,
                market_price,
                integral,
                proportional_gain,
                integral_gain,
                elapsed,
            );

            assert_eq!(quoted["canSubmit"], true);
            assert_eq!(quoted["code"], "ready");
            assert_eq!(quoted["currentRedemptionPrice"], price.to_string());
            assert_eq!(quoted["marketPrice"], market_price.to_string());
            assert_eq!(quoted["elapsedMilliseconds"], elapsed.to_string());
            assert_eq!(
                quoted["nextRedemptionRatePerMillisecond"],
                expected.redemption_rate_per_millisecond.to_string()
            );
            assert_eq!(
                quoted["nextControllerIntegralTerm"],
                expected.controller_integral_term.to_string()
            );
            assert_eq!(quoted["errors"], json!([]));
            assert_eq!(quoted["warnings"], json!([]));
        }
    }

    #[test]
    fn quotes_match_both_integral_and_rate_clamps_and_bounded_extremes() {
        let oracle_id = id(5);
        let cases = [
            (
                FIXED_POINT_ONE * 2,
                FIXED_POINT_ONE,
                0,
                0,
                FIXED_POINT_ONE as i128,
                1_000_001,
            ),
            (
                FIXED_POINT_ONE,
                FIXED_POINT_ONE * 2,
                0,
                0,
                FIXED_POINT_ONE as i128,
                1_000_001,
            ),
            (
                FIXED_POINT_ONE * 2,
                FIXED_POINT_ONE,
                0,
                (FIXED_POINT_ONE as i128) * 1_000,
                0,
                1,
            ),
            (
                FIXED_POINT_ONE,
                FIXED_POINT_ONE * 2,
                0,
                (FIXED_POINT_ONE as i128) * 1_000,
                0,
                1,
            ),
            (
                u128::MAX / 2,
                1,
                INTEGRAL_CLAMP,
                (FIXED_POINT_ONE as i128) * 1_000,
                FIXED_POINT_ONE as i128,
                MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
            ),
        ];

        for (price, market_price, integral, proportional_gain, integral_gain, elapsed) in cases {
            let mut params = parameters(oracle_id);
            params.controller_proportional_gain = proportional_gain;
            params.controller_integral_gain = integral_gain;
            params.maximum_oracle_price_age_milliseconds = elapsed;
            let state = redemption(price, integral, 0);
            let observation = oracle(oracle_id, market_price, elapsed);
            let quoted = value(request(&params, &state, &observation, elapsed));
            let expected = run_controller_tick(
                price,
                market_price,
                integral,
                proportional_gain,
                integral_gain,
                elapsed,
            );

            assert_eq!(
                quoted["nextRedemptionRatePerMillisecond"],
                expected.redemption_rate_per_millisecond.to_string()
            );
            assert_eq!(
                quoted["nextControllerIntegralTerm"],
                expected.controller_integral_term.to_string()
            );
        }
    }

    #[test]
    fn quote_uses_raw_elapsed_for_controller_after_clamped_price_projection() {
        let oracle_id = id(5);
        let elapsed = MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS + 123;
        let mut params = parameters(oracle_id);
        params.controller_proportional_gain = 0;
        params.controller_integral_gain = FIXED_POINT_ONE as i128;
        let price = FIXED_POINT_ONE + 1;
        let state = redemption(price, 0, 0);
        let observation = oracle(oracle_id, FIXED_POINT_ONE, elapsed);

        let quoted = value(request(&params, &state, &observation, elapsed));
        let expected = run_controller_tick(
            price,
            FIXED_POINT_ONE,
            0,
            0,
            FIXED_POINT_ONE as i128,
            elapsed,
        );

        assert_eq!(quoted["elapsedMilliseconds"], elapsed.to_string());
        assert_eq!(
            quoted["nextControllerIntegralTerm"],
            expected.controller_integral_term.to_string()
        );
        assert_ne!(
            quoted["nextControllerIntegralTerm"],
            MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS.to_string()
        );
    }

    #[test]
    fn gate_boundaries_and_combined_blockers_match_host_order() {
        let oracle_id = id(5);
        let mut params = parameters(oracle_id);
        params.minimum_milliseconds_between_rate_updates = 10;
        params.maximum_oracle_price_age_milliseconds = 20;

        let exact_state = redemption(FIXED_POINT_ONE, 0, 90);
        let exact_observation = oracle(oracle_id, FIXED_POINT_ONE, 80);
        assert_eq!(
            value(request(&params, &exact_state, &exact_observation, 100))["canSubmit"],
            true
        );

        let early = redemption(FIXED_POINT_ONE, 0, 91);
        let early_quote = value(request(&params, &early, &exact_observation, 100));
        assert_eq!(early_quote["errors"][0]["code"], "rate_update_too_soon");
        assert_eq!(
            early_quote["errors"][0]["details"],
            json!({
                "elapsedMilliseconds": "9",
                "minimumMillisecondsBetweenRateUpdates": "10",
            })
        );

        let stale = oracle(oracle_id, FIXED_POINT_ONE, 79);
        let stale_quote = value(request(&params, &exact_state, &stale, 100));
        assert_eq!(stale_quote["errors"][0]["code"], "oracle_stale");
        assert_eq!(
            stale_quote["errors"][0]["details"],
            json!({
                "oracleAgeMilliseconds": "21",
                "maximumOraclePriceAgeMilliseconds": "20",
            })
        );

        let zero = oracle(oracle_id, 0, 100);
        let zero_quote = value(request(&params, &exact_state, &zero, 100));
        assert_eq!(zero_quote["errors"][0]["code"], "oracle_price_zero");
        assert_eq!(
            zero_quote["errors"][0]["details"],
            json!({"marketPrice": "0"})
        );

        let combined = oracle(oracle_id, 0, 79);
        let combined_quote = value(request(&params, &early, &combined, 100));
        assert_eq!(combined_quote["canSubmit"], false);
        assert_eq!(combined_quote["code"], "blocked");
        assert_eq!(
            combined_quote["errors"]
                .as_array()
                .expect("errors are an array")
                .iter()
                .map(|error| error["code"].as_str().expect("code is a string"))
                .collect::<Vec<_>>(),
            vec!["oracle_stale", "oracle_price_zero", "rate_update_too_soon"]
        );
        for error in combined_quote["errors"]
            .as_array()
            .expect("errors are an array")
        {
            assert_eq!(error["recoverable"], true);
            assert_eq!(error["blockingFields"], json!([]));
        }
        assert!(combined_quote["nextRedemptionRatePerMillisecond"].is_null());
        assert!(combined_quote["nextControllerIntegralTerm"].is_null());
    }

    #[test]
    fn inverted_timestamps_saturate_and_frozen_state_does_not_block() {
        let oracle_id = id(5);
        let params = parameters(oracle_id);
        let future_state = redemption(FIXED_POINT_ONE, 0, 200);
        let future_observation = oracle(oracle_id, FIXED_POINT_ONE, 300);
        let inverted = value(request(&params, &future_state, &future_observation, 100));
        assert_eq!(inverted["elapsedMilliseconds"], "0");
        assert_eq!(inverted["errors"][0]["code"], "rate_update_too_soon");

        let mut frozen = params;
        frozen.is_frozen = true;
        let due_state = redemption(FIXED_POINT_ONE, 0, 99);
        let current_observation = oracle(oracle_id, FIXED_POINT_ONE, 100);
        assert_eq!(
            value(request(&frozen, &due_state, &current_observation, 100))["canSubmit"],
            true
        );
    }

    #[test]
    fn response_preserves_exact_clamp_strings() {
        let oracle_id = id(5);
        let params = parameters(oracle_id);
        let state = redemption(FIXED_POINT_ONE, 0, 0);
        let observation = oracle(oracle_id, FIXED_POINT_ONE, 1);
        let quoted = value(request(&params, &state, &observation, 1));

        assert_eq!(
            quoted["clampMetadata"],
            json!({
                "integralMinimum": (-INTEGRAL_CLAMP).to_string(),
                "integralMaximum": INTEGRAL_CLAMP.to_string(),
                "rateDeltaMinimum": (-RATE_DELTA_CLAMP).to_string(),
                "rateDeltaMaximum": RATE_DELTA_CLAMP.to_string(),
            })
        );
        for key in [
            "currentRedemptionPrice",
            "marketPrice",
            "elapsedMilliseconds",
            "nextRedemptionRatePerMillisecond",
            "nextControllerIntegralTerm",
        ] {
            assert!(quoted[key].is_string(), "{key} must remain an exact string");
        }
    }

    #[test]
    fn quote_validates_global_oracle_and_clock_inputs() {
        let oracle_id = id(5);
        let params = parameters(oracle_id);
        let state = redemption(FIXED_POINT_ONE, 0, 0);
        let observation = oracle(oracle_id, FIXED_POINT_ONE, 1);
        let base = request(&params, &state, &observation, 1);

        let mut invalid_program = base.clone();
        invalid_program.stablecoin_program_id = String::from("not-a-program-id");
        assert_error(invalid_program, "invalid_program_id");

        let mut wrong_parameters_pda = base.clone();
        wrong_parameters_pda.protocol_parameters.id = account_id_hex(id(20));
        assert_error(wrong_parameters_pda, "protocol_parameters_pda_mismatch");

        let mut wrong_parameters_owner = base.clone();
        wrong_parameters_owner
            .protocol_parameters
            .account
            .as_mut()
            .expect("account exists")
            .program_owner = hex::encode(program_id_bytes(ORACLE_PROGRAM_ID));
        assert_error(wrong_parameters_owner, "stablecoin_program_mismatch");

        let mut malformed_parameters = base.clone();
        malformed_parameters
            .protocol_parameters
            .account
            .as_mut()
            .expect("account exists")
            .data = String::from("00");
        assert_error(malformed_parameters, "invalid_protocol_parameters_data");

        let mut wrong_redemption_pda = base.clone();
        wrong_redemption_pda.redemption_price_state.id = account_id_hex(id(21));
        assert_error(wrong_redemption_pda, "redemption_price_state_pda_mismatch");

        let mut wrong_redemption_owner = base.clone();
        wrong_redemption_owner
            .redemption_price_state
            .account
            .as_mut()
            .expect("account exists")
            .program_owner = hex::encode(program_id_bytes(ORACLE_PROGRAM_ID));
        assert_error(wrong_redemption_owner, "stablecoin_program_mismatch");

        let mut malformed_redemption = base.clone();
        malformed_redemption
            .redemption_price_state
            .account
            .as_mut()
            .expect("account exists")
            .data = String::from("00");
        assert_error(malformed_redemption, "invalid_redemption_price_state_data");

        let mut wrong_oracle_id = base.clone();
        wrong_oracle_id.market_price_oracle.id = account_id_hex(id(22));
        assert_error(wrong_oracle_id, "market_price_oracle_mismatch");

        let mut malformed_oracle = base.clone();
        malformed_oracle
            .market_price_oracle
            .account
            .as_mut()
            .expect("account exists")
            .data = String::from("00");
        assert_error(malformed_oracle, "invalid_market_price_oracle");

        let mut failed_read = base.clone();
        failed_read.market_price_oracle.status = String::from("not_found");
        failed_read.market_price_oracle.account = None;
        assert_error(failed_read, "account_read_failed");

        let mut wrong_clock_id = base.clone();
        wrong_clock_id.clock.id = account_id_hex(id(23));
        assert_error(wrong_clock_id, "invalid_clock");

        let mut malformed_clock = base;
        malformed_clock
            .clock
            .account
            .as_mut()
            .expect("account exists")
            .data = String::from("00");
        assert_error(malformed_clock, "invalid_clock");
    }

    #[test]
    fn oracle_owner_and_asset_pair_are_deliberately_not_pinned() {
        let oracle_id = id(5);
        let params = parameters(oracle_id);
        let state = redemption(FIXED_POINT_ONE, 0, 0);
        let mut observation = oracle(oracle_id, FIXED_POINT_ONE, 1);
        observation.1.program_owner = [0xFF_u32; 8];

        let quoted = value(request(&params, &state, &observation, 1));
        assert_eq!(quoted["canSubmit"], true);
    }
}
