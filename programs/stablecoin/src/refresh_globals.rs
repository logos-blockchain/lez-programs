//! Host-side implementation of `Instruction::RefreshGlobals` (spec §10.3a).
//!
//! Best-effort combined poke: ALWAYS advances the stability-fee accumulator
//! (§10.2); advances the redemption price (§10.3 / §6.4) ONLY when its interval
//! is due and the oracle is fresh and non-zero, skipping rather than panicking
//! otherwise. A LEZ transaction carries exactly one instruction, so this is the
//! only way to advance both globals in a single transaction.

use nssa_core::{
    account::AccountWithMetadata,
    program::{AccountPostState, ChainedCall, ProgramId},
};

use crate::{
    accrue_stability_fee::{advance_fee_accumulator, decode_fee_accrual_inputs, read_clock},
    update_redemption_rate::{advance_redemption_price, decode_oracle, decode_redemption_inputs},
};

/// Advance both globals in one instruction, best-effort.
///
/// Every piece of math here is the shared helper the standalone pokes call, so
/// the combined path can never drift from them.
///
/// Panics ONLY on caller authorization, an uninitialized / foreign-owned /
/// wrong-PDA global, an oracle id mismatch, or a wrong clock account. A
/// not-yet-due interval and a stale or zero-price oracle are SOFT — they skip
/// the redemption half instead of failing the transaction. Never blocked by the
/// frozen flag.
///
/// See spec §10.3a for the full account contract.
#[allow(clippy::needless_pass_by_value)]
pub fn refresh_globals(
    caller: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    redemption_price_state: AccountWithMetadata,
    market_price_oracle: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(caller.is_authorized, "Caller authorization is missing");

    let (params, accumulator) = decode_fee_accrual_inputs(
        &protocol_parameters,
        &stability_fee_accumulator,
        stablecoin_program_id,
    );
    let (_, redemption) = decode_redemption_inputs(
        &protocol_parameters,
        &redemption_price_state,
        stablecoin_program_id,
    );
    let oracle = decode_oracle(&market_price_oracle, &params);
    let now = read_clock(&clock);

    // Fee half — always runs. It has no throttle and needs no oracle, so it
    // makes progress even during an oracle outage.
    let accumulator_post =
        advance_fee_accumulator(&stability_fee_accumulator, &params, &accumulator, now);

    // Redemption half — three soft gates. Failing any one of them leaves the
    // redemption state untouched rather than panicking (the asymmetry with
    // `update_redemption_rate` is intentional; spec §10.3a).
    let interval_due = now.saturating_sub(redemption.last_updated_at)
        >= params.minimum_milliseconds_between_rate_updates;
    let oracle_fresh =
        now.saturating_sub(oracle.timestamp) <= params.maximum_oracle_price_age_milliseconds;
    let oracle_price_usable = oracle.price > 0;

    let redemption_post = if interval_due && oracle_fresh && oracle_price_usable {
        advance_redemption_price(
            &redemption_price_state,
            &params,
            &redemption,
            oracle.price,
            now,
        )
    } else {
        redemption_price_state.account.clone()
    };

    let post_states = vec![
        AccountPostState::new(caller.account),
        AccountPostState::new(protocol_parameters.account),
        AccountPostState::new(accumulator_post),
        AccountPostState::new(redemption_post),
        AccountPostState::new(market_price_oracle.account),
        AccountPostState::new(clock.account),
    ];

    (post_states, vec![])
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests deliberately panic on bad state via assert!/#[should_panic] and index fixed-size vectors"
)]
mod tests {
    use nssa_core::account::AccountId;
    use stablecoin_core::{
        compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
        math::FIXED_POINT_ONE, RedemptionPriceState, StabilityFeeAccumulator,
    };

    use super::*;
    use crate::test_support::{
        accumulator_account, caller_account, clock_account, oracle_account,
        protocol_parameters_account, redemption_price_state_account, uninitialized,
        ParameterOverrides, ACCUMULATOR_ANCHOR, MARKET_PRICE_BELOW_ANCHOR, NOW,
        STABLECOIN_PROGRAM_ID, T0,
    };

    fn decode_accumulator(post: &AccountPostState) -> StabilityFeeAccumulator {
        StabilityFeeAccumulator::try_from(&post.account().data).expect("decode accumulator")
    }

    fn decode_redemption(post: &AccountPostState) -> RedemptionPriceState {
        RedemptionPriceState::try_from(&post.account().data).expect("decode redemption state")
    }

    /// The default call: interval due, oracle fresh and non-zero.
    fn invoke_with(
        overrides: ParameterOverrides,
        redemption_last_updated_at: u64,
        oracle_timestamp: u64,
        oracle_price: u128,
    ) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
        refresh_globals(
            caller_account(),
            protocol_parameters_account(overrides),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            redemption_price_state_account(redemption_last_updated_at),
            oracle_account(oracle_timestamp, oracle_price),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        )
    }

    fn invoke() -> (Vec<AccountPostState>, Vec<ChainedCall>) {
        invoke_with(
            ParameterOverrides::default(),
            T0,
            NOW,
            MARKET_PRICE_BELOW_ANCHOR,
        )
    }

    #[test]
    fn both_halves_run_when_interval_due_and_oracle_fresh() {
        let (post_states, chained_calls) = invoke();
        assert_eq!(post_states.len(), 6);
        assert!(chained_calls.is_empty());

        let accumulator = decode_accumulator(&post_states[2]);
        assert!(accumulator.accumulated_rate_at_last_accrual > ACCUMULATOR_ANCHOR);
        assert_eq!(accumulator.last_accrued_at, NOW);

        // Redemption > market with a positive Kp, so the rate rises above 1.0.
        let redemption = decode_redemption(&post_states[3]);
        assert!(redemption.redemption_rate_per_millisecond > FIXED_POINT_ONE);
        assert_eq!(redemption.last_updated_at, NOW);
    }

    #[test]
    fn both_halves_match_the_standalone_pokes_exactly() {
        // The combined poke must produce byte-identical state to running the two
        // standalone pokes; they share the same helpers, and this pins it.
        let (combined, _) = invoke();

        let (accrue_only, _) = crate::accrue_stability_fee::accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
        let (update_only, _) = crate::update_redemption_rate::update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            redemption_price_state_account(T0),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );

        assert_eq!(combined[2].account(), accrue_only[2].account());
        assert_eq!(combined[3].account(), update_only[2].account());
    }

    #[test]
    fn redemption_half_skips_when_interval_not_elapsed_but_fee_still_runs() {
        // 100_000 ms since the last update < the 300_000 ms minimum.
        let recent = NOW - 100_000;
        let (post_states, _) = invoke_with(
            ParameterOverrides::default(),
            recent,
            NOW,
            MARKET_PRICE_BELOW_ANCHOR,
        );

        assert_eq!(decode_accumulator(&post_states[2]).last_accrued_at, NOW);

        let redemption = decode_redemption(&post_states[3]);
        assert_eq!(redemption.redemption_rate_per_millisecond, FIXED_POINT_ONE);
        assert_eq!(redemption.last_updated_at, recent);
    }

    #[test]
    fn redemption_half_skips_when_oracle_is_stale_but_fee_still_runs() {
        // 1_000_000 ms old > the 900_000 ms maximum age.
        let (post_states, _) = invoke_with(
            ParameterOverrides::default(),
            T0,
            NOW - 1_000_000,
            MARKET_PRICE_BELOW_ANCHOR,
        );

        assert_eq!(decode_accumulator(&post_states[2]).last_accrued_at, NOW);

        let redemption = decode_redemption(&post_states[3]);
        assert_eq!(redemption.redemption_rate_per_millisecond, FIXED_POINT_ONE);
        assert_eq!(redemption.last_updated_at, T0);
    }

    #[test]
    fn redemption_half_skips_when_oracle_price_is_zero_but_fee_still_runs() {
        let (post_states, _) = invoke_with(ParameterOverrides::default(), T0, NOW, 0);

        assert_eq!(decode_accumulator(&post_states[2]).last_accrued_at, NOW);
        assert_eq!(decode_redemption(&post_states[3]).last_updated_at, T0);
    }

    #[test]
    fn a_skipped_redemption_half_leaves_the_account_byte_identical() {
        let (post_states, _) = invoke_with(ParameterOverrides::default(), T0, NOW, 0);
        assert_eq!(
            post_states[3].account(),
            &redemption_price_state_account(T0).account
        );
    }

    #[test]
    fn both_halves_run_while_frozen() {
        // Pokes are never blocked by the frozen flag (spec §10.3a); the function
        // does not read `is_frozen` at all.
        let (post_states, _) = invoke_with(
            ParameterOverrides {
                is_frozen: true,
                ..ParameterOverrides::default()
            },
            T0,
            NOW,
            MARKET_PRICE_BELOW_ANCHOR,
        );
        assert_eq!(decode_accumulator(&post_states[2]).last_accrued_at, NOW);
        assert_eq!(decode_redemption(&post_states[3]).last_updated_at, NOW);
    }

    #[test]
    fn echoes_the_oracle_and_clock_unchanged_and_claims_no_pda() {
        let (post_states, _) = invoke();
        assert_eq!(post_states[2].required_claim(), None);
        assert_eq!(post_states[3].required_claim(), None);
        assert_eq!(
            post_states[4].account(),
            &oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR).account
        );
        assert_eq!(post_states[5].account(), &clock_account(NOW).account);
    }

    #[test]
    #[should_panic(expected = "Caller authorization is missing")]
    fn requires_caller_authorization() {
        let mut caller = caller_account();
        caller.is_authorized = false;
        let _ = refresh_globals(
            caller,
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            redemption_price_state_account(T0),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "ProtocolParameters account must be initialized")]
    fn rejects_uninitialized_protocol_parameters() {
        let _ = refresh_globals(
            caller_account(),
            uninitialized(stablecoin_core::compute_protocol_parameters_pda(
                STABLECOIN_PROGRAM_ID,
            )),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            redemption_price_state_account(T0),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "StabilityFeeAccumulator account must be initialized")]
    fn rejects_uninitialized_accumulator() {
        let _ = refresh_globals(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            uninitialized(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)),
            redemption_price_state_account(T0),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "RedemptionPriceState account must be initialized")]
    fn rejects_uninitialized_redemption_price_state() {
        let _ = refresh_globals(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            uninitialized(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(
        expected = "StabilityFeeAccumulator account ID does not match expected PDA derivation"
    )]
    fn rejects_wrong_accumulator_pda() {
        let mut accumulator = accumulator_account(ACCUMULATOR_ANCHOR, T0);
        accumulator.account_id = AccountId::new([0xDE; 32]);
        let _ = refresh_globals(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator,
            redemption_price_state_account(T0),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(
        expected = "Market price oracle account_id does not match ProtocolParameters.market_price_oracle_id"
    )]
    fn rejects_oracle_id_mismatch() {
        // A wrong oracle account is a caller error, not a transient condition —
        // so unlike staleness, this one panics here too.
        let mut oracle = oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR);
        oracle.account_id = AccountId::new([0xDE; 32]);
        let _ = refresh_globals(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            redemption_price_state_account(T0),
            oracle,
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
    fn rejects_wrong_clock_account() {
        let mut clock = clock_account(NOW);
        clock.account_id = AccountId::new([0xC1; 32]);
        let _ = refresh_globals(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            redemption_price_state_account(T0),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock,
            STABLECOIN_PROGRAM_ID,
        );
    }
}
