//! Admin parameter updates (spec §10.10–10.16).
//!
//! Every setter shares one skeleton: validate `protocol_parameters` at its
//! canonical PDA, check the caller against the stored admin handle, bound-check
//! the new value against §8, then overwrite exactly the named field(s).

use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_stability_fee_accumulator_pda, ProtocolParameters,
    StabilityFeeAccumulator,
};
use twap_oracle_core::OraclePriceAccount;

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

/// The shape all six simple setters share: validate the parameters account at its
/// canonical PDA, check the caller against the stored admin handle, let the caller
/// mutate the decoded struct, then write it back.
///
/// Bound-checking happens inside `mutate`, so each setter states its own §8 band
/// and its own panic message.
fn update_parameters(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    mutate: impl FnOnce(&mut ProtocolParameters),
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let mut parameters = ProtocolParameters::try_from(&crate::checks::decode_global(
        &protocol_parameters,
        compute_protocol_parameters_pda(stablecoin_program_id),
        stablecoin_program_id,
        "ProtocolParameters",
    ))
    .expect("ProtocolParameters must decode");
    crate::checks::assert_admin_authorized(&admin, &parameters);

    mutate(&mut parameters);

    let mut parameters_post = protocol_parameters.account;
    parameters_post.data = Data::from(&parameters);

    let post_states = vec![
        AccountPostState::new(admin.account),
        AccountPostState::new(parameters_post),
    ];

    (post_states, vec![])
}

/// Retune the minimum collateralization ratio (spec §10.11).
///
/// Tightening can leave existing positions retroactively under-collateralized:
/// they can still `deposit_collateral` or `repay_debt` to recover, but cannot
/// `withdraw_collateral` or `generate_debt` until they are back above the ratio.
///
/// # Panics
/// - `admin` did not sign, or is not `ProtocolParameters.admin_account_id`.
/// - `new_ratio` is outside the §8 band `1.1x ..= 10x`.
/// - `protocol_parameters` is invalid — see [`set_stability_fee_per_millisecond`].
pub fn set_minimum_collateralization_ratio(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    new_ratio: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    update_parameters(
        admin,
        protocol_parameters,
        stablecoin_program_id,
        |parameters| {
            crate::checks::assert_collateralization_ratio_in_band(
                new_ratio,
                "new_minimum_collateralization_ratio",
            );
            parameters.minimum_collateralization_ratio = new_ratio;
        },
    )
}

/// Retune both PI controller gains (spec §10.12).
///
/// Bundled because tuning one without the other is rarely meaningful. Deliberately
/// does **not** reset `controller_integral_term`: the accumulated history stays,
/// so re-tuning does not discard the controller's state.
///
/// # Panics
/// - `admin` did not sign, or is not `ProtocolParameters.admin_account_id`.
/// - Either gain magnitude is outside its §8 band.
/// - `protocol_parameters` is invalid.
pub fn set_controller_gains(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    new_proportional_gain: i128,
    new_integral_gain: i128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    update_parameters(
        admin,
        protocol_parameters,
        stablecoin_program_id,
        |parameters| {
            crate::checks::assert_controller_gains_in_band(
                new_proportional_gain,
                new_integral_gain,
            );
            parameters.controller_proportional_gain = new_proportional_gain;
            parameters.controller_integral_gain = new_integral_gain;
        },
    )
}

/// Retune both timing parameters (spec §10.14).
///
/// # Panics
/// - `admin` did not sign, or is not `ProtocolParameters.admin_account_id`.
/// - Either value is outside the §8 band `1 ..= 86_400_000` milliseconds.
/// - `protocol_parameters` is invalid.
pub fn set_timing_parameters(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    new_minimum_milliseconds_between_rate_updates: u64,
    new_maximum_oracle_price_age_milliseconds: u64,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    update_parameters(
        admin,
        protocol_parameters,
        stablecoin_program_id,
        |parameters| {
            crate::checks::assert_timing_milliseconds_in_band(
                new_minimum_milliseconds_between_rate_updates,
                "new_minimum_milliseconds_between_rate_updates",
            );
            crate::checks::assert_timing_milliseconds_in_band(
                new_maximum_oracle_price_age_milliseconds,
                "new_maximum_oracle_price_age_milliseconds",
            );
            parameters.minimum_milliseconds_between_rate_updates =
                new_minimum_milliseconds_between_rate_updates;
            parameters.maximum_oracle_price_age_milliseconds =
                new_maximum_oracle_price_age_milliseconds;
        },
    )
}

/// Rotate the admin handle (spec §10.15).
///
/// One-step: the new handle is effective immediately, since every admin check
/// reads `ProtocolParameters.admin_account_id` at call time. There is no
/// confirmation step, so a wrong id locks the admin out permanently.
///
/// # Panics
/// - `admin` did not sign, or is not the current `admin_account_id`.
/// - `protocol_parameters` is invalid.
pub fn set_admin(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    new_admin_account_id: AccountId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    update_parameters(
        admin,
        protocol_parameters,
        stablecoin_program_id,
        |parameters| {
            parameters.admin_account_id = new_admin_account_id;
        },
    )
}

/// Rotate the freeze-authority handle (spec §10.16).
///
/// Set by the **admin**, not by the freeze authority itself, so a compromised
/// freeze authority cannot entrench itself. Same one-step caveat as [`set_admin`].
///
/// # Panics
/// - `admin` did not sign, or is not `ProtocolParameters.admin_account_id`.
/// - `protocol_parameters` is invalid.
pub fn set_freeze_authority(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    new_freeze_authority_account_id: AccountId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    update_parameters(
        admin,
        protocol_parameters,
        stablecoin_program_id,
        |parameters| {
            parameters.freeze_authority_account_id = new_freeze_authority_account_id;
        },
    )
}

/// Rotate the market-price oracle (spec §10.13).
///
/// Validates the replacement's shape and that its base/quote pair matches the
/// definitions bound at bootstrap, so the controller cannot be pointed at an
/// oracle quoting a different market. `program_owner` is deliberately not pinned:
/// any producer emitting a well-formed `OraclePriceAccount` is acceptable.
///
/// # Panics
/// - `admin` did not sign, or is not `ProtocolParameters.admin_account_id`.
/// - `new_oracle` is uninitialized, does not decode as an `OraclePriceAccount`, or its `base_asset`
///   / `quote_asset` do not match the bound definitions.
/// - `protocol_parameters` is invalid.
pub fn set_market_price_oracle(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    new_oracle: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let new_oracle_id = new_oracle.account_id;
    let (mut post_states, chained_calls) = update_parameters(
        admin,
        protocol_parameters,
        stablecoin_program_id,
        |parameters| {
            assert_ne!(
                new_oracle.account,
                Account::default(),
                "New market price oracle must be initialized"
            );
            let oracle = OraclePriceAccount::try_from(&new_oracle.account.data)
                .expect("New market price oracle must decode as OraclePriceAccount");
            assert_eq!(
                oracle.base_asset, parameters.stablecoin_definition_id,
                "New oracle base_asset must equal the stablecoin definition's account_id"
            );
            assert_eq!(
                oracle.quote_asset, parameters.collateral_definition_id,
                "New oracle quote_asset must equal the collateral definition's account_id"
            );
            parameters.market_price_oracle_id = new_oracle_id;
        },
    );
    post_states.push(AccountPostState::new(new_oracle.account));
    (post_states, chained_calls)
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
    // --- the six simple setters ---

    fn params() -> AccountWithMetadata {
        protocol_parameters_account(ParameterOverrides::default())
    }

    fn before() -> ProtocolParameters {
        ProtocolParameters::try_from(&params().account.data).expect("valid ProtocolParameters")
    }

    fn new_oracle_account() -> AccountWithMetadata {
        let mut oracle = crate::test_support::oracle_account(NOW, FIXED_POINT_ONE / 4);
        oracle.account_id = AccountId::new([0x3Au8; 32]);
        oracle
    }

    #[test]
    fn set_minimum_collateralization_ratio_writes_only_that_field() {
        let new_ratio = FIXED_POINT_ONE * 2;
        let (post_states, chained_calls) = set_minimum_collateralization_ratio(
            admin_account(),
            params(),
            STABLECOIN_PROGRAM_ID,
            new_ratio,
        );

        assert_eq!(post_states.len(), 2);
        assert!(chained_calls.is_empty());
        let after = decoded(&post_states[1]);
        assert_eq!(after.minimum_collateralization_ratio, new_ratio);
        assert_eq!(
            ProtocolParameters {
                minimum_collateralization_ratio: before().minimum_collateralization_ratio,
                ..after
            },
            before()
        );
    }

    #[test]
    fn set_minimum_collateralization_ratio_accepts_both_band_endpoints() {
        for ratio in [FIXED_POINT_ONE * 110 / 100, FIXED_POINT_ONE * 10] {
            let (post_states, _) = set_minimum_collateralization_ratio(
                admin_account(),
                params(),
                STABLECOIN_PROGRAM_ID,
                ratio,
            );
            assert_eq!(
                decoded(&post_states[1]).minimum_collateralization_ratio,
                ratio
            );
        }
    }

    #[test]
    #[should_panic(expected = "new_minimum_collateralization_ratio below 1.1x")]
    fn set_minimum_collateralization_ratio_rejects_below_the_band() {
        set_minimum_collateralization_ratio(
            admin_account(),
            params(),
            STABLECOIN_PROGRAM_ID,
            FIXED_POINT_ONE * 110 / 100 - 1,
        );
    }

    #[test]
    #[should_panic(expected = "new_minimum_collateralization_ratio above 10x")]
    fn set_minimum_collateralization_ratio_rejects_above_the_band() {
        set_minimum_collateralization_ratio(
            admin_account(),
            params(),
            STABLECOIN_PROGRAM_ID,
            FIXED_POINT_ONE * 10 + 1,
        );
    }

    #[test]
    fn set_controller_gains_writes_both_and_preserves_the_integral_term() {
        let parameters = protocol_parameters_account(ParameterOverrides {
            controller_integral_gain: 5,
            ..Default::default()
        });
        let integral_term_before = ProtocolParameters::try_from(&parameters.account.data)
            .expect("valid ProtocolParameters");
        let (post_states, _) =
            set_controller_gains(admin_account(), parameters, STABLECOIN_PROGRAM_ID, -42, 7);

        let after = decoded(&post_states[1]);
        assert_eq!(after.controller_proportional_gain, -42);
        assert_eq!(after.controller_integral_gain, 7);
        // Retuning must not discard the controller's accumulated history; the
        // integral *term* lives on RedemptionPriceState and is untouched here.
        assert_eq!(
            ProtocolParameters {
                controller_proportional_gain: integral_term_before.controller_proportional_gain,
                controller_integral_gain: integral_term_before.controller_integral_gain,
                ..after
            },
            integral_term_before
        );
    }

    #[test]
    #[should_panic(expected = "controller_proportional_gain out of band")]
    fn set_controller_gains_rejects_an_oversized_proportional_gain() {
        let kp = i128::try_from(FIXED_POINT_ONE * 1_000).expect("fits i128") + 1;
        set_controller_gains(admin_account(), params(), STABLECOIN_PROGRAM_ID, kp, 0);
    }

    #[test]
    #[should_panic(expected = "controller_integral_gain out of band")]
    fn set_controller_gains_rejects_an_oversized_integral_gain() {
        let ki = i128::try_from(FIXED_POINT_ONE).expect("fits i128") + 1;
        set_controller_gains(admin_account(), params(), STABLECOIN_PROGRAM_ID, 0, ki);
    }

    #[test]
    fn set_timing_parameters_writes_both_fields() {
        let (post_states, _) =
            set_timing_parameters(admin_account(), params(), STABLECOIN_PROGRAM_ID, 5, 6);
        let after = decoded(&post_states[1]);
        assert_eq!(after.minimum_milliseconds_between_rate_updates, 5);
        assert_eq!(after.maximum_oracle_price_age_milliseconds, 6);
    }

    #[test]
    #[should_panic(expected = "new_minimum_milliseconds_between_rate_updates below minimum 1ms")]
    fn set_timing_parameters_rejects_a_zero_interval() {
        set_timing_parameters(admin_account(), params(), STABLECOIN_PROGRAM_ID, 0, 6);
    }

    #[test]
    #[should_panic(
        expected = "new_maximum_oracle_price_age_milliseconds above maximum 86_400_000ms"
    )]
    fn set_timing_parameters_rejects_an_oversized_staleness_window() {
        set_timing_parameters(
            admin_account(),
            params(),
            STABLECOIN_PROGRAM_ID,
            5,
            86_400_001,
        );
    }

    #[test]
    fn set_admin_rotates_the_handle_in_one_step() {
        let new_admin = AccountId::new([0xADu8; 32]);
        let (post_states, _) =
            set_admin(admin_account(), params(), STABLECOIN_PROGRAM_ID, new_admin);

        let after = decoded(&post_states[1]);
        assert_eq!(after.admin_account_id, new_admin);
        // Effective immediately: the old admin no longer satisfies the gate.
        assert_ne!(after.admin_account_id, admin_id());
    }

    #[test]
    fn set_freeze_authority_rotates_the_handle() {
        let new_authority = AccountId::new([0xFAu8; 32]);
        let (post_states, _) = set_freeze_authority(
            admin_account(),
            params(),
            STABLECOIN_PROGRAM_ID,
            new_authority,
        );
        assert_eq!(
            decoded(&post_states[1]).freeze_authority_account_id,
            new_authority
        );
    }

    #[test]
    fn set_market_price_oracle_rotates_and_echoes_the_new_oracle() {
        let (post_states, chained_calls) = set_market_price_oracle(
            admin_account(),
            params(),
            new_oracle_account(),
            STABLECOIN_PROGRAM_ID,
        );

        assert_eq!(post_states.len(), 3);
        assert!(chained_calls.is_empty());
        assert_eq!(
            decoded(&post_states[1]).market_price_oracle_id,
            new_oracle_account().account_id
        );
        assert_eq!(*post_states[2].account(), new_oracle_account().account);
    }

    #[test]
    #[should_panic(expected = "New oracle base_asset must equal the stablecoin definition")]
    fn set_market_price_oracle_rejects_a_different_base_asset() {
        let mut oracle = new_oracle_account();
        oracle.account.data = Data::from(&OraclePriceAccount {
            base_asset: AccountId::new([0x99u8; 32]),
            quote_asset: crate::test_support::collateral_definition_id(),
            price: FIXED_POINT_ONE / 4,
            timestamp: NOW,
            source_id: crate::test_support::oracle_source_id(),
            confidence_interval: 0,
        });
        set_market_price_oracle(admin_account(), params(), oracle, STABLECOIN_PROGRAM_ID);
    }

    #[test]
    #[should_panic(expected = "New oracle quote_asset must equal the collateral definition")]
    fn set_market_price_oracle_rejects_a_different_quote_asset() {
        let mut oracle = new_oracle_account();
        oracle.account.data = Data::from(&OraclePriceAccount {
            base_asset: crate::test_support::stablecoin_definition_id(),
            quote_asset: AccountId::new([0x99u8; 32]),
            price: FIXED_POINT_ONE / 4,
            timestamp: NOW,
            source_id: crate::test_support::oracle_source_id(),
            confidence_interval: 0,
        });
        set_market_price_oracle(admin_account(), params(), oracle, STABLECOIN_PROGRAM_ID);
    }

    #[test]
    #[should_panic(expected = "New market price oracle must be initialized")]
    fn set_market_price_oracle_rejects_an_uninitialized_oracle() {
        set_market_price_oracle(
            admin_account(),
            params(),
            uninitialized(AccountId::new([0x3Au8; 32])),
            STABLECOIN_PROGRAM_ID,
        );
    }

    // Every public entry point must reject a signer that is not the stored admin,
    // not just the one that happens to share the helper.

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn set_minimum_collateralization_ratio_rejects_a_non_admin() {
        let mut impostor = admin_account();
        impostor.account_id = freeze_authority_id();
        set_minimum_collateralization_ratio(
            impostor,
            params(),
            STABLECOIN_PROGRAM_ID,
            FIXED_POINT_ONE * 2,
        );
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn set_controller_gains_rejects_a_non_admin() {
        let mut impostor = admin_account();
        impostor.account_id = freeze_authority_id();
        set_controller_gains(impostor, params(), STABLECOIN_PROGRAM_ID, 0, 0);
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn set_timing_parameters_rejects_a_non_admin() {
        let mut impostor = admin_account();
        impostor.account_id = freeze_authority_id();
        set_timing_parameters(impostor, params(), STABLECOIN_PROGRAM_ID, 5, 6);
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn set_admin_rejects_a_non_admin() {
        let mut impostor = admin_account();
        impostor.account_id = freeze_authority_id();
        set_admin(impostor, params(), STABLECOIN_PROGRAM_ID, admin_id());
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn set_freeze_authority_rejects_a_non_admin() {
        // Notably the freeze authority itself cannot rotate the role — only the
        // admin can, so a compromised freeze authority cannot entrench itself.
        let mut impostor = admin_account();
        impostor.account_id = freeze_authority_id();
        set_freeze_authority(impostor, params(), STABLECOIN_PROGRAM_ID, admin_id());
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn set_market_price_oracle_rejects_a_non_admin() {
        let mut impostor = admin_account();
        impostor.account_id = freeze_authority_id();
        set_market_price_oracle(
            impostor,
            params(),
            new_oracle_account(),
            STABLECOIN_PROGRAM_ID,
        );
    }
}
