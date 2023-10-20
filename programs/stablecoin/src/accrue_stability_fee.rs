//! Host-side implementation of `Instruction::AccrueStabilityFee` (spec §10.2).
//!
//! Wall-clock time comes from the system `CLOCK_01` account, same as
//! [`crate::initialize_program`] — the pinned `spel-framework`'s
//! `ProgramContext` exposes no clock.

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::{AccountStateDiff, ChainedCall},
};
use stablecoin_core::{
    compute_stability_fee_accumulator_pda,
    math::{compute_current_accumulated_rate, mul_div, FIXED_POINT_ONE},
    ProtocolParameters, StabilityFeeAccumulator,
};

/// Advance the global stability-fee accumulator to the clock's timestamp and
/// mint the fees that accrued on the outstanding debt to the surplus holding.
///
/// Permissionless and idempotent: there is NO minimum-interval throttle (spec
/// §10.2, §5.3, §6.1). The read-side projection keeps every position current
/// regardless of cadence, so a redundant call just re-stamps `last_accrued_at`
/// and mints nothing. Never blocked by the frozen flag.
///
/// See spec §10.2 for the full account contract and panic conditions.
#[allow(clippy::needless_pass_by_value)]
pub fn accrue_stability_fee(
    caller: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    stablecoin_definition: AccountWithMetadata,
    stablecoin_master_holding: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: AccountId,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    assert!(caller.is_authorized, "Caller authorization is missing");

    let (params, accumulator) = decode_fee_accrual_inputs(
        &protocol_parameters,
        &stability_fee_accumulator,
        stablecoin_program_id,
    );
    let now = read_clock(&clock);

    let accrual = accrue_fees(&params, &accumulator, now);
    let surplus_mint = crate::checks::surplus_mint_call(
        &stablecoin_definition,
        &stablecoin_master_holding,
        &params,
        stablecoin_program_id,
        accrual.accrued_fees,
    );

    let state_diffs = vec![
        AccountStateDiff::unchanged(caller),
        AccountStateDiff::unchanged(protocol_parameters),
        AccountStateDiff::new(
            stability_fee_accumulator,
            BalanceDiff::Add(0),
            Data::from(&accrual.accumulator),
        ),
        AccountStateDiff::unchanged(stablecoin_definition),
        AccountStateDiff::unchanged(stablecoin_master_holding),
        AccountStateDiff::unchanged(clock),
    ];

    (state_diffs, surplus_mint.into_iter().collect())
}

/// Validate and decode the two accounts the fee half needs.
///
/// Shared with [`crate::refresh_globals`] so the combined poke can never drift
/// from the standalone one.
pub(crate) fn decode_fee_accrual_inputs(
    protocol_parameters: &AccountWithMetadata,
    stability_fee_accumulator: &AccountWithMetadata,
    stablecoin_program_id: AccountId,
) -> (ProtocolParameters, StabilityFeeAccumulator) {
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
        stability_fee_accumulator.account,
        Account::default(),
        "StabilityFeeAccumulator account must be initialized"
    );
    assert_eq!(
        stability_fee_accumulator.account.program_owner, stablecoin_program_id,
        "StabilityFeeAccumulator not owned by this stablecoin program"
    );
    assert_eq!(
        stability_fee_accumulator.account_id,
        compute_stability_fee_accumulator_pda(stablecoin_program_id),
        "StabilityFeeAccumulator account ID does not match expected PDA derivation"
    );

    let params = ProtocolParameters::try_from(&protocol_parameters.account.data)
        .expect("ProtocolParameters must decode");
    let accumulator = StabilityFeeAccumulator::try_from(&stability_fee_accumulator.account.data)
        .expect("StabilityFeeAccumulator must decode");

    (params, accumulator)
}

/// The accumulator advanced to `now`, and the fees that accrued on the
/// outstanding debt since the last accrual.
pub(crate) struct FeeAccrual {
    /// The accumulator to persist. `total_normalized_debt` is carried over.
    pub(crate) accumulator: StabilityFeeAccumulator,
    /// `total_normalized_debt × Δaccumulator / FIXED_POINT_ONE`, rounded down:
    /// the stablecoin to mint to the surplus holding (spec §6.6).
    pub(crate) accrued_fees: u128,
}

/// Project the accumulator to `now` and compute the fees accrued on the
/// outstanding debt — RAI's `SAFEEngine.updateAccumulatedRate`.
///
/// Shared by every instruction that accrues: both fee pokes, the fee setter,
/// `generate_debt` and `repay_debt`. They can never disagree about how a gap
/// accrues or how much surplus it mints.
pub(crate) fn accrue_fees(
    params: &ProtocolParameters,
    accumulator: &StabilityFeeAccumulator,
    now: u64,
) -> FeeAccrual {
    let current = compute_current_accumulated_rate(
        accumulator.accumulated_rate_at_last_accrual,
        params.stability_fee_per_millisecond,
        accumulator.last_accrued_at,
        now,
    );
    // The fee is bounded below by 1.0 (§8), so the accumulator never decreases.
    let rate_delta = current
        .checked_sub(accumulator.accumulated_rate_at_last_accrual)
        .expect("Stability fee accumulator must not decrease");
    let accrued_fees = mul_div(
        accumulator.total_normalized_debt,
        rate_delta,
        FIXED_POINT_ONE,
    );

    FeeAccrual {
        accumulator: StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: current,
            last_accrued_at: now,
            ..accumulator.clone()
        },
        accrued_fees,
    }
}

/// Read the millisecond wall-clock timestamp from the system `CLOCK_01` account.
///
/// Shared by all three pokes.
pub(crate) fn read_clock(clock: &AccountWithMetadata) -> u64 {
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Clock account must be the system CLOCK_01 account"
    );
    assert_ne!(
        clock.account,
        Account::default(),
        "Clock account must be initialized"
    );
    ClockAccountData::from_bytes(clock.account.data.as_ref()).timestamp
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests deliberately panic on bad state via assert!/#[should_panic] and index fixed-size vectors"
)]
mod tests {
    use lee_core::account::{AccountId, Nonce};
    use stablecoin_core::math::FIXED_POINT_ONE;

    use super::*;
    use crate::{
        test_support::{
            accumulator_account, caller_account, clock_account, protocol_parameters_account,
            stablecoin_definition_account, stablecoin_master_holding_account, uninitialized,
            ParameterOverrides, ACCUMULATOR_ANCHOR, NOW, STABLECOIN_PROGRAM_ID, T0,
            TEST_STABILITY_FEE_PER_MILLISECOND,
        },
        StateDiffExt,
    };

    fn invoke() -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
        accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        )
    }

    #[test]
    fn happy_path_without_debt_returns_six_post_states_and_mints_nothing() {
        let (post_states, chained_calls) = invoke();
        assert_eq!(post_states.len(), 6);
        // No outstanding debt, so no fees accrued and nothing to mint.
        assert!(chained_calls.is_empty());
    }

    #[test]
    fn mints_the_fees_accrued_on_outstanding_debt_to_the_surplus_holding() {
        let total_normalized_debt = 1_000_000_000_000_000_000_u128;
        let (post_states, chained_calls) = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            crate::test_support::accumulator_account_with_debt(
                ACCUMULATOR_ANCHOR,
                T0,
                total_normalized_debt,
            ),
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );

        let decoded = StabilityFeeAccumulator::try_from(post_states[2].post_data()).unwrap();
        assert_eq!(decoded.total_normalized_debt, total_normalized_debt);
        let expected_fees = mul_div(
            total_normalized_debt,
            decoded.accumulated_rate_at_last_accrual - ACCUMULATOR_ANCHOR,
            FIXED_POINT_ONE,
        );
        assert!(expected_fees > 0);

        let expected_mint = ChainedCall::new(
            crate::test_support::TOKEN_PROGRAM_ID,
            vec![
                crate::test_support::stablecoin_definition_id(),
                stablecoin_core::compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID),
            ],
            &token_core::Instruction::Mint {
                amount_to_mint: expected_fees,
            },
        )
        .with_pda_seeds(vec![
            stablecoin_core::compute_stablecoin_definition_pda_seed(),
        ]);
        assert_eq!(chained_calls, vec![expected_mint]);
    }

    #[test]
    #[should_panic(
        expected = "Stablecoin master holding account ID does not match expected PDA derivation"
    )]
    fn rejects_a_surplus_holding_that_is_not_the_master_holding() {
        let mut holding = stablecoin_master_holding_account();
        holding.account_id = AccountId::new([0xBB; 32]);
        let _ = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            stablecoin_definition_account(),
            holding,
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(
        expected = "Stablecoin definition does not match the one bound at initialize_program"
    )]
    fn rejects_a_foreign_stablecoin_definition() {
        let mut definition = stablecoin_definition_account();
        definition.account_id = AccountId::new([0xBC; 32]);
        let _ = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            definition,
            stablecoin_master_holding_account(),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    fn happy_path_advances_accumulator_by_the_projection() {
        let (post_states, _) = invoke();
        let decoded =
            StabilityFeeAccumulator::try_from(post_states[2].post_data()).expect("decode");
        let expected = compute_current_accumulated_rate(
            ACCUMULATOR_ANCHOR,
            TEST_STABILITY_FEE_PER_MILLISECOND,
            T0,
            NOW,
        );
        assert_eq!(decoded.accumulated_rate_at_last_accrual, expected);
        assert!(decoded.accumulated_rate_at_last_accrual > ACCUMULATOR_ANCHOR);
        assert_eq!(decoded.last_accrued_at, NOW);
    }

    #[test]
    fn happy_path_claims_no_pda_and_echoes_the_clock() {
        // The accumulator PDA was claimed at initialize_program time; this poke
        // only rewrites its data.
        let (post_states, _) = invoke();
        assert_eq!(
            post_states[2].post_owner(STABLECOIN_PROGRAM_ID),
            post_states[2].pre_state.account.program_owner
        );
        assert_eq!(
            post_states[5].post_owner(STABLECOIN_PROGRAM_ID),
            post_states[5].pre_state.account.program_owner
        );
        assert_eq!(
            post_states[5].post_account(STABLECOIN_PROGRAM_ID),
            clock_account(NOW).account
        );
    }

    #[test]
    fn no_throttle_zero_elapsed_is_a_noop_restamp() {
        // There is NO minimum-interval throttle: calling with
        // `last_accrued_at == now` does not panic. Zero elapsed time collapses to
        // the compound_rate identity, so the anchor is unchanged.
        let (post_states, _) = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, NOW),
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
        let decoded = StabilityFeeAccumulator::try_from(post_states[2].post_data()).unwrap();
        assert_eq!(decoded.accumulated_rate_at_last_accrual, ACCUMULATOR_ANCHOR);
        assert_eq!(decoded.last_accrued_at, NOW);
    }

    #[test]
    fn accrual_is_allowed_while_frozen() {
        // Pokes are never blocked by the frozen flag (spec §10.2).
        let (post_states, _) = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides {
                is_frozen: true,
                ..ParameterOverrides::default()
            }),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
        let decoded = StabilityFeeAccumulator::try_from(post_states[2].post_data()).unwrap();
        assert_eq!(decoded.last_accrued_at, NOW);
    }

    #[test]
    #[should_panic(expected = "Caller authorization is missing")]
    fn requires_caller_authorization() {
        let mut caller = caller_account();
        caller.is_authorized = false;
        let _ = accrue_stability_fee(
            caller,
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "ProtocolParameters account must be initialized")]
    fn rejects_uninitialized_protocol_parameters() {
        let _ = accrue_stability_fee(
            caller_account(),
            uninitialized(stablecoin_core::compute_protocol_parameters_pda(
                STABLECOIN_PROGRAM_ID,
            )),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "ProtocolParameters not owned by this stablecoin program")]
    fn rejects_foreign_owned_protocol_parameters() {
        let mut parameters = protocol_parameters_account(ParameterOverrides::default());
        parameters.account.program_owner = AccountId::new([9u8; 32]);
        let _ = accrue_stability_fee(
            caller_account(),
            parameters,
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "StabilityFeeAccumulator account must be initialized")]
    fn rejects_uninitialized_accumulator() {
        let _ = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            uninitialized(compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)),
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "StabilityFeeAccumulator not owned by this stablecoin program")]
    fn rejects_foreign_owned_accumulator() {
        let mut accumulator = accumulator_account(ACCUMULATOR_ANCHOR, T0);
        accumulator.account.program_owner = AccountId::new([9u8; 32]);
        let _ = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator,
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
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
        let _ = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator,
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
    fn rejects_wrong_clock_account() {
        let mut clock = clock_account(NOW);
        clock.account_id = AccountId::new([0xC1; 32]);
        let _ = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock,
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Clock account must be initialized")]
    fn rejects_uninitialized_clock_account() {
        let clock = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: CLOCK_01_PROGRAM_ACCOUNT_ID,
        };
        let _ = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock,
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    fn a_zero_fee_rate_leaves_the_anchor_untouched() {
        // FIXED_POINT_ONE is "no drift": the accumulator only re-stamps.
        let (post_states, _) = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides {
                stability_fee_per_millisecond: FIXED_POINT_ONE,
                ..ParameterOverrides::default()
            }),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            stablecoin_definition_account(),
            stablecoin_master_holding_account(),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
        let decoded = StabilityFeeAccumulator::try_from(post_states[2].post_data()).unwrap();
        assert_eq!(decoded.accumulated_rate_at_last_accrual, ACCUMULATOR_ANCHOR);
        assert_eq!(decoded.last_accrued_at, NOW);
    }

    #[test]
    fn accumulator_post_state_preserves_owner_and_nonce() {
        let (post_states, _) = invoke();
        let original = accumulator_account(ACCUMULATOR_ANCHOR, T0);
        assert_eq!(
            post_states[2].post_owner(STABLECOIN_PROGRAM_ID),
            original.account.program_owner
        );
        assert_eq!(
            post_states[2].post_account(STABLECOIN_PROGRAM_ID).nonce,
            Nonce(0)
        );
    }
}
