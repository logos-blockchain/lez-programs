//! Host-side implementation of `Instruction::ResetRedemptionRateController`
//! (spec §10.19).

use lee_core::{
    account::{AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::{AccountStateDiff, ChainedCall},
};
use stablecoin_core::{
    bound_redemption_price, compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    math::{compute_current_redemption_price_wide, FIXED_POINT_ONE},
    ProtocolParameters, RedemptionPriceState,
};

/// Return the redemption-rate controller to rest without repricing the system.
///
/// Re-anchors the redemption price at its current projection, clamped into the
/// §8 bounds, then sets the rate to exactly `1.0` and the integral term to `0`.
/// The price itself does not jump, so no position's collateralization changes at
/// the moment of the call. The next `update_redemption_rate` starts the
/// controller from a clean slate.
///
/// The recovery path for a wound-up or runaway controller, for example after a
/// bad oracle reading. Callable by the admin or the freeze authority, allowed
/// while frozen, and idempotent. It never panics on the projection, so it works
/// even from a state the u128 projection cannot express.
///
/// # Panics
/// - `authority` did not sign, or is neither `ProtocolParameters.admin_account_id` nor
///   `ProtocolParameters.freeze_authority_account_id`.
/// - `protocol_parameters` or `redemption_price_state` is uninitialized, wrongly owned, not at its
///   canonical PDA, or does not decode.
/// - `clock` is not the initialized system `CLOCK_01` account.
pub fn reset_redemption_rate_controller(
    authority: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    redemption_price_state: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: AccountId,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    assert!(
        authority.is_authorized,
        "Authority authorization is missing"
    );

    let parameters = ProtocolParameters::try_from(&crate::checks::decode_global(
        &protocol_parameters,
        compute_protocol_parameters_pda(stablecoin_program_id),
        stablecoin_program_id,
        "ProtocolParameters",
    ))
    .expect("ProtocolParameters must decode");
    assert!(
        authority.account_id == parameters.admin_account_id
            || authority.account_id == parameters.freeze_authority_account_id,
        "Signer is neither the protocol's admin nor its freeze authority"
    );

    let redemption = RedemptionPriceState::try_from(&crate::checks::decode_global(
        &redemption_price_state,
        compute_redemption_price_state_pda(stablecoin_program_id),
        stablecoin_program_id,
        "RedemptionPriceState",
    ))
    .expect("RedemptionPriceState must decode");

    let now = crate::accrue_stability_fee::read_clock(&clock);
    let current_price = bound_redemption_price(compute_current_redemption_price_wide(
        redemption.redemption_price_at_last_update,
        redemption.redemption_rate_per_millisecond,
        redemption.last_updated_at,
        now,
    ));

    let reset_state = RedemptionPriceState {
        redemption_price_at_last_update: current_price,
        redemption_rate_per_millisecond: FIXED_POINT_ONE,
        controller_integral_term: 0,
        last_updated_at: now,
    };

    let state_diffs = vec![
        AccountStateDiff::unchanged(authority),
        AccountStateDiff::unchanged(protocol_parameters),
        AccountStateDiff::new(
            redemption_price_state,
            BalanceDiff::Add(0),
            Data::from(&reset_state),
        ),
        AccountStateDiff::unchanged(clock),
    ];

    (state_diffs, vec![])
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "tests index fixed-size vectors and decode known-good fixtures"
)]
mod tests {
    use lee_core::account::AccountId;
    use stablecoin_core::{
        math::compute_current_redemption_price, MAXIMUM_REDEMPTION_PRICE, MINIMUM_REDEMPTION_PRICE,
    };

    use super::*;
    use crate::{
        test_support::{
            admin_id, clock_account, freeze_authority_id, protocol_parameters_account,
            redemption_price_state_account_with_integral_term, uninitialized, ParameterOverrides,
            NOW, REDEMPTION_PRICE_ANCHOR, STABLECOIN_PROGRAM_ID, T0,
        },
        StateDiffExt,
    };

    fn signer(account_id: AccountId) -> AccountWithMetadata {
        let mut account = crate::test_support::caller_account();
        account.account_id = account_id;
        account
    }

    /// A wound-up state: the rate pinned at the clamp and a saturated integral.
    fn runaway_state(rate: u128, last_updated_at: u64) -> AccountWithMetadata {
        let mut state = redemption_price_state_account_with_integral_term(
            last_updated_at,
            stablecoin_core::INTEGRAL_CLAMP,
        );
        let mut decoded = RedemptionPriceState::try_from(&state.account.data).unwrap();
        decoded.redemption_rate_per_millisecond = rate;
        state.account.data = Data::from(&decoded);
        state
    }

    fn invoke(
        authority: AccountWithMetadata,
        state: AccountWithMetadata,
    ) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
        reset_redemption_rate_controller(
            authority,
            protocol_parameters_account(ParameterOverrides::default()),
            state,
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        )
    }

    fn decode(post_states: &[AccountStateDiff]) -> RedemptionPriceState {
        RedemptionPriceState::try_from(post_states[2].post_data()).unwrap()
    }

    #[test]
    fn resets_rate_and_integral_and_re_anchors_at_the_projected_price() {
        let rate = FIXED_POINT_ONE + stablecoin_core::RATE_DELTA_CLAMP.unsigned_abs();
        let (post_states, chained_calls) =
            invoke(signer(freeze_authority_id()), runaway_state(rate, T0));
        assert_eq!(post_states.len(), 4);
        assert!(chained_calls.is_empty());

        let after = decode(&post_states);
        assert_eq!(
            after.redemption_price_at_last_update,
            compute_current_redemption_price(REDEMPTION_PRICE_ANCHOR, rate, T0, NOW)
        );
        assert_eq!(after.redemption_rate_per_millisecond, FIXED_POINT_ONE);
        assert_eq!(after.controller_integral_term, 0);
        assert_eq!(after.last_updated_at, NOW);
    }

    #[test]
    fn the_admin_may_reset_too() {
        let (post_states, _) = invoke(signer(admin_id()), runaway_state(FIXED_POINT_ONE, T0));
        assert_eq!(decode(&post_states).controller_integral_term, 0);
    }

    #[test]
    fn is_allowed_while_frozen() {
        let (post_states, _) = reset_redemption_rate_controller(
            signer(freeze_authority_id()),
            protocol_parameters_account(ParameterOverrides {
                is_frozen: true,
                ..ParameterOverrides::default()
            }),
            runaway_state(FIXED_POINT_ONE, T0),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
        assert_eq!(decode(&post_states).last_updated_at, NOW);
    }

    #[test]
    fn recovers_a_state_whose_projection_overflows_u128() {
        // The old ±10^-5 clamp, a day out: far past u128.
        let rate = FIXED_POINT_ONE + FIXED_POINT_ONE / 100_000;
        let (post_states, _) = invoke(
            signer(freeze_authority_id()),
            runaway_state(rate, NOW - 86_400_000),
        );
        let after = decode(&post_states);
        assert_eq!(
            after.redemption_price_at_last_update,
            MAXIMUM_REDEMPTION_PRICE
        );
        assert_eq!(after.redemption_rate_per_millisecond, FIXED_POINT_ONE);
    }

    #[test]
    fn recovers_a_state_whose_projection_decays_to_zero() {
        let rate = FIXED_POINT_ONE - FIXED_POINT_ONE / 100_000;
        let (post_states, _) = invoke(
            signer(freeze_authority_id()),
            runaway_state(rate, NOW - 86_400_000),
        );
        let after = decode(&post_states);
        assert_eq!(
            after.redemption_price_at_last_update,
            MINIMUM_REDEMPTION_PRICE
        );
        assert_eq!(after.redemption_rate_per_millisecond, FIXED_POINT_ONE);
    }

    #[test]
    fn is_idempotent() {
        let (first, _) = invoke(
            signer(freeze_authority_id()),
            runaway_state(FIXED_POINT_ONE, T0),
        );
        let mut state = runaway_state(FIXED_POINT_ONE, T0);
        state.account.data = first[2].post_data().clone();
        let (second, _) = invoke(signer(freeze_authority_id()), state);
        assert_eq!(decode(&first), decode(&second));
    }

    #[test]
    #[should_panic(expected = "Authority authorization is missing")]
    fn requires_a_signature() {
        let mut authority = signer(freeze_authority_id());
        authority.is_authorized = false;
        let _ = invoke(authority, runaway_state(FIXED_POINT_ONE, T0));
    }

    #[test]
    #[should_panic(expected = "Signer is neither the protocol's admin nor its freeze authority")]
    fn rejects_any_other_signer() {
        let _ = invoke(
            signer(AccountId::new([0xEE; 32])),
            runaway_state(FIXED_POINT_ONE, T0),
        );
    }

    #[test]
    #[should_panic(expected = "RedemptionPriceState account must be initialized")]
    fn rejects_an_uninitialized_redemption_price_state() {
        let _ = invoke(
            signer(freeze_authority_id()),
            uninitialized(compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)),
        );
    }

    #[test]
    #[should_panic(
        expected = "RedemptionPriceState account ID does not match expected PDA derivation"
    )]
    fn rejects_a_redemption_price_state_at_the_wrong_address() {
        let mut state = runaway_state(FIXED_POINT_ONE, T0);
        state.account_id = AccountId::new([0xDE; 32]);
        let _ = invoke(signer(freeze_authority_id()), state);
    }

    #[test]
    #[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
    fn rejects_the_wrong_clock() {
        let mut clock = clock_account(NOW);
        clock.account_id = AccountId::new([0xC1; 32]);
        let _ = reset_redemption_rate_controller(
            signer(freeze_authority_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            runaway_state(FIXED_POINT_ONE, T0),
            clock,
            STABLECOIN_PROGRAM_ID,
        );
    }
}
