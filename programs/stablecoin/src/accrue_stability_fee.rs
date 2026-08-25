//! Host-side implementation of `Instruction::AccrueStabilityFee` (spec §10.2).
//!
//! Wall-clock time comes from the system `CLOCK_01` account, same as
//! [`crate::initialize_program`] — the pinned `spel-framework`'s
//! `ProgramContext` exposes no clock.

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{
    compute_stability_fee_accumulator_pda, math::compute_current_accumulated_rate,
    ProtocolParameters, StabilityFeeAccumulator,
};

/// Advance the global stability-fee accumulator to the clock's timestamp.
///
/// Permissionless and idempotent: there is NO minimum-interval throttle (spec
/// §10.2, §5.3, §6.1). The read-side projection keeps every position current
/// regardless of cadence, so a redundant call is a harmless no-op that just
/// re-stamps `last_accrued_at`. Never blocked by the frozen flag.
///
/// See spec §10.2 for the full account contract and panic conditions.
#[allow(clippy::needless_pass_by_value)]
pub fn accrue_stability_fee(
    caller: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(caller.is_authorized, "Caller authorization is missing");

    let (params, accumulator) = decode_fee_accrual_inputs(
        &protocol_parameters,
        &stability_fee_accumulator,
        stablecoin_program_id,
    );
    let now = read_clock(&clock);

    let accumulator_post =
        advance_fee_accumulator(&stability_fee_accumulator, &params, &accumulator, now);

    let post_states = vec![
        AccountPostState::new(caller.account),
        AccountPostState::new(protocol_parameters.account),
        AccountPostState::new(accumulator_post),
        AccountPostState::new(clock.account),
    ];

    (post_states, vec![])
}

/// Validate and decode the two accounts the fee half needs.
///
/// Shared with [`crate::refresh_globals`] so the combined poke can never drift
/// from the standalone one.
pub(crate) fn decode_fee_accrual_inputs(
    protocol_parameters: &AccountWithMetadata,
    stability_fee_accumulator: &AccountWithMetadata,
    stablecoin_program_id: ProgramId,
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

/// Project the accumulator to `now` and return the updated account.
///
/// Shared with [`crate::refresh_globals`] — the fee half is identical in both.
pub(crate) fn advance_fee_accumulator(
    stability_fee_accumulator: &AccountWithMetadata,
    params: &ProtocolParameters,
    accumulator: &StabilityFeeAccumulator,
    now: u64,
) -> Account {
    let updated = StabilityFeeAccumulator {
        accumulated_rate_at_last_accrual: compute_current_accumulated_rate(
            accumulator.accumulated_rate_at_last_accrual,
            params.stability_fee_per_millisecond,
            accumulator.last_accrued_at,
            now,
        ),
        last_accrued_at: now,
    };

    let mut accumulator_post = stability_fee_accumulator.account.clone();
    accumulator_post.data = Data::from(&updated);
    accumulator_post
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
    use crate::test_support::{
        accumulator_account, caller_account, clock_account, protocol_parameters_account,
        uninitialized, ParameterOverrides, ACCUMULATOR_ANCHOR, NOW, STABLECOIN_PROGRAM_ID, T0,
        TEST_STABILITY_FEE_PER_MILLISECOND,
    };

    fn invoke() -> (Vec<AccountPostState>, Vec<ChainedCall>) {
        accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        )
    }

    #[test]
    fn happy_path_returns_four_post_states_and_no_chained_calls() {
        let (post_states, chained_calls) = invoke();
        assert_eq!(post_states.len(), 4);
        assert!(chained_calls.is_empty());
    }

    #[test]
    fn happy_path_advances_accumulator_by_the_projection() {
        let (post_states, _) = invoke();
        let decoded =
            StabilityFeeAccumulator::try_from(&post_states[2].account().data).expect("decode");
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
        assert_eq!(post_states[2].required_claim(), None);
        assert_eq!(post_states[3].required_claim(), None);
        assert_eq!(post_states[3].account(), &clock_account(NOW).account);
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
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
        let decoded = StabilityFeeAccumulator::try_from(&post_states[2].account().data).unwrap();
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
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
        let decoded = StabilityFeeAccumulator::try_from(&post_states[2].account().data).unwrap();
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
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "ProtocolParameters not owned by this stablecoin program")]
    fn rejects_foreign_owned_protocol_parameters() {
        let mut parameters = protocol_parameters_account(ParameterOverrides::default());
        parameters.account.program_owner = [9u32; 8];
        let _ = accrue_stability_fee(
            caller_account(),
            parameters,
            accumulator_account(ACCUMULATOR_ANCHOR, T0),
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
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "StabilityFeeAccumulator not owned by this stablecoin program")]
    fn rejects_foreign_owned_accumulator() {
        let mut accumulator = accumulator_account(ACCUMULATOR_ANCHOR, T0);
        accumulator.account.program_owner = [9u32; 8];
        let _ = accrue_stability_fee(
            caller_account(),
            protocol_parameters_account(ParameterOverrides::default()),
            accumulator,
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
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
        );
        let decoded = StabilityFeeAccumulator::try_from(&post_states[2].account().data).unwrap();
        assert_eq!(decoded.accumulated_rate_at_last_accrual, ACCUMULATOR_ANCHOR);
        assert_eq!(decoded.last_accrued_at, NOW);
    }

    #[test]
    fn accumulator_post_state_preserves_owner_and_nonce() {
        let (post_states, _) = invoke();
        let original = accumulator_account(ACCUMULATOR_ANCHOR, T0);
        assert_eq!(
            post_states[2].account().program_owner,
            original.account.program_owner
        );
        assert_eq!(post_states[2].account().nonce, Nonce(0));
    }
}
