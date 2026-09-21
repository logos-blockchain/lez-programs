//! Admin parameter updates (spec §10.10–10.16).
//!
//! Every setter shares one skeleton: validate `protocol_parameters` at its
//! canonical PDA, check the caller against the stored admin handle, bound-check
//! the new value against §8, then overwrite exactly the named field(s).

use lee_core::{
    account::{AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_stability_fee_accumulator_pda, ProtocolParameters,
    StabilityFeeAccumulator,
};

/// Retune the stability fee (spec §10.10).
///
/// Accrues the fee accumulator forward at the **old** rate first, so the new rate
/// applies only from `now` onward and never retroactively to the elapsed gap.
/// That is why this is the one setter that takes the clock and the accumulator.
///
/// # Panics
/// - `admin` did not sign, or is not `ProtocolParameters.admin_account_id`.
/// - `new_rate` is outside the §8 band.
/// - `protocol_parameters` or `stability_fee_accumulator` is uninitialized, wrongly owned, not at
///   its canonical PDA, or does not decode.
/// - `clock` is not the initialized system `CLOCK_01` account.
pub fn set_stability_fee_per_millisecond(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    new_rate: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let mut parameters = ProtocolParameters::try_from(&crate::checks::decode_global(
        &protocol_parameters,
        compute_protocol_parameters_pda(stablecoin_program_id),
        stablecoin_program_id,
        "ProtocolParameters",
    ))
    .expect("ProtocolParameters must decode");
    crate::checks::assert_admin_authorized(&admin, &parameters);
    crate::checks::assert_stability_fee_in_band(new_rate, "new_stability_fee_per_millisecond");

    let accumulator = StabilityFeeAccumulator::try_from(&crate::checks::decode_global(
        &stability_fee_accumulator,
        compute_stability_fee_accumulator_pda(stablecoin_program_id),
        stablecoin_program_id,
        "StabilityFeeAccumulator",
    ))
    .expect("StabilityFeeAccumulator must decode");

    let now = crate::accrue_stability_fee::read_clock(&clock);
    // Accrue *before* the rate is replaced. `advance_fee_accumulator` reads the
    // rate out of `parameters`, so calling it first is what makes the change
    // non-retroactive — and it is the same helper `accrue_stability_fee` uses, so
    // the two can never disagree about how a gap accrues.
    let accumulator_post = crate::accrue_stability_fee::advance_fee_accumulator(
        &stability_fee_accumulator,
        &parameters,
        &accumulator,
        now,
    );

    parameters.stability_fee_per_millisecond = new_rate;
    let mut parameters_post = protocol_parameters.account;
    parameters_post.data = Data::from(&parameters);

    let post_states = vec![
        AccountPostState::new(admin.account),
        AccountPostState::new(parameters_post),
        AccountPostState::new(accumulator_post),
        AccountPostState::new(clock.account),
    ];

    (post_states, vec![])
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "tests build fixture values from constants and deliberately panic via #[should_panic]"
)]
mod tests {
    use lee_core::account::{Account, AccountId};
    use stablecoin_core::math::{compute_current_accumulated_rate, FIXED_POINT_ONE};

    use super::*;
    use crate::test_support::{
        accumulator_account, accumulator_id, admin_id, clock_account, freeze_authority_id,
        protocol_parameters_account, protocol_parameters_id, uninitialized, ParameterOverrides,
        ACCUMULATOR_ANCHOR, NOW, STABLECOIN_PROGRAM_ID, T0,
    };

    const OLD_RATE: u128 = FIXED_POINT_ONE + 1_500_000_000_000_000;
    const NEW_RATE: u128 = FIXED_POINT_ONE + 3_000_000_000_000_000;

    fn admin_account() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: admin_id(),
        }
    }

    fn parameters_at(rate: u128) -> AccountWithMetadata {
        protocol_parameters_account(ParameterOverrides {
            stability_fee_per_millisecond: rate,
            ..Default::default()
        })
    }

    fn set_rate(
        admin: AccountWithMetadata,
        parameters: AccountWithMetadata,
        accumulator: AccountWithMetadata,
        now: u64,
        new_rate: u128,
    ) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
        set_stability_fee_per_millisecond(
            admin,
            parameters,
            accumulator,
            clock_account(now),
            STABLECOIN_PROGRAM_ID,
            new_rate,
        )
    }

    fn decoded(post: &AccountPostState) -> ProtocolParameters {
        ProtocolParameters::try_from(&post.account().data).expect("valid ProtocolParameters")
    }

    #[test]
    fn writes_the_new_rate_and_emits_no_chained_calls() {
        let (post_states, chained_calls) = set_rate(
            admin_account(),
            parameters_at(OLD_RATE),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            NOW,
            NEW_RATE,
        );

        assert_eq!(post_states.len(), 4);
        assert!(chained_calls.is_empty());
        assert_eq!(
            decoded(&post_states[1]).stability_fee_per_millisecond,
            NEW_RATE
        );
        assert_eq!(*post_states[3].account(), clock_account(NOW).account);
    }

    #[test]
    fn changes_only_the_fee_field() {
        let before = ProtocolParameters::try_from(&parameters_at(OLD_RATE).account.data)
            .expect("valid ProtocolParameters");
        let (post_states, _) = set_rate(
            admin_account(),
            parameters_at(OLD_RATE),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            NOW,
            NEW_RATE,
        );
        let after = decoded(&post_states[1]);
        assert_eq!(
            ProtocolParameters {
                stability_fee_per_millisecond: OLD_RATE,
                ..after
            },
            before
        );
    }

    /// The load-bearing property: the elapsed gap accrues at the OLD rate, so a
    /// rate change is never retroactive.
    #[test]
    fn accrues_the_elapsed_gap_at_the_old_rate() {
        let (post_states, _) = set_rate(
            admin_account(),
            parameters_at(OLD_RATE),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            NOW,
            NEW_RATE,
        );

        let accumulator = StabilityFeeAccumulator::try_from(&post_states[2].account().data)
            .expect("valid StabilityFeeAccumulator");
        let at_old_rate = compute_current_accumulated_rate(ACCUMULATOR_ANCHOR, OLD_RATE, T0, NOW);
        let at_new_rate = compute_current_accumulated_rate(ACCUMULATOR_ANCHOR, NEW_RATE, T0, NOW);

        assert_eq!(accumulator.accumulated_rate_at_last_accrual, at_old_rate);
        assert_ne!(
            at_old_rate, at_new_rate,
            "fixture must make the two rates distinguishable"
        );
        assert_eq!(accumulator.last_accrued_at, NOW);
    }

    #[test]
    fn zero_elapsed_time_leaves_the_anchor_unchanged() {
        let (post_states, _) = set_rate(
            admin_account(),
            parameters_at(OLD_RATE),
            accumulator_account(ACCUMULATOR_ANCHOR, NOW),
            NOW,
            NEW_RATE,
        );
        let accumulator = StabilityFeeAccumulator::try_from(&post_states[2].account().data)
            .expect("valid StabilityFeeAccumulator");
        assert_eq!(
            accumulator.accumulated_rate_at_last_accrual,
            ACCUMULATOR_ANCHOR
        );
    }

    #[test]
    fn accepts_both_band_endpoints() {
        for rate in [FIXED_POINT_ONE, FIXED_POINT_ONE * 2] {
            let (post_states, _) = set_rate(
                admin_account(),
                parameters_at(OLD_RATE),
                accumulator_account(ACCUMULATOR_ANCHOR, T0),
                NOW,
                rate,
            );
            assert_eq!(decoded(&post_states[1]).stability_fee_per_millisecond, rate);
        }
    }

    #[test]
    #[should_panic(expected = "new_stability_fee_per_millisecond below FIXED_POINT_ONE")]
    fn rejects_a_rate_below_the_band() {
        set_rate(
            admin_account(),
            parameters_at(OLD_RATE),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            NOW,
            FIXED_POINT_ONE - 1,
        );
    }

    #[test]
    #[should_panic(expected = "new_stability_fee_per_millisecond above sane upper bound")]
    fn rejects_a_rate_above_the_band() {
        set_rate(
            admin_account(),
            parameters_at(OLD_RATE),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            NOW,
            FIXED_POINT_ONE * 2 + 1,
        );
    }

    #[test]
    #[should_panic(expected = "Admin authorization is missing")]
    fn requires_the_admin_to_sign() {
        let mut admin = admin_account();
        admin.is_authorized = false;
        set_rate(
            admin,
            parameters_at(OLD_RATE),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            NOW,
            NEW_RATE,
        );
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn rejects_a_signer_other_than_the_admin() {
        let mut impostor = admin_account();
        impostor.account_id = freeze_authority_id();
        set_rate(
            impostor,
            parameters_at(OLD_RATE),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            NOW,
            NEW_RATE,
        );
    }

    #[test]
    #[should_panic(
        expected = "ProtocolParameters account ID does not match expected PDA derivation"
    )]
    fn rejects_protocol_parameters_at_the_wrong_address() {
        let mut parameters = parameters_at(OLD_RATE);
        parameters.account_id = AccountId::new([0xC0u8; 32]);
        set_rate(
            admin_account(),
            parameters,
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            NOW,
            NEW_RATE,
        );
    }

    #[test]
    #[should_panic(expected = "ProtocolParameters account must be initialized")]
    fn rejects_uninitialized_protocol_parameters() {
        set_rate(
            admin_account(),
            uninitialized(protocol_parameters_id()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            NOW,
            NEW_RATE,
        );
    }

    #[test]
    #[should_panic(
        expected = "StabilityFeeAccumulator account ID does not match expected PDA derivation"
    )]
    fn rejects_the_accumulator_at_the_wrong_address() {
        let mut accumulator = accumulator_account(ACCUMULATOR_ANCHOR, T0);
        accumulator.account_id = AccountId::new([0xC1u8; 32]);
        set_rate(
            admin_account(),
            parameters_at(OLD_RATE),
            accumulator,
            NOW,
            NEW_RATE,
        );
    }

    #[test]
    #[should_panic(expected = "StabilityFeeAccumulator account must be initialized")]
    fn rejects_an_uninitialized_accumulator() {
        set_rate(
            admin_account(),
            parameters_at(OLD_RATE),
            uninitialized(accumulator_id()),
            NOW,
            NEW_RATE,
        );
    }
}
