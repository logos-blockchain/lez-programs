//! Host-side implementation of `Instruction::UpdateRedemptionRate` (spec §10.3).
//!
//! Wall-clock time comes from the system `CLOCK_01` account, same as the other
//! pokes.

use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{
    compute_redemption_price_state_pda, controller::run_controller_tick,
    math::compute_current_redemption_price, ControllerOutput, ProtocolParameters,
    RedemptionPriceState,
};
use twap_oracle_core::OraclePriceAccount;

use crate::accrue_stability_fee::read_clock;

/// Run one tick of the redemption-rate controller and re-anchor the redemption
/// price.
///
/// Permissionless but strict: unlike [`crate::refresh_globals`], every gate here
/// is hard. It panics when called before
/// `minimum_milliseconds_between_rate_updates` has elapsed, and when the oracle
/// is stale or reports a zero price — a keeper may want that as an explicit
/// failure signal rather than a silent skip. Never blocked by the frozen flag.
///
/// See spec §10.3 for the full account contract and panic conditions.
#[allow(clippy::needless_pass_by_value)]
pub fn update_redemption_rate(
    caller: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    redemption_price_state: AccountWithMetadata,
    market_price_oracle: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(caller.is_authorized, "Caller authorization is missing");

    let (params, redemption) = decode_redemption_inputs(
        &protocol_parameters,
        &redemption_price_state,
        stablecoin_program_id,
    );
    let oracle = decode_oracle(&market_price_oracle, &params);
    let now = read_clock(&clock);

    // Hard gates. `refresh_globals` treats these same three conditions as soft
    // skips; here they are failures (spec §10.3 vs §10.3a).
    assert!(
        now.saturating_sub(oracle.timestamp) <= params.maximum_oracle_price_age_milliseconds,
        "Market price oracle observation is stale"
    );
    assert!(oracle.price > 0, "Market price oracle reports zero price");
    assert!(
        now.saturating_sub(redemption.last_updated_at)
            >= params.minimum_milliseconds_between_rate_updates,
        "update_redemption_rate called too soon since last update"
    );

    let redemption_post = advance_redemption_price(
        &redemption_price_state,
        &params,
        &redemption,
        oracle.price,
        now,
    );

    let post_states = vec![
        AccountPostState::new(caller.account),
        AccountPostState::new(protocol_parameters.account),
        AccountPostState::new(redemption_post),
        AccountPostState::new(market_price_oracle.account),
        AccountPostState::new(clock.account),
    ];

    (post_states, vec![])
}

/// Validate and decode the two accounts the redemption half needs.
///
/// Shared with [`crate::refresh_globals`] so the combined poke can never drift
/// from the standalone one.
pub(crate) fn decode_redemption_inputs(
    protocol_parameters: &AccountWithMetadata,
    redemption_price_state: &AccountWithMetadata,
    stablecoin_program_id: ProgramId,
) -> (ProtocolParameters, RedemptionPriceState) {
    assert_ne!(
        protocol_parameters.account,
        Account::default(),
        "ProtocolParameters account must be initialized"
    );
    assert_eq!(
        protocol_parameters.account.program_owner, stablecoin_program_id,
        "ProtocolParameters not owned by this stablecoin program"
    );
    assert_ne!(
        redemption_price_state.account,
        Account::default(),
        "RedemptionPriceState account must be initialized"
    );
    assert_eq!(
        redemption_price_state.account.program_owner, stablecoin_program_id,
        "RedemptionPriceState not owned by this stablecoin program"
    );
    assert_eq!(
        redemption_price_state.account_id,
        compute_redemption_price_state_pda(stablecoin_program_id),
        "RedemptionPriceState account ID does not match expected PDA derivation"
    );

    let params = ProtocolParameters::try_from(&protocol_parameters.account.data)
        .expect("ProtocolParameters must decode");
    let redemption = RedemptionPriceState::try_from(&redemption_price_state.account.data)
        .expect("RedemptionPriceState must decode");

    (params, redemption)
}

/// Validate the oracle account and decode its observation.
///
/// The oracle's `account_id` must equal
/// `ProtocolParameters.market_price_oracle_id`. Its `program_owner` is
/// deliberately NOT pinned, so a future aggregator or an alternative producer
/// can replace the TWAP oracle without a code change here. Shared with
/// [`crate::refresh_globals`], where the id mismatch is likewise a hard panic.
pub(crate) fn decode_oracle(
    market_price_oracle: &AccountWithMetadata,
    params: &ProtocolParameters,
) -> OraclePriceAccount {
    assert_ne!(
        market_price_oracle.account,
        Account::default(),
        "Market price oracle account must be initialized"
    );
    assert_eq!(
        market_price_oracle.account_id, params.market_price_oracle_id,
        "Market price oracle account_id does not match ProtocolParameters.market_price_oracle_id"
    );
    OraclePriceAccount::try_from(&market_price_oracle.account.data)
        .expect("Market price oracle must decode as OraclePriceAccount")
}

/// Project the redemption price to `now`, run one controller tick against
/// `market_price`, and return the re-anchored account.
///
/// Shared with [`crate::refresh_globals`] — the redemption half is identical in
/// both. The projection deliberately runs BEFORE the tick and uses the OLD rate;
/// the tick then produces the NEW rate that later reads compound against.
pub(crate) fn advance_redemption_price(
    redemption_price_state: &AccountWithMetadata,
    params: &ProtocolParameters,
    redemption: &RedemptionPriceState,
    market_price: u128,
    now: u64,
) -> Account {
    let milliseconds_elapsed = now.saturating_sub(redemption.last_updated_at);
    let current_price = compute_current_redemption_price(
        redemption.redemption_price_at_last_update,
        redemption.redemption_rate_per_millisecond,
        redemption.last_updated_at,
        now,
    );

    let ControllerOutput {
        redemption_rate_per_millisecond,
        controller_integral_term,
    } = run_controller_tick(
        current_price,
        market_price,
        redemption.controller_integral_term,
        params.controller_proportional_gain,
        params.controller_integral_gain,
        milliseconds_elapsed,
    );

    let mut redemption_post = redemption_price_state.account.clone();
    redemption_post.data = Data::from(&RedemptionPriceState {
        redemption_price_at_last_update: current_price,
        redemption_rate_per_millisecond,
        controller_integral_term,
        last_updated_at: now,
    });
    redemption_post
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
    use stablecoin_core::math::FIXED_POINT_ONE;

    use super::*;
    use crate::test_support::{
        caller_account, clock_account, oracle_account, protocol_parameters_account,
        redemption_price_state_account, redemption_price_state_account_with_integral_term,
        uninitialized, ParameterOverrides, MARKET_PRICE_BELOW_ANCHOR, NOW, REDEMPTION_PRICE_ANCHOR,
        STABLECOIN_PROGRAM_ID, T0,
    };

    fn invoke() -> (Vec<AccountPostState>, Vec<ChainedCall>) {
        update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            redemption_price_state_account(T0),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        )
    }

    #[test]
    fn happy_path_returns_five_post_states_and_no_chained_calls() {
        let (post_states, chained_calls) = invoke();
        assert_eq!(post_states.len(), 5);
        assert!(chained_calls.is_empty());
    }

    #[test]
    fn happy_path_re_anchors_with_the_projected_price_and_controller_output() {
        let (post_states, _) = invoke();
        let decoded =
            RedemptionPriceState::try_from(&post_states[2].account().data).expect("decode");

        let projected =
            compute_current_redemption_price(REDEMPTION_PRICE_ANCHOR, FIXED_POINT_ONE, T0, NOW);
        let expected = run_controller_tick(
            projected,
            MARKET_PRICE_BELOW_ANCHOR,
            0,
            FIXED_POINT_ONE as i128,
            0,
            NOW - T0,
        );

        assert_eq!(decoded.redemption_price_at_last_update, projected);
        assert_eq!(
            decoded.redemption_rate_per_millisecond,
            expected.redemption_rate_per_millisecond
        );
        assert_eq!(
            decoded.controller_integral_term,
            expected.controller_integral_term
        );
        assert_eq!(decoded.last_updated_at, NOW);
    }

    #[test]
    fn market_below_redemption_drives_the_rate_above_one() {
        // error = redemption − market > 0 with a positive Kp: the rate rises, so
        // the redemption price climbs and pulls the market up (negative feedback).
        let (post_states, _) = invoke();
        let decoded = RedemptionPriceState::try_from(&post_states[2].account().data).unwrap();
        assert!(decoded.redemption_rate_per_millisecond > FIXED_POINT_ONE);
    }

    #[test]
    fn market_above_redemption_drives_the_rate_below_one() {
        let (post_states, _) = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            redemption_price_state_account(T0),
            oracle_account(NOW, REDEMPTION_PRICE_ANCHOR * 2),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
        let decoded = RedemptionPriceState::try_from(&post_states[2].account().data).unwrap();
        assert!(decoded.redemption_rate_per_millisecond < FIXED_POINT_ONE);
    }

    #[test]
    fn carries_the_prior_integral_term_forward() {
        let standing = 12_345_678_i128;
        let (post_states, _) = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides {
                controller_proportional_gain: 0,
                controller_integral_gain: 0,
                ..ParameterOverrides::default()
            }),
            redemption_price_state_account_with_integral_term(T0, standing),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
        let decoded = RedemptionPriceState::try_from(&post_states[2].account().data).unwrap();
        // Ki = 0 means nothing is integrated this tick, so the term is unchanged.
        assert_eq!(decoded.controller_integral_term, standing);
    }

    #[test]
    fn happy_path_claims_no_pda_and_echoes_oracle_and_clock() {
        let (post_states, _) = invoke();
        assert_eq!(post_states[2].required_claim(), None);
        assert_eq!(
            post_states[3].account(),
            &oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR).account
        );
        assert_eq!(post_states[4].account(), &clock_account(NOW).account);
    }

    #[test]
    fn update_is_allowed_while_frozen() {
        let (post_states, _) = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides {
                is_frozen: true,
                ..ParameterOverrides::default()
            }),
            redemption_price_state_account(T0),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
        let decoded = RedemptionPriceState::try_from(&post_states[2].account().data).unwrap();
        assert_eq!(decoded.last_updated_at, NOW);
    }

    #[test]
    #[should_panic(expected = "Caller authorization is missing")]
    fn requires_caller_authorization() {
        let mut caller = caller_account();
        caller.is_authorized = false;
        let _ = update_redemption_rate(
            caller,
            protocol_parameters_account(ParameterOverrides::default()),
            redemption_price_state_account(T0),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "ProtocolParameters account must be initialized")]
    fn rejects_uninitialized_protocol_parameters() {
        let _ = update_redemption_rate(
            caller_account(),
            uninitialized(stablecoin_core::compute_protocol_parameters_pda(
                STABLECOIN_PROGRAM_ID,
            )),
            redemption_price_state_account(T0),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "RedemptionPriceState account must be initialized")]
    fn rejects_uninitialized_redemption_price_state() {
        let _ = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            uninitialized(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "RedemptionPriceState not owned by this stablecoin program")]
    fn rejects_foreign_owned_redemption_price_state() {
        let mut state = redemption_price_state_account(T0);
        state.account.program_owner = [9u32; 8];
        let _ = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            state,
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(
        expected = "RedemptionPriceState account ID does not match expected PDA derivation"
    )]
    fn rejects_wrong_redemption_price_state_pda() {
        let mut state = redemption_price_state_account(T0);
        state.account_id = AccountId::new([0xDE; 32]);
        let _ = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            state,
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Market price oracle account must be initialized")]
    fn rejects_uninitialized_oracle() {
        let _ = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            redemption_price_state_account(T0),
            uninitialized(crate::test_support::oracle_id()),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(
        expected = "Market price oracle account_id does not match ProtocolParameters.market_price_oracle_id"
    )]
    fn rejects_oracle_id_mismatch() {
        let mut oracle = oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR);
        oracle.account_id = AccountId::new([0xDE; 32]);
        let _ = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            redemption_price_state_account(T0),
            oracle,
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Market price oracle observation is stale")]
    fn rejects_stale_oracle() {
        // 1_000_000 ms old > the 900_000 ms maximum age.
        let _ = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            redemption_price_state_account(T0),
            oracle_account(NOW - 1_000_000, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Market price oracle reports zero price")]
    fn rejects_zero_oracle_price() {
        let _ = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            redemption_price_state_account(T0),
            oracle_account(NOW, 0),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "update_redemption_rate called too soon since last update")]
    fn rejects_call_before_the_minimum_interval() {
        // 100_000 ms since the last update < the 300_000 ms minimum.
        let _ = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            redemption_price_state_account(NOW - 100_000),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
    fn rejects_wrong_clock_account() {
        let mut clock = clock_account(NOW);
        clock.account_id = AccountId::new([0xC1; 32]);
        let _ = update_redemption_rate(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            redemption_price_state_account(T0),
            oracle_account(NOW, MARKET_PRICE_BELOW_ANCHOR),
            clock,
            STABLECOIN_PROGRAM_ID,
        );
    }
}
