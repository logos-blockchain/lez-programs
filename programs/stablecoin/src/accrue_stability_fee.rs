use nssa_core::{
    account::{AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};

use crate::shared::{
    accrue_stability_fee_state, read_clock_timestamp, read_protocol_parameters,
    read_stability_fee_accumulator,
};

/// Rolls the global stability-fee accumulator forward.
///
/// # Panics
/// Panics if the caller is unauthorized or the protocol accounts are invalid.
pub fn accrue_stability_fee(
    caller: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(caller.is_authorized, "Caller authorization is missing");

    let params = read_protocol_parameters(&protocol_parameters, stablecoin_program_id);
    let accumulator =
        read_stability_fee_accumulator(&stability_fee_accumulator, stablecoin_program_id);
    let now = read_clock_timestamp(&clock);
    let updated = accrue_stability_fee_state(&accumulator, &params, now);

    let mut accumulator_post = stability_fee_accumulator.account.clone();
    accumulator_post.data = Data::from(&updated);

    let post_states = vec![
        AccountPostState::new(caller.account),
        AccountPostState::new(protocol_parameters.account),
        AccountPostState::new(accumulator_post),
        AccountPostState::new(clock.account),
    ];

    (post_states, vec![])
}
