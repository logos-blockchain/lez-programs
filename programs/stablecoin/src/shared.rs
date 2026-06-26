use borsh::BorshDeserialize as _;
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{Account, AccountWithMetadata},
    program::ProgramId,
};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, current_accumulated_rate, ProtocolParameters,
    RedemptionPriceState, StabilityFeeAccumulator, MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
};

pub(crate) fn read_clock_timestamp(clock: &AccountWithMetadata) -> u64 {
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Clock account must be the canonical 1-block LEZ clock account"
    );
    assert_ne!(
        clock.account,
        Account::default(),
        "Clock account must be initialized"
    );
    ClockAccountData::try_from_slice(clock.account.data.as_ref())
        .expect("Clock account must hold valid ClockAccountData")
        .timestamp
}

pub(crate) fn read_protocol_parameters(
    protocol_parameters: &AccountWithMetadata,
    stablecoin_program_id: ProgramId,
) -> ProtocolParameters {
    assert_eq!(
        protocol_parameters.account_id,
        compute_protocol_parameters_pda(stablecoin_program_id),
        "Protocol parameters account ID does not match PDA"
    );
    assert_initialized_owned(
        protocol_parameters,
        stablecoin_program_id,
        "Protocol parameters",
    );
    ProtocolParameters::try_from(&protocol_parameters.account.data)
        .expect("Protocol parameters account must hold valid ProtocolParameters state")
}

pub(crate) fn read_stability_fee_accumulator(
    stability_fee_accumulator: &AccountWithMetadata,
    stablecoin_program_id: ProgramId,
) -> StabilityFeeAccumulator {
    assert_eq!(
        stability_fee_accumulator.account_id,
        compute_stability_fee_accumulator_pda(stablecoin_program_id),
        "Stability fee accumulator account ID does not match PDA"
    );
    assert_initialized_owned(
        stability_fee_accumulator,
        stablecoin_program_id,
        "Stability fee accumulator",
    );
    StabilityFeeAccumulator::try_from(&stability_fee_accumulator.account.data)
        .expect("Stability fee accumulator account must hold valid state")
}

pub(crate) fn read_redemption_price_state(
    redemption_price_state: &AccountWithMetadata,
    stablecoin_program_id: ProgramId,
) -> RedemptionPriceState {
    assert_eq!(
        redemption_price_state.account_id,
        compute_redemption_price_state_pda(stablecoin_program_id),
        "Redemption price state account ID does not match PDA"
    );
    assert_initialized_owned(
        redemption_price_state,
        stablecoin_program_id,
        "Redemption price state",
    );
    RedemptionPriceState::try_from(&redemption_price_state.account.data)
        .expect("Redemption price state account must hold valid state")
}

pub(crate) fn accrue_stability_fee_state(
    accumulator: &StabilityFeeAccumulator,
    params: &ProtocolParameters,
    now: u64,
) -> StabilityFeeAccumulator {
    let elapsed = now
        .saturating_sub(accumulator.last_accrued_at)
        .min(MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS);
    let last_accrued_at = accumulator
        .last_accrued_at
        .checked_add(elapsed)
        .expect("Clamped elapsed timestamp cannot overflow");

    StabilityFeeAccumulator {
        accumulated_rate_at_last_accrual: current_accumulated_rate(accumulator, params, now),
        last_accrued_at,
    }
}

fn assert_initialized_owned(
    account: &AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    label: &str,
) {
    assert_ne!(
        account.account,
        Account::default(),
        "{label} account must be initialized"
    );
    assert_eq!(
        account.account.program_owner, stablecoin_program_id,
        "{label} account is not owned by this stablecoin program"
    );
}
